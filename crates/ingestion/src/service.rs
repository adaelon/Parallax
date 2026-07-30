use std::{
    error::Error,
    fmt,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read},
    path::Path,
    thread,
};

use crate::{
    ArchiveInput, ArchiveRepository, ArchiveStatus, BLOCK_LINEAGE_RULE_VERSION,
    BlockLineageRepository, CandidateKind, EvidenceBlockQueryRepository, EvidenceBlockRef,
    EvidenceBlockView, EvidenceError, EvidenceExtractionRepository, FileObservation, ImportOutcome,
    ImportPolicy, IncrementalWorkItem, IncrementalWorkPlan, IntakeDecision, LineageBatch,
    LineageError, MarkdownArchiveRepository, MarkdownParseStart, MarkdownParseState,
    MaterializedExtraction, UnparsedReason, compute_block_lineage, evaluate_observations,
    validate_accepted_markdown,
};
use eam_markdown::{
    CONTRACT_VERSION, MarkdownParseError, ParseLimits, ParsedMarkdownV1, parse_markdown,
};

#[derive(Debug)]
pub enum ImportError<E> {
    Io(io::Error),
    Repository(E),
}

impl<E: fmt::Display> fmt::Display for ImportError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Context Inbox I/O failed: {error}"),
            Self::Repository(error) => write!(formatter, "archive persistence failed: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for ImportError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Repository(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub enum MarkdownProcessError<E> {
    Repository(E),
}

#[derive(Debug)]
pub enum ExtractionProcessError<E> {
    Repository(E),
    Evidence(EvidenceError),
}

#[derive(Debug)]
pub enum IncrementalProcessError<E> {
    Repository(E),
    Evidence(EvidenceError),
    Lineage(LineageError),
}

impl<E: fmt::Display> fmt::Display for IncrementalProcessError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => write!(formatter, "incremental persistence failed: {error}"),
            Self::Evidence(error) => write!(formatter, "accepted Markdown is invalid: {error}"),
            Self::Lineage(error) => write!(formatter, "block lineage is invalid: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for IncrementalProcessError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::Evidence(error) => Some(error),
            Self::Lineage(error) => Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncrementalMaterialization {
    extraction: MaterializedExtraction,
    lineage: Option<LineageBatch>,
    work_plan: IncrementalWorkPlan,
}

impl IncrementalMaterialization {
    #[must_use]
    pub const fn extraction(&self) -> &MaterializedExtraction {
        &self.extraction
    }

    #[must_use]
    pub const fn lineage(&self) -> Option<&LineageBatch> {
        self.lineage.as_ref()
    }

    #[must_use]
    pub const fn work_plan(&self) -> &IncrementalWorkPlan {
        &self.work_plan
    }
}

impl<E: fmt::Display> fmt::Display for ExtractionProcessError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => write!(formatter, "extraction persistence failed: {error}"),
            Self::Evidence(error) => write!(formatter, "accepted Markdown is invalid: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for ExtractionProcessError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::Evidence(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub enum EvidenceQueryError<E> {
    Repository(E),
    Evidence(EvidenceError),
}

impl<E: fmt::Display> fmt::Display for EvidenceQueryError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => write!(formatter, "evidence query failed: {error}"),
            Self::Evidence(error) => write!(formatter, "evidence reference is invalid: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for EvidenceQueryError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::Evidence(error) => Some(error),
        }
    }
}

impl<E: fmt::Display> fmt::Display for MarkdownProcessError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => write!(formatter, "Markdown persistence failed: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for MarkdownProcessError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownProcessingOutcome {
    Accepted {
        archive_id: u64,
        block_count: usize,
        relation_count: usize,
    },
    Rejected {
        archive_id: u64,
        reason: UnparsedReason,
    },
    NotRetried {
        archive_id: u64,
        state: MarkdownParseState,
    },
}

