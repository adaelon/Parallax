//! Bounded Context Inbox intake before format-specific understanding.

mod domain;
mod service;

pub use domain::{
    ArchiveInput, ArchiveReceipt, ArchiveRepository, ArchiveStatus, ArchivedEvidence,
    CandidateKind, FileObservation, ImportOutcome, ImportPolicy, IntakeDecision,
    MarkdownArchiveRepository, MarkdownParseAttempt, MarkdownParseStart, MarkdownParseState,
    RejectReason, UnparsedReason, evaluate_observations,
};
pub use service::{
    ImportError, MarkdownProcessError, MarkdownProcessingOutcome, ingest_inbox_file,
    process_archived_markdown,
};
