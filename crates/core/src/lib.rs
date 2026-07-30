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
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, DecisionImpact, DisputeState,
    EvidenceCitation, EvidenceId, FrozenEvidenceBlock, FrozenLedgerClaim, FrozenMemoryDispute,
    FrozenRetrievalWindow, JudgmentProposal, JudgmentRejection, JudgmentRejectionReason,
    PersonTurnClassification, RetrievalSnapshot, RetrievedContextItem, RuntimeRequest,
    RuntimeResponse, SessionId, SourceCurrentness, Speaker, StructuredOperationRejection,
    StructuredOperationRejectionReason, Timestamp, TurnOutcome, Uncertainty,
    UnsupportedStructuredOperation, WorkingContext, WorkingContextError,
};
pub use in_memory::InMemoryRepository;
pub use memory_loop::{CoreError, MemoryCore};
pub use ports::{
    Clock, CounterpartRuntime, MemoryRepository, RepositoryError, RuntimeError, RuntimeErrorKind,
    SystemClock,
};
pub use scripted_runtime::{IncrementingClock, ScriptedRuntime};