/// Observes, bounds, reads, and archives one Context Inbox path without parsing it.
///
/// # Errors
///
/// Returns an I/O error when the path cannot be inspected or read, or the
/// repository error when encrypted archive persistence fails.
pub fn ingest_inbox_file<R: ArchiveRepository>(
    repository: &mut R,
    path: &Path,
    policy: &ImportPolicy,
    oversized_approved: bool,
    archived_at_millis: i64,
) -> Result<ImportOutcome, ImportError<R::Error>> {
    if is_device_path(path) {
        return Ok(ImportOutcome::Rejected(
            crate::RejectReason::UnsupportedFileType,
        ));
    }
    let first = observe_path(path).map_err(ImportError::Io)?;
    if !policy.stability_window.is_zero() {
        thread::sleep(policy.stability_window);
    }
    let second = observe_path(path).map_err(ImportError::Io)?;
    match evaluate_observations(&first, &second, policy, oversized_approved) {
        IntakeDecision::Discovered => return Ok(ImportOutcome::Discovered),
        IntakeDecision::AwaitingApproval { bytes } => {
            return Ok(ImportOutcome::AwaitingApproval { bytes });
        }
        IntakeDecision::Rejected(reason) => return Ok(ImportOutcome::Rejected(reason)),
        IntakeDecision::Stable => {}
    }

    let mut file = open_without_following(path).map_err(ImportError::Io)?;
    let opened = observe_metadata(&file.metadata().map_err(ImportError::Io)?);
    match evaluate_observations(&second, &opened, policy, oversized_approved) {
        IntakeDecision::Stable => {}
        IntakeDecision::Discovered => return Ok(ImportOutcome::Discovered),
        IntakeDecision::AwaitingApproval { bytes } => {
            return Ok(ImportOutcome::AwaitingApproval { bytes });
        }
        IntakeDecision::Rejected(reason) => return Ok(ImportOutcome::Rejected(reason)),
    }

    let read_limit = policy.hard_import_limit_bytes.saturating_add(1);
    let capacity = usize::try_from(opened.length).unwrap_or(0);
    let mut content = Vec::new();
    content
        .try_reserve_exact(capacity)
        .map_err(|error| ImportError::Io(io::Error::other(error)))?;
    file.by_ref()
        .take(read_limit)
        .read_to_end(&mut content)
        .map_err(ImportError::Io)?;
    if u64::try_from(content.len()).unwrap_or(u64::MAX) > policy.hard_import_limit_bytes {
        return Ok(ImportOutcome::Rejected(
            crate::RejectReason::HardLimitExceeded,
        ));
    }

    let after_read = observe_path(path).map_err(ImportError::Io)?;
    if second != after_read || opened != after_read {
        return Ok(ImportOutcome::Discovered);
    }

    let source_locator = path.as_os_str().to_string_lossy();
    let status = archive_status(path);
    let receipt = repository
        .archive(ArchiveInput {
            source_locator: &source_locator,
            content: &content,
            status,
            archived_at_millis,
        })
        .map_err(ImportError::Repository)?;
    Ok(ImportOutcome::Archived(receipt))
}

/// Reprocesses one already encrypted Markdown archive under `eam-markdown-v1`.
///
/// `STARTED` is committed before authenticated plaintext is read. Every
/// expected decode or parse failure is then committed as one atomic rejection;
/// repository failures deliberately leave `STARTED` for startup recovery.
///
/// # Errors
///
/// Returns the repository error when attempt, object, acceptance, or rejection
/// persistence fails.
pub fn process_archived_markdown<R: MarkdownArchiveRepository>(
    repository: &mut R,
    archive_id: u64,
    limits: ParseLimits,
    started_at_millis: i64,
    finished_at_millis: i64,
) -> Result<MarkdownProcessingOutcome, MarkdownProcessError<R::Error>> {
    match repository
        .begin_markdown_parse(archive_id, CONTRACT_VERSION, started_at_millis)
        .map_err(MarkdownProcessError::Repository)?
    {
        MarkdownParseStart::Started => {}
        MarkdownParseStart::AlreadyAttempted(state) => {
            return Ok(MarkdownProcessingOutcome::NotRetried { archive_id, state });
        }
    }

    let content = repository
        .read_archived_content(archive_id)
        .map_err(MarkdownProcessError::Repository)?;
    let Ok(source) = std::str::from_utf8(&content) else {
        return reject_markdown(
            repository,
            archive_id,
            UnparsedReason::InvalidEncoding,
            finished_at_millis,
        );
    };
    let parsed = match parse_markdown(source, limits) {
        Ok(parsed) => parsed,
        Err(error) => {
            let reason = match error {
                MarkdownParseError::ResourceLimit(resource) => {
                    UnparsedReason::ResourceLimit(resource)
                }
                MarkdownParseError::InvalidStructure => UnparsedReason::InvalidStructure,
            };
            return reject_markdown(repository, archive_id, reason, finished_at_millis);
        }
    };
    accept_markdown(repository, archive_id, &parsed, finished_at_millis)
}

