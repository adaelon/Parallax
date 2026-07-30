use eam_core::{Claim, ClaimId, RepositoryError, Timestamp};

use crate::{MemoryId, MemoryVersion, ValidatedMemoryProposal};

pub trait LongTermMemoryRepository {
    /// Resolves one immutable source claim from the three ledgers.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the ledger cannot be queried.
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError>;

    /// Atomically appends one new memory or one successor version.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when source references or version continuity
    /// changed after validation, or the complete version cannot be committed.
    fn append_memory(
        &mut self,
        proposal: ValidatedMemoryProposal,
        formed_at: Timestamp,
    ) -> Result<MemoryVersion, RepositoryError>;

    /// Loads the latest version of one stable memory identity.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted memory state is invalid.
    fn current_memory(&self, id: MemoryId) -> Result<Option<MemoryVersion>, RepositoryError>;

    /// Loads every version of one memory in version order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted memory state is invalid.
    fn memory_versions(&self, id: MemoryId) -> Result<Vec<MemoryVersion>, RepositoryError>;

    /// Loads all versions ordered by memory identity and version.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted memory state is invalid.
    fn all_memory_versions(&self) -> Result<Vec<MemoryVersion>, RepositoryError>;
}
