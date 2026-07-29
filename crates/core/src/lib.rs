//! Trusted domain core for the `evrything-about-me` memory loop.
//!
//! S01 deliberately exposes only in-memory adapters. Persistence, encryption,
//! identity creation, real model access, long-term memory, UI, and ingestion
//! belong to later implementation slices.

mod domain;
mod in_memory;
mod memory_loop;
mod ports;
mod scripted_runtime;

pub use domain::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, EvidenceCitation, EvidenceId,
    JudgmentProposal, JudgmentRejection, JudgmentRejectionReason, PersonTurnClassification,
    RuntimeRequest, RuntimeResponse, SessionId, Speaker, StructuredOperationRejection,
    StructuredOperationRejectionReason, Timestamp, TurnOutcome, Uncertainty,
    UnsupportedStructuredOperation, WorkingContext,
};
pub use in_memory::InMemoryRepository;
pub use memory_loop::{CoreError, MemoryCore};
pub use ports::{
    Clock, CounterpartRuntime, MemoryRepository, RepositoryError, RuntimeError, RuntimeErrorKind,
};
pub use scripted_runtime::{IncrementingClock, ScriptedRuntime};
