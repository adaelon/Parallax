//! Explicit, sourced, and versioned long-term-memory maintenance.
//!
//! Ledger entries and deep-understanding projections never become memories by
//! calling this crate implicitly. A counterpart-authored [`MemoryProposal`]
//! must pass deterministic source, attribution, time, confidence, and
//! cross-task-retention checks before a repository may append a version.

mod domain;
mod in_memory;
mod ports;
mod service;

pub use domain::{
    MAX_DISPUTE_EVIDENCE, MAX_MEMORY_SOURCES, MAX_MEMORY_TEXT_BYTES, MemoryBasis, MemoryConfidence,
    MemoryDispute, MemoryDisputeId, MemoryDisputeOutcome, MemoryDisputeRequest,
    MemoryDisputeResolution, MemoryDisputeReview, MemoryDisputeReviewDecision,
    MemoryDisputeReviewRecord, MemoryId, MemoryKind, MemoryProposal, MemoryStatus, MemorySubject,
    MemoryTarget, MemoryVersion, ValidatedMemoryDispute, ValidatedMemoryDisputeReview,
    ValidatedMemoryProposal,
};
pub use in_memory::InMemoryLongTermMemoryRepository;
pub use ports::LongTermMemoryRepository;
pub use service::{
    MemoryDisputeRejectionReason, MemoryDisputeReviewRejectionReason, MemoryError,
    MemoryMaintenance, MemoryProposalField, MemoryProposalRejectionReason,
};
