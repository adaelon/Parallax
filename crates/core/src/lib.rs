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
    ActiveRelationalConstraint, ApplicableTime, Claim, ClaimCorrectionReceipt, ClaimId, ClaimOwner,
    ClaimStatus, ConversationEvidence, DecisionImpact, DisputeState, EvidenceCitation, EvidenceId,
    ForgetReceipt, ForgetRequest, ForgetTarget, FrozenEvidenceBlock, FrozenLedgerClaim,
    FrozenMemoryDispute, FrozenRetrievalWindow, JudgmentProposal, JudgmentRejection,
    JudgmentRejectionReason, PersonTurnClassification, RelationalConstraintDeparture,
    RelationalConstraintDepartureRejection, RelationalConstraintDepartureRejectionReason,
    RelationalConstraintError, RelationalConstraintPriority, RetrievalSnapshot,
    RetrievedContextItem, RuntimeRequest, RuntimeResponse, SessionId, SharedAgreementAssent,
    SharedAgreementAssentRejection, SharedAgreementAssentRejectionReason, SharedAgreementCandidate,
    SharedAgreementCandidateId, SharedAgreementCandidateStatus, SharedAgreementDecision,
    SharedAgreementResolution, SharedAgreementRevision, SharedExperience, SharedExperienceKind,
    SharedExperienceProposal, SharedExperienceRejection, SharedExperienceRejectionReason,
    SourceCurrentness, Speaker, StructuredOperationRejection, StructuredOperationRejectionReason,
    Timestamp, TurnOutcome, Uncertainty, UnsupportedStructuredOperation, WorkingContext,
    WorkingContextError,
};
pub use in_memory::InMemoryRepository;
pub use memory_loop::{CoreError, MemoryCore};
pub use ports::{
    ClaimCorrectionRepository, Clock, CounterpartRuntime, ForgetRepository, MemoryRepository,
    RepositoryError, RuntimeError, RuntimeErrorKind, SharedExperienceRepository, SystemClock,
};
pub use scripted_runtime::{IncrementingClock, ScriptedRuntime};
