use std::collections::BTreeMap;

use crate::{
    Claim, ClaimCorrectionReceipt, ClaimCorrectionRepository, ClaimId, ClaimOwner, ClaimStatus,
    ConversationEvidence, EvidenceId, MemoryRepository, RepositoryError,
};

#[derive(Debug)]
pub struct InMemoryRepository {
    next_evidence_id: u64,
    next_claim_id: u64,
    evidence: BTreeMap<EvidenceId, ConversationEvidence>,
    claims: BTreeMap<ClaimId, Claim>,
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
        }
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