/// Converts one accepted S09 artifact into an immutable S10 extraction.
///
/// The authenticated archived Markdown remains the only canonical text. The
/// validated revision and all Core-owned blocks are committed atomically.
///
/// # Errors
///
/// Returns a repository error when the accepted input cannot be loaded or the
/// transaction fails, and an evidence error when the accepted input violates
/// the S10 contract.
pub fn materialize_accepted_markdown<R: EvidenceExtractionRepository>(
    repository: &mut R,
    evidence_id: u64,
    contract_version: &str,
) -> Result<MaterializedExtraction, ExtractionProcessError<R::Error>> {
    let accepted = repository
        .load_accepted_markdown(evidence_id, contract_version)
        .map_err(ExtractionProcessError::Repository)?;
    let canonical_text = std::str::from_utf8(accepted.canonical_bytes())
        .map_err(|_| ExtractionProcessError::Evidence(EvidenceError::InvalidCanonicalEncoding))?;
    let extraction = validate_accepted_markdown(
        accepted.evidence_id(),
        canonical_text,
        accepted.parsed(),
        accepted.accepted_at_millis(),
    )
    .map_err(ExtractionProcessError::Evidence)?;
    repository
        .commit_extraction(&extraction)
        .map_err(ExtractionProcessError::Repository)
}

/// Materializes one accepted revision, compares it with the adjacent revision
/// of the same source, and atomically persists the resulting lineage plan.
///
/// The first revision has no predecessor and therefore returns an in-memory
/// plan containing only `RebuildIndex` items. Later revisions persist the full
/// lineage batch before returning.
///
/// # Errors
///
/// Returns the shared repository error for encrypted storage failures, an
/// evidence error for invalid accepted Markdown, or a lineage error for an
/// invalid adjacent revision pair.
pub fn materialize_incremental_markdown<R>(
    repository: &mut R,
    evidence_id: u64,
    contract_version: &str,
    decided_at_millis: i64,
) -> Result<
    IncrementalMaterialization,
    IncrementalProcessError<<R as EvidenceExtractionRepository>::Error>,
>
where
    R: EvidenceExtractionRepository
        + BlockLineageRepository<Error = <R as EvidenceExtractionRepository>::Error>,
{
    let extraction = materialize_accepted_markdown(repository, evidence_id, contract_version)
        .map_err(|error| match error {
            ExtractionProcessError::Repository(error) => IncrementalProcessError::Repository(error),
            ExtractionProcessError::Evidence(error) => IncrementalProcessError::Evidence(error),
        })?;
    if let Some(batch) = BlockLineageRepository::load_lineage_batch(
        repository,
        extraction.revision().id(),
        BLOCK_LINEAGE_RULE_VERSION,
    )
    .map_err(IncrementalProcessError::Repository)?
    {
        let work_plan = batch.work_plan().clone();
        return Ok(IncrementalMaterialization {
            extraction,
            lineage: Some(batch),
            work_plan,
        });
    }
    let pair = BlockLineageRepository::load_lineage_pair(repository, extraction.revision().id())
        .map_err(IncrementalProcessError::Repository)?;
    let Some(pair) = pair else {
        let work_plan = IncrementalWorkPlan::new(
            extraction
                .blocks()
                .iter()
                .map(|block| IncrementalWorkItem::RebuildIndex {
                    to_ref: block.reference(),
                })
                .collect(),
        );
        return Ok(IncrementalMaterialization {
            extraction,
            lineage: None,
            work_plan,
        });
    };
    if pair.current().extraction().revision().id() != extraction.revision().id() {
        return Err(IncrementalProcessError::Lineage(
            LineageError::InvalidRevisionPair,
        ));
    }
    let batch = compute_block_lineage(
        pair.source_record_id(),
        pair.previous().extraction(),
        pair.previous().canonical_text(),
        pair.current().extraction(),
        pair.current().canonical_text(),
        decided_at_millis,
    )
    .map_err(IncrementalProcessError::Lineage)?;
    debug_assert_eq!(batch.rule_version(), BLOCK_LINEAGE_RULE_VERSION);
    let batch = BlockLineageRepository::commit_lineage_batch(repository, &batch)
        .map_err(IncrementalProcessError::Repository)?;
    let work_plan = batch.work_plan().clone();
    Ok(IncrementalMaterialization {
        extraction,
        lineage: Some(batch),
        work_plan,
    })
}

