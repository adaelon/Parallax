//! Bounded Context Inbox intake before format-specific understanding.

mod domain;
mod evidence;
mod service;

pub use domain::{
    ArchiveInput, ArchiveReceipt, ArchiveRepository, ArchiveStatus, ArchivedEvidence,
    CandidateKind, FileObservation, ImportOutcome, ImportPolicy, IntakeDecision,
    MarkdownArchiveRepository, MarkdownParseAttempt, MarkdownParseStart, MarkdownParseState,
    RejectReason, UnparsedReason, evaluate_observations,
};
pub use evidence::{
    AcceptedMarkdownSource, CanonicalEvidenceBlockSource, EvidenceBlock, EvidenceBlockDraft,
    EvidenceBlockId, EvidenceBlockMetadata, EvidenceBlockQueryRepository, EvidenceBlockRef,
    EvidenceBlockView, EvidenceError, EvidenceExtractionRepository, ExtractionRevision,
    ExtractionRevisionId, MARKDOWN_LOCATOR_VERSION, MarkdownLocator, MarkdownLocatorValue,
    MaterializedExtraction, NATIVE_NAVIGATION_UNAVAILABLE, SourceAnchor, UiTextRange,
    ValidatedExtraction, project_utf8_span_to_utf16, resolve_native_navigation,
    validate_accepted_markdown,
};
pub use service::{
    EvidenceQueryError, ExtractionProcessError, ImportError, MarkdownProcessError,
    MarkdownProcessingOutcome, ingest_inbox_file, materialize_accepted_markdown,
    open_evidence_block, process_archived_markdown,
};
