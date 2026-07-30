use std::collections::BTreeMap;

use eam_core::{Claim, ClaimId, RepositoryError, Timestamp};

use crate::{
    LongTermMemoryRepository, MemoryId, MemoryStatus, MemoryTarget, MemoryVersion,
    ValidatedMemoryProposal,
};

#[derive(Debug, Default)]
pub struct InMemoryLongTermMemoryRepository {
    claims: BTreeMap<ClaimId, Claim>,
    memories: BTreeMap<MemoryId, Vec<MemoryVersion>>,
    next_memory_id: u64,
}

impl InMemoryLongTermMemoryRepository {
    #[must_use]
    pub fn new() -> Self {
        Self {
            claims: BTreeMap::new(),
            memories: BTreeMap::new(),
            next_memory_id: 1,
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
}

impl LongTermMemoryRepository for InMemoryLongTermMemoryRepository {
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        Ok(self.claims.get(&id).cloned())
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
        );
        self.memories.entry(id).or_default().push(stored.clone());
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

    fn all_memory_versions(&self) -> Result<Vec<MemoryVersion>, RepositoryError> {
        Ok(self
            .memories
            .values()
            .flat_map(|versions| versions.iter().cloned())
            .collect())
    }
}
