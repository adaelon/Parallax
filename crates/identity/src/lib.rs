//! Trusted identity formation rules for the digital counterpart.

mod domain;
mod in_memory;
mod ports;
mod presence;
mod scripted_runtime;
mod self_bundle;
mod service;

pub use domain::{
    CounterpartInconsistencyReason, CounterpartReadiness, IdentityAuthorship, IdentityProfile,
    IdentityStateVersion, InitialIdentityProposal, InitialIdentityRequest, InitialSelfIntroduction,
    IntroductionAnswer, IntroductionItem, PersonRepresentation, ReflectivePurposeStatus,
    SelfIntroductionCategory,
};
pub use in_memory::InMemoryIdentityRepository;
pub use ports::{
    CounterpartRepository, IdentityRepository, IdentityRuntime, SelfBundleRepository, WakeWork,
};
pub use presence::{
    PresenceCoordinator, PresenceError, WakeInterruption, WakeInterruptionReason, WakeOutcome,
};
pub use scripted_runtime::ScriptedIdentityRuntime;
pub use self_bundle::{
    PresenceState, SelfBundleListField, SelfBundleState, SelfBundleValidationError,
    SelfBundleVersion, WakeCommit, WakeExit, WakeTrigger,
};
pub use service::{
    IdentityError, IdentityField, IdentityFormation, IdentityProposalRejectionReason,
};
