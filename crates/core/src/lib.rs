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
    ActiveRelationalConstraint, AgreementWithdrawal, AgreementWithdrawalActor,
    AgreementWithdrawalProposal, AgreementWithdrawalRejection, AgreementWithdrawalRejectionReason,
    ApplicableTime, Claim, ClaimCorrectionReceipt, ClaimId, ClaimOwner, ClaimStatus,
    ConversationEvidence, DecisionImpact, DisputeState, EvidenceCitation, EvidenceId,
    ForgetReceipt, ForgetRequest, ForgetTarget, FrozenEvidenceBlock, FrozenLedgerClaim,
    FrozenMemoryDispute, FrozenRetrievalWindow, G08_IMMEDIATE_SAFETY_FIXTURE_ID,
    G08_IMMEDIATE_SAFETY_QUOTE, IdentityField, IdentityPersonRepresentation,
    IdentityProfileChanges, IdentityProfileSnapshot, IdentityReflectivePurposeStatus,
    IdentityRevisionAuthorship, IdentityRevisionCommit, IdentityRevisionProposal,
    IdentityRevisionReceipt, IdentityRevisionRejection, IdentityRevisionRejectionReason,
    IdentityRuntimeContext, IdentityStateSnapshot, JudgmentProposal, JudgmentRejection,
    JudgmentRejectionReason, MAX_OPEN_REFLECTION_INVITATIONS, MAX_REFLECTION_EVIDENCE_REFS,
    MAX_REFLECTION_OBSERVATION_BYTES, MAX_REFLECTION_TOPIC_BYTES, MAX_REFLECTION_WHY_NOW_BYTES,
    PatternMaturityCommitOutcome, PatternMaturityProposal, PatternMaturityReceipt,
    PatternMaturityWriteRejection, PatternMaturityWriteRejectionReason, PersonTurnClassification,
    REFLECTION_DEFER_MILLIS, REFLECTION_PROACTIVE_COOLDOWN_MILLIS,
    REFLECTION_SCHEDULE_CONTRACT_VERSION, ReflectionDecision, ReflectionDelivery,
    ReflectionImportance, ReflectionInvitation, ReflectionInvitationBasis, ReflectionInvitationId,
    ReflectionInvitationProposal, ReflectionInvitationReceipt, ReflectionInvitationRejection,
    ReflectionInvitationRejectionReason, ReflectionInvitationState, ReflectionOpportunity,
    ReflectionRuntimeContext, ReflectionRuntimeDisposition, ReflectionTransitionError,
    RelationalConstraintDeparture, RelationalConstraintDepartureRejection,
    RelationalConstraintDepartureRejectionReason, RelationalConstraintError,
    RelationalConstraintPriority, RetrievalSnapshot, RetrievedContextItem, RuntimeRequest,
    RuntimeResponse, SessionId, SharedAgreementAssent, SharedAgreementAssentRejection,
    SharedAgreementAssentRejectionReason, SharedAgreementCandidate, SharedAgreementCandidateId,
    SharedAgreementCandidateStatus, SharedAgreementDecision, SharedAgreementResolution,
    SharedAgreementRevision, SharedExperience, SharedExperienceKind, SharedExperienceProposal,
    SharedExperienceRejection, SharedExperienceRejectionReason, SourceCurrentness, Speaker,
    StructuredOperationRejection, StructuredOperationRejectionReason, Timestamp, TurnOutcome,
    Uncertainty, UnsupportedStructuredOperation, WorkingContext, WorkingContextError,
    agreement_is_active_at, decide_reflection_invitation, offer_reflection_invitation,
    reflection_delivery,
};
pub use in_memory::InMemoryRepository;
pub use memory_loop::{CoreError, MemoryCore};
pub use ports::{
    ClaimCorrectionRepository, Clock, CounterpartRuntime, ForgetRepository,
    IdentityEvolutionRepository, MemoryRepository, ReflectionInvitationRepository, RepositoryError,
    RuntimeError, RuntimeErrorKind, SharedExperienceRepository, SystemClock,
};
pub use scripted_runtime::{IncrementingClock, ScriptedRuntime};