/// Opens one permanent block reference as an exact quote plus ephemeral UI range.
///
/// # Errors
///
/// Returns a repository error for encrypted storage failures, `BlockNotFound`
/// for an unknown immutable reference, or an evidence error for a corrupt
/// canonical encoding/range.
pub fn open_evidence_block<R: EvidenceBlockQueryRepository>(
    repository: &R,
    reference: EvidenceBlockRef,
) -> Result<EvidenceBlockView, EvidenceQueryError<R::Error>> {
    let source = repository
        .load_canonical_evidence_block(reference)
        .map_err(EvidenceQueryError::Repository)?
        .ok_or(EvidenceQueryError::Evidence(EvidenceError::BlockNotFound))?;
    if source.block().reference() != reference {
        return Err(EvidenceQueryError::Evidence(EvidenceError::BlockNotFound));
    }
    let canonical_text = std::str::from_utf8(source.canonical_bytes())
        .map_err(|_| EvidenceQueryError::Evidence(EvidenceError::InvalidCanonicalEncoding))?;
    EvidenceBlockView::new(source.block().clone(), canonical_text)
        .map_err(EvidenceQueryError::Evidence)
}

fn accept_markdown<R: MarkdownArchiveRepository>(
    repository: &mut R,
    archive_id: u64,
    parsed: &ParsedMarkdownV1,
    finished_at_millis: i64,
) -> Result<MarkdownProcessingOutcome, MarkdownProcessError<R::Error>> {
    repository
        .accept_markdown_parse(archive_id, CONTRACT_VERSION, parsed, finished_at_millis)
        .map_err(MarkdownProcessError::Repository)?;
    Ok(MarkdownProcessingOutcome::Accepted {
        archive_id,
        block_count: parsed.blocks.len(),
        relation_count: parsed.relations.len(),
    })
}

fn reject_markdown<R: MarkdownArchiveRepository>(
    repository: &mut R,
    archive_id: u64,
    reason: UnparsedReason,
    finished_at_millis: i64,
) -> Result<MarkdownProcessingOutcome, MarkdownProcessError<R::Error>> {
    repository
        .reject_markdown_parse(archive_id, CONTRACT_VERSION, reason, finished_at_millis)
        .map_err(MarkdownProcessError::Repository)?;
    Ok(MarkdownProcessingOutcome::Rejected { archive_id, reason })
}

fn observe_path(path: &Path) -> io::Result<FileObservation> {
    fs::symlink_metadata(path).map(|metadata| observe_metadata(&metadata))
}

fn observe_metadata(metadata: &Metadata) -> FileObservation {
    FileObservation {
        length: metadata.len(),
        modified_at: metadata.modified().ok(),
        kind: candidate_kind(metadata),
    }
}

fn candidate_kind(metadata: &Metadata) -> CandidateKind {
    if metadata.file_type().is_symlink() || is_reparse_point(metadata) {
        CandidateKind::ReparsePoint
    } else if metadata.is_file() {
        CandidateKind::RegularFile
    } else {
        CandidateKind::Other
    }
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &Metadata) -> bool {
    false
}

