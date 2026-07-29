use std::collections::BTreeMap;

use crate::{Claim, ClaimId, ConversationEvidence, EvidenceId, MemoryRepository, RepositoryError};

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
