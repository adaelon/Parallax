//! Trusted identity formation rules for the digital counterpart.

mod domain;
mod in_memory;
mod ports;
mod scripted_runtime;
mod service;

pub use domain::{
    IdentityAuthorship, IdentityProfile, IdentityStateVersion, InitialIdentityProposal,
    InitialIdentityRequest, InitialSelfIntroduction, IntroductionAnswer, IntroductionItem,
    PersonRepresentation, ReflectivePurposeStatus, SelfIntroductionCategory,
};
pub use in_memory::InMemoryIdentityRepository;
pub use ports::{IdentityRepository, IdentityRuntime};
pub use scripted_runtime::ScriptedIdentityRuntime;
pub use service::{
    IdentityError, IdentityField, IdentityFormation, IdentityProposalRejectionReason,
};
