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
    MAX_MEMORY_SOURCES, MAX_MEMORY_TEXT_BYTES, MemoryBasis, MemoryConfidence, MemoryId, MemoryKind,
    MemoryProposal, MemoryStatus, MemorySubject, MemoryTarget, MemoryVersion,
    ValidatedMemoryProposal,
};
pub use in_memory::InMemoryLongTermMemoryRepository;
pub use ports::LongTermMemoryRepository;
pub use service::{
    MemoryError, MemoryMaintenance, MemoryProposalField, MemoryProposalRejectionReason,
};
