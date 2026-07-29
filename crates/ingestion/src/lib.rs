//! Bounded Context Inbox intake before format-specific understanding.

mod domain;
mod service;

pub use domain::{
    ArchiveInput, ArchiveReceipt, ArchiveRepository, ArchiveStatus, ArchivedEvidence,
    CandidateKind, FileObservation, ImportOutcome, ImportPolicy, IntakeDecision, RejectReason,
    UnparsedReason, evaluate_observations,
};
pub use service::{ImportError, ingest_inbox_file};
