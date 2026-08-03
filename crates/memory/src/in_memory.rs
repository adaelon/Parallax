use std::collections::BTreeMap;

use eam_core::{Claim, ClaimId, ConversationEvidence, EvidenceId, RepositoryError, Timestamp};

use crate::{
    LongTermMemoryRepository, MemoryDispute, MemoryDisputeId, MemoryDisputeOutcome,
    MemoryDisputeResolution, MemoryDisputeReviewRecord, MemoryId, MemoryStatus, MemoryTarget,
    MemoryVersion, PatternMaturityRecord, ValidatedMemoryDispute, ValidatedMemoryDisputeReview,
    ValidatedMemoryProposal, ValidatedPatternMaturityProposal,
};

#[derive(Debug, Default)]
pub struct InMemoryLongTermMemoryRepository {
    evidence: BTreeMap<EvidenceId, ConversationEvidence>,
    claims: BTreeMap<ClaimId, Claim>,
    memories: BTreeMap<MemoryId, Vec<MemoryVersion>>,
    maturity_records: BTreeMap<MemoryId, Vec<PatternMaturityRecord>>,
    disputes: BTreeMap<MemoryDisputeId, MemoryDispute>,
    next_memory_id: u64,
    next_dispute_id: u64,
}

impl InMemoryLongTermMemoryRepository {
    #[must_use]
    pub fn new() -> Self {
        Self {
            evidence: BTreeMap::new(),
            claims: BTreeMap::new(),
            memories: BTreeMap::new(),
            maturity_records: BTreeMap::new(),
            disputes: BTreeMap::new(),
            next_memory_id: 1,
            next_dispute_id: 1,
        }
    }

    /// Seeds immutable ledger claims for domain tests or local orchestration.
    ///
    /// # Errors
    ///
    /// Rejects duplicate claim identifiers.
    pub fn with_claims(claims: impl IntoIterator<Item = Claim>) -> Result<Self, RepositoryError> {
        let mut repository = Self::new();
        for claim in claims {
            if repository.claims.insert(claim.id(), claim).is_some() {
                return Err(RepositoryError::new("duplicate claim id"));
            }
        }
        Ok(repository)
    }

    /// Seeds immutable evidence and ledger claims for dispute-domain tests.
    ///
    /// # Errors
    ///
    /// Rejects duplicate evidence or claim identifiers.
    pub fn with_evidence_and_claims(
        evidence: impl IntoIterator<Item = ConversationEvidence>,
        claims: impl IntoIterator<Item = Claim>,
    ) -> Result<Self, RepositoryError> {
        let mut repository = Self::with_claims(claims)?;
        for item in evidence {
            if repository.evidence.insert(item.id(), item).is_some() {
                return Err(RepositoryError::new("duplicate evidence id"));
            }
        }
        Ok(repository)
    }
}