#[cfg(windows)]
fn open_without_following(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(windows))]
fn open_without_following(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

fn archive_status(path: &Path) -> ArchiveStatus {
    let markdown = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
    if markdown {
        ArchiveStatus::Archived
    } else {
        ArchiveStatus::ArchivedUnparsed(UnparsedReason::UnsupportedFormat)
    }
}

#[cfg(windows)]
fn is_device_path(path: &Path) -> bool {
    let rendered = path.as_os_str().to_string_lossy();
    let upper = rendered.to_ascii_uppercase();
    if upper.starts_with(r"\\.\") || upper.starts_with(r"\\?\GLOBALROOT\") {
        return true;
    }
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let base = file_name
        .trim_end_matches([' ', '.'])
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || base.strip_prefix("COM").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || base.strip_prefix("LPT").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
}

#[cfg(not(windows))]
const fn is_device_path(_: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use tempfile::tempdir;

    use super::*;
    use crate::{ArchiveReceipt, RejectReason};

    #[derive(Default)]
    struct RecordingRepository {
        archived: Vec<(String, Vec<u8>, ArchiveStatus)>,
    }

    impl ArchiveRepository for RecordingRepository {
        type Error = io::Error;

        fn archive(&mut self, input: ArchiveInput<'_>) -> Result<ArchiveReceipt, Self::Error> {
            self.archived.push((
                input.source_locator.to_owned(),
                input.content.to_vec(),
                input.status,
            ));
            Ok(ArchiveReceipt {
                archive_id: u64::try_from(self.archived.len()).unwrap(),
                status: input.status,
                object_reused: false,
                source_version_reused: false,
            })
        }
    }

    fn test_policy() -> ImportPolicy {
        ImportPolicy {
            stability_window: Duration::ZERO,
            auto_import_limit_bytes: 16,
            hard_import_limit_bytes: 32,
        }
    }

    #[test]
    fn markdown_archives_without_claiming_it_was_parsed() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("note.md");
        fs::write(&path, b"# evidence").unwrap();
        let mut repository = RecordingRepository::default();

        let outcome = ingest_inbox_file(&mut repository, &path, &test_policy(), false, 10).unwrap();

        assert!(matches!(
            outcome,
            ImportOutcome::Archived(ArchiveReceipt {
                status: ArchiveStatus::Archived,
                ..
            })
        ));
        assert_eq!(repository.archived[0].1, b"# evidence");
    }

    #[test]
    fn non_markdown_archives_as_unsupported_and_directory_is_rejected() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("photo.bin");
        fs::write(&path, b"opaque").unwrap();
        let mut repository = RecordingRepository::default();

        let outcome = ingest_inbox_file(&mut repository, &path, &test_policy(), false, 20).unwrap();
        assert!(matches!(
            outcome,
            ImportOutcome::Archived(ArchiveReceipt {
                status: ArchiveStatus::ArchivedUnparsed(UnparsedReason::UnsupportedFormat),
                ..
            })
        ));
        assert_eq!(
            ingest_inbox_file(&mut repository, directory.path(), &test_policy(), false, 30)
                .unwrap(),
            ImportOutcome::Rejected(RejectReason::UnsupportedFileType)
        );
    }

    #[test]
    fn oversized_file_waits_without_reading_or_archiving() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("large.md");
        fs::write(&path, vec![0_u8; 17]).unwrap();
        let mut repository = RecordingRepository::default();

        let outcome = ingest_inbox_file(&mut repository, &path, &test_policy(), false, 40).unwrap();

        assert_eq!(outcome, ImportOutcome::AwaitingApproval { bytes: 17 });
        assert!(repository.archived.is_empty());

        let approved = ingest_inbox_file(&mut repository, &path, &test_policy(), true, 41).unwrap();
        assert!(matches!(approved, ImportOutcome::Archived(_)));
        assert_eq!(repository.archived.len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn windows_device_names_are_rejected_before_file_access() {
        let mut repository = RecordingRepository::default();

        let outcome = ingest_inbox_file(
            &mut repository,
            Path::new(r"\\.\NUL"),
            &test_policy(),
            false,
            50,
        )
        .unwrap();

        assert_eq!(
            outcome,
            ImportOutcome::Rejected(RejectReason::UnsupportedFileType)
        );
        assert!(repository.archived.is_empty());
    }
}
