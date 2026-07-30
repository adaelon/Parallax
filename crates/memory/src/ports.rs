use eam_core::{Claim, ClaimId, ConversationEvidence, EvidenceId, RepositoryError, Timestamp};

use crate::{
    MemoryDispute, MemoryDisputeId, MemoryDisputeResolution, MemoryId, MemoryVersion,
    ValidatedMemoryDispute, ValidatedMemoryDisputeReview, ValidatedMemoryProposal,
};

pub trait LongTermMemoryRepository {
    /// Resolves one immutable source claim from the three ledgers.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the ledger cannot be queried.
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError>;

    /// Resolves immutable conversation evidence used by a dispute or review.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when evidence cannot be queried.
    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError>;

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

    /// Atomically appends a person dispute and moves the target version to
    /// `DISPUTED`.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when version continuity, evidence, or state
    /// changed after validation.
    fn append_memory_dispute(
        &mut self,
        dispute: ValidatedMemoryDispute,
        raised_at: Timestamp,
    ) -> Result<MemoryDispute, RepositoryError>;

    /// Loads one dispute by stable identity.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted dispute state is invalid.
    fn memory_dispute(&self, id: MemoryDisputeId)
    -> Result<Option<MemoryDispute>, RepositoryError>;

    /// Loads every dispute for one stable memory in creation order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted dispute state is invalid.
    fn memory_disputes(&self, id: MemoryId) -> Result<Vec<MemoryDispute>, RepositoryError>;

    /// Atomically records a counterpart review and applies its maintained,
    /// retracted, or revised memory transition.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the dispute is closed, state is stale, or
    /// the whole transition cannot commit.
    fn complete_memory_dispute(
        &mut self,
        review: ValidatedMemoryDisputeReview,
        reviewed_at: Timestamp,
    ) -> Result<MemoryDisputeResolution, RepositoryError>;

    /// Returns current retracted memories whose normalized statement is an
    /// exact match, paired with the source claims used before retraction.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted memory state cannot be queried.
    fn retracted_memory_sources(
        &self,
        statement: &str,
    ) -> Result<Vec<(MemoryId, Vec<ClaimId>)>, RepositoryError>;
}