impl LongTermMemoryRepository for InMemoryLongTermMemoryRepository {
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        Ok(self.claims.get(&id).cloned())
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.get(&id).cloned())
    }

    fn append_memory(
        &mut self,
        proposal: ValidatedMemoryProposal,
        formed_at: Timestamp,
    ) -> Result<MemoryVersion, RepositoryError> {
        let (id, version, predecessor_version) = match proposal.target() {
            MemoryTarget::New => {
                let id = MemoryId::new(self.next_memory_id)
                    .ok_or_else(|| RepositoryError::new("invalid memory id"))?;
                self.next_memory_id = self
                    .next_memory_id
                    .checked_add(1)
                    .ok_or_else(|| RepositoryError::new("memory identifier space exhausted"))?;
                (id, 1, None)
            }
            MemoryTarget::Revise {
                memory_id,
                expected_version,
            } => {
                let versions = self
                    .memories
                    .get_mut(&memory_id)
                    .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
                let current = versions
                    .last_mut()
                    .ok_or_else(|| RepositoryError::new("memory has no versions"))?;
                if current.version() != expected_version {
                    return Err(RepositoryError::new("stale memory version"));
                }
                let version = expected_version
                    .checked_add(1)
                    .ok_or_else(|| RepositoryError::new("memory version space exhausted"))?;
                current.set_status(MemoryStatus::Superseded);
                (memory_id, version, Some(expected_version))
            }
        };
        let stored = MemoryVersion::restore(
            id,
            version,
            predecessor_version,
            proposal.statement().to_owned(),
            proposal.subject(),
            proposal.kind(),
            proposal.source_claim_ids().to_vec(),
            proposal.applicable_time(),
            proposal.confidence(),
            proposal.salience_reason().to_owned(),
            proposal.basis(),
            proposal.initial_status(),
            formed_at,
            proposal.pattern_counterexample_review().cloned(),
        );
        self.memories.entry(id).or_default().push(stored.clone());
        Ok(stored)
    }

    fn append_pattern_maturity(
        &mut self,
        proposal: ValidatedPatternMaturityProposal,
        proposed_at: Timestamp,
    ) -> Result<MemoryVersion, RepositoryError> {
        let versions = self
            .memories
            .get_mut(&proposal.memory_id())
            .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
        let current = versions
            .last_mut()
            .ok_or_else(|| RepositoryError::new("memory has no versions"))?;
        if current.version() != proposal.expected_version() {
            return Err(RepositoryError::new("stale memory version"));
        }
        if current.status() != MemoryStatus::ProvisionalPattern
            || current.basis() != crate::MemoryBasis::PatternCandidate
        {
            return Err(RepositoryError::new(
                "only a provisional pattern can mature",
            ));
        }
        let previous = current.clone();
        let next_version = current
            .version()
            .checked_add(1)
            .ok_or_else(|| RepositoryError::new("memory version space exhausted"))?;
        current.set_status(MemoryStatus::Superseded);
        let stored = MemoryVersion::restore(
            previous.id(),
            next_version,
            Some(previous.version()),
            previous.statement().to_owned(),
            previous.subject(),
            previous.kind(),
            proposal.all_source_claim_ids().to_vec(),
            previous.applicable_time(),
            previous.confidence(),
            previous.salience_reason().to_owned(),
            previous.basis(),
            MemoryStatus::SupportedCounterpartView,
            proposed_at,
            Some(proposal.counterexample_review_ref().clone()),
        );
        versions.push(stored.clone());
        self.maturity_records
            .entry(previous.id())
            .or_default()
            .push(PatternMaturityRecord::restore(
                previous.id(),
                previous.version(),
                next_version,
                proposal.new_support_claim_ids().to_vec(),
                proposal.counter_evidence_refs().to_vec(),
                proposal.counterexample_review_ref().clone(),
                proposal.discussion_evidence_refs().to_vec(),
                proposal.rationale().to_owned(),
                proposed_at,
            ));
        Ok(stored)
    }

    fn current_memory(&self, id: MemoryId) -> Result<Option<MemoryVersion>, RepositoryError> {
        Ok(self
            .memories
            .get(&id)
            .and_then(|versions| versions.last())
            .cloned())
    }

    fn memory_versions(&self, id: MemoryId) -> Result<Vec<MemoryVersion>, RepositoryError> {
        Ok(self.memories.get(&id).cloned().unwrap_or_default())
    }

    fn pattern_maturity_records(
        &self,
        id: MemoryId,
    ) -> Result<Vec<PatternMaturityRecord>, RepositoryError> {
        Ok(self.maturity_records.get(&id).cloned().unwrap_or_default())
    }

    fn all_memory_versions(&self) -> Result<Vec<MemoryVersion>, RepositoryError> {
        Ok(self
            .memories
            .values()
            .flat_map(|versions| versions.iter().cloned())
            .collect())
    }

    fn append_memory_dispute(
        &mut self,
        dispute: ValidatedMemoryDispute,
        raised_at: Timestamp,
    ) -> Result<MemoryDispute, RepositoryError> {
        let versions = self
            .memories
            .get_mut(&dispute.memory_id())
            .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
        let current = versions
            .last_mut()
            .ok_or_else(|| RepositoryError::new("memory has no versions"))?;
        if current.version() != dispute.memory_version() {
            return Err(RepositoryError::new("stale memory version"));
        }
        if self.disputes.values().any(|stored| {
            stored.memory_id() == dispute.memory_id()
                && stored.memory_version() == dispute.memory_version()
                && stored.outcome() == MemoryDisputeOutcome::Open
        }) {
            return Err(RepositoryError::new(
                "memory version already has an open dispute",
            ));
        }
        current.set_status(MemoryStatus::Disputed);
        let id = MemoryDisputeId::new(self.next_dispute_id)
            .ok_or_else(|| RepositoryError::new("invalid memory dispute id"))?;
        self.next_dispute_id = self
            .next_dispute_id
            .checked_add(1)
            .ok_or_else(|| RepositoryError::new("memory dispute identifier space exhausted"))?;
        let stored = MemoryDispute::restore(
            id,
            dispute.memory_id(),
            dispute.memory_version(),
            dispute.reason().to_owned(),
            dispute.counter_evidence().to_vec(),
            raised_at,
            MemoryDisputeOutcome::Open,
            None,
            None,
        );
        self.disputes.insert(id, stored.clone());
        Ok(stored)
    }

    fn memory_dispute(
        &self,
        id: MemoryDisputeId,
    ) -> Result<Option<MemoryDispute>, RepositoryError> {
        Ok(self.disputes.get(&id).cloned())
    }

    fn memory_disputes(&self, id: MemoryId) -> Result<Vec<MemoryDispute>, RepositoryError> {
        Ok(self
            .disputes
            .values()
            .filter(|dispute| dispute.memory_id() == id)
            .cloned()
            .collect())
    }

    fn complete_memory_dispute(
        &mut self,
        review: ValidatedMemoryDisputeReview,
        reviewed_at: Timestamp,
    ) -> Result<MemoryDisputeResolution, RepositoryError> {
        let dispute = self
            .disputes
            .get(&review.dispute_id())
            .cloned()
            .ok_or_else(|| RepositoryError::new("memory dispute does not exist"))?;
        if dispute.outcome() != MemoryDisputeOutcome::Open {
            return Err(RepositoryError::new("memory dispute is already resolved"));
        }
        let versions = self
            .memories
            .get_mut(&dispute.memory_id())
            .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
        let current = versions
            .last_mut()
            .ok_or_else(|| RepositoryError::new("memory has no versions"))?;
        if current.version() != dispute.memory_version()
            || current.status() != MemoryStatus::Disputed
        {
            return Err(RepositoryError::new("memory dispute state is stale"));
        }

        let (resolved_memory, revised_version) = match review.outcome() {
            MemoryDisputeOutcome::Maintained => (current.clone(), None),
            MemoryDisputeOutcome::Retracted => {
                current.set_status(MemoryStatus::Retracted);
                (current.clone(), None)
            }
            MemoryDisputeOutcome::Weakened => {
                current.set_status(MemoryStatus::Weakened);
                (current.clone(), None)
            }
            MemoryDisputeOutcome::Revised => {
                let proposal = review
                    .revision()
                    .ok_or_else(|| RepositoryError::new("revised dispute has no proposal"))?;
                let MemoryTarget::Revise {
                    memory_id,
                    expected_version,
                } = proposal.target()
                else {
                    return Err(RepositoryError::new("dispute revision target is invalid"));
                };
                if memory_id != dispute.memory_id() || expected_version != dispute.memory_version()
                {
                    return Err(RepositoryError::new("dispute revision target is stale"));
                }
                current.set_status(MemoryStatus::Superseded);
                let next_version = expected_version
                    .checked_add(1)
                    .ok_or_else(|| RepositoryError::new("memory version space exhausted"))?;
                let stored = MemoryVersion::restore(
                    memory_id,
                    next_version,
                    Some(expected_version),
                    proposal.statement().to_owned(),
                    proposal.subject(),
                    proposal.kind(),
                    proposal.source_claim_ids().to_vec(),
                    proposal.applicable_time(),
                    proposal.confidence(),
                    proposal.salience_reason().to_owned(),
                    proposal.basis(),
                    proposal.initial_status(),
                    reviewed_at,
                    proposal.pattern_counterexample_review().cloned(),
                );
                versions.push(stored.clone());
                (stored, Some(next_version))
            }
            MemoryDisputeOutcome::Open => {
                return Err(RepositoryError::new("review outcome cannot remain open"));
            }
        };
        let review_record = MemoryDisputeReviewRecord::restore(
            review.outcome(),
            review.rationale().to_owned(),
            review.evidence().to_vec(),
            reviewed_at,
        );
        let stored_dispute = self
            .disputes
            .get_mut(&review.dispute_id())
            .expect("the dispute was resolved above");
        stored_dispute.set_review(review_record, revised_version);
        Ok(MemoryDisputeResolution::new(
            stored_dispute.clone(),
            resolved_memory,
        ))
    }

    fn retracted_memory_sources(
        &self,
        statement: &str,
    ) -> Result<Vec<(MemoryId, Vec<ClaimId>)>, RepositoryError> {
        Ok(self
            .memories
            .iter()
            .filter_map(|(id, versions)| {
                let current = versions.last()?;
                (current.status() == MemoryStatus::Retracted
                    && current.statement().trim() == statement.trim())
                .then(|| (*id, current.source_claim_ids().to_vec()))
            })
            .collect())
    }
}
