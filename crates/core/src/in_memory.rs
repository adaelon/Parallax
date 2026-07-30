use std::collections::BTreeMap;

use crate::{
    Claim, ClaimCorrectionReceipt, ClaimCorrectionRepository, ClaimId, ClaimOwner, ClaimStatus,
    ConversationEvidence, EvidenceId, ForgetReceipt, ForgetRepository, ForgetTarget,
    MemoryRepository, RepositoryError, Timestamp,
};

#[derive(Debug)]
pub struct InMemoryRepository {
    next_evidence_id: u64,
    next_claim_id: u64,
    evidence: BTreeMap<EvidenceId, ConversationEvidence>,
    claims: BTreeMap<ClaimId, Claim>,
    deletion_intents: BTreeMap<ForgetTarget, ForgetReceipt>,
    next_deletion_intent_id: u64,
}

impl Default for InMemoryRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryRepository {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_evidence_id: 1,
            next_claim_id: 1,
            evidence: BTreeMap::new(),
            claims: BTreeMap::new(),
            deletion_intents: BTreeMap::new(),
            next_deletion_intent_id: 1,
        }
    }
}

impl ForgetRepository for InMemoryRepository {
    fn commit_forget(
        &mut self,
        target: ForgetTarget,
        _requested_at: Timestamp,
    ) -> Result<Option<ForgetReceipt>, RepositoryError> {
        if let Some(receipt) = self.deletion_intents.get(&target) {
            return Ok(Some(*receipt));
        }
        let ForgetTarget::ConversationEvidence(evidence_id) = target else {
            return Ok(None);
        };
        if !self.evidence.contains_key(&evidence_id) {
            return Ok(None);
        }

        let mut affected_claims = self
            .claims
            .values()
            .filter(|claim| {
                claim
                    .support()
                    .iter()
                    .any(|citation| citation.evidence_id() == evidence_id)
            })
            .map(Claim::id)
            .collect::<Vec<_>>();
        loop {
            let previous_len = affected_claims.len();
            for claim in self.claims.values() {
                let linked = claim
                    .supersedes()
                    .is_some_and(|id| affected_claims.contains(&id))
                    || claim
                        .superseded_by()
                        .is_some_and(|id| affected_claims.contains(&id));
                if linked && !affected_claims.contains(&claim.id()) {
                    affected_claims.push(claim.id());
                }
            }
            if affected_claims.len() == previous_len {
                break;
            }
        }
        for claim_id in &affected_claims {
            self.claims.remove(claim_id);
        }
        self.evidence.remove(&evidence_id);

        let receipt = ForgetReceipt::new(
            self.next_deletion_intent_id,
            target,
            1 + affected_claims.len(),
            0,
            0,
        );
        self.next_deletion_intent_id += 1;
        self.deletion_intents.insert(target, receipt);
        Ok(Some(receipt))
    }
}

impl MemoryRepository for InMemoryRepository {
    fn next_evidence_id(&mut self) -> EvidenceId {
        let id = EvidenceId::from_raw(self.next_evidence_id);
        self.next_evidence_id += 1;
        id
    }

    fn next_claim_id(&mut self) -> ClaimId {
        let id = ClaimId::from_raw(self.next_claim_id);
        self.next_claim_id += 1;
        id
    }

    fn append_evidence(&mut self, evidence: ConversationEvidence) -> Result<(), RepositoryError> {
        if self.evidence.insert(evidence.id(), evidence).is_some() {
            return Err(RepositoryError::new("duplicate evidence id"));
        }
        Ok(())
    }

    fn append_claim(&mut self, claim: Claim) -> Result<(), RepositoryError> {
        if self.claims.insert(claim.id(), claim).is_some() {
            return Err(RepositoryError::new("duplicate claim id"));
        }
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.get(&id).cloned())
    }

    fn all_evidence(&self) -> Result<Vec<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.values().cloned().collect())
    }

    fn all_claims(&self) -> Result<Vec<Claim>, RepositoryError> {
        Ok(self.claims.values().cloned().collect())
    }
}

impl ClaimCorrectionRepository for InMemoryRepository {
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        Ok(self.claims.get(&id).cloned())
    }

    fn commit_person_fact_correction(
        &mut self,
        evidence: ConversationEvidence,
        replacement: Claim,
    ) -> Result<ClaimCorrectionReceipt, RepositoryError> {
        let superseded_id = replacement
            .supersedes()
            .ok_or_else(|| RepositoryError::new("correction claim has no predecessor"))?;
        let previous = self
            .claims
            .get(&superseded_id)
            .ok_or_else(|| RepositoryError::new("claim does not exist"))?;
        if previous.owner() != ClaimOwner::Person || replacement.owner() != ClaimOwner::Person {
            return Err(RepositoryError::new("only person claims can be corrected"));
        }
        if previous.status() != ClaimStatus::Current {
            return Err(RepositoryError::new("claim is not current"));
        }
        if self.evidence.contains_key(&evidence.id()) || self.claims.contains_key(&replacement.id())
        {
            return Err(RepositoryError::new("duplicate correction identifier"));
        }

        self.evidence.insert(evidence.id(), evidence.clone());
        self.claims
            .get_mut(&superseded_id)
            .expect("validated predecessor remains present")
            .mark_superseded_by(replacement.id());
        self.claims.insert(replacement.id(), replacement.clone());
        Ok(ClaimCorrectionReceipt::new(
            evidence.id(),
            superseded_id,
            replacement.id(),
            0,
            0,
            0,
            0,
        ))
    }
}
