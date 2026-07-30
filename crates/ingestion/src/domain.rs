use std::time::{Duration, SystemTime};

use eam_markdown::{ParseResource, ParsedMarkdownV1};

pub const DEFAULT_AUTO_IMPORT_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
pub const DEFAULT_HARD_IMPORT_LIMIT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateKind {
    RegularFile,
    ReparsePoint,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileObservation {
    pub length: u64,
    pub modified_at: Option<SystemTime>,
    pub kind: CandidateKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    ReparsePoint,
    UnsupportedFileType,
    HardLimitExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnparsedReason {
    UnsupportedFormat,
    InvalidEncoding,
    ResourceLimit(ParseResource),
    InvalidStructure,
    ParserInterrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveStatus {
    Archived,
    ArchivedUnparsed(UnparsedReason),
    Extracted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntakeDecision {
    Discovered,
    Stable,
    AwaitingApproval { bytes: u64 },
    Rejected(RejectReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPolicy {
    pub stability_window: Duration,
    pub auto_import_limit_bytes: u64,
    pub hard_import_limit_bytes: u64,
}

impl Default for ImportPolicy {
    fn default() -> Self {
        Self {
            stability_window: Duration::from_millis(500),
            auto_import_limit_bytes: DEFAULT_AUTO_IMPORT_LIMIT_BYTES,
            hard_import_limit_bytes: DEFAULT_HARD_IMPORT_LIMIT_BYTES,
        }
    }
}

#[must_use]
pub fn evaluate_observations(
    first: &FileObservation,
    second: &FileObservation,
    policy: &ImportPolicy,
    oversized_approved: bool,
) -> IntakeDecision {
    if matches!(first.kind, CandidateKind::ReparsePoint)
        || matches!(second.kind, CandidateKind::ReparsePoint)
    {
        return IntakeDecision::Rejected(RejectReason::ReparsePoint);
    }
    if !matches!(first.kind, CandidateKind::RegularFile)
        || !matches!(second.kind, CandidateKind::RegularFile)
    {
        return IntakeDecision::Rejected(RejectReason::UnsupportedFileType);
    }
    if first != second {
        return IntakeDecision::Discovered;
    }
    if second.length > policy.hard_import_limit_bytes {
        return IntakeDecision::Rejected(RejectReason::HardLimitExceeded);
    }
    if second.length > policy.auto_import_limit_bytes && !oversized_approved {
        return IntakeDecision::AwaitingApproval {
            bytes: second.length,
        };
    }
    IntakeDecision::Stable
}

pub struct ArchiveInput<'a> {
    pub source_locator: &'a str,
    pub content: &'a [u8],
    pub status: ArchiveStatus,
    pub archived_at_millis: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveReceipt {
    pub archive_id: u64,
    pub status: ArchiveStatus,
    pub object_reused: bool,
    pub source_version_reused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivedEvidence {
    pub archive_id: u64,
    pub source_locator: String,
    pub content_length: u64,
    pub status: ArchiveStatus,
    pub archived_at_millis: i64,
}

pub trait ArchiveRepository {
    type Error;

    /// Persists one already-stable file version after its archive status is known.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the encrypted object or its database
    /// reference cannot be committed.
    fn archive(&mut self, input: ArchiveInput<'_>) -> Result<ArchiveReceipt, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownParseState {
    Started,
    Accepted,
    Rejected,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownParseAttempt {
    pub archive_id: u64,
    pub parser_version: String,
    pub state: MarkdownParseState,
    pub failure_reason: Option<UnparsedReason>,
    pub started_at_millis: i64,
    pub finished_at_millis: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownParseStart {
    Started,
    AlreadyAttempted(MarkdownParseState),
}

pub trait MarkdownArchiveRepository {
    type Error;

    /// Persists `STARTED` before any archived plaintext is read or parsed.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the archive is unavailable or the
    /// attempt cannot be committed atomically.
    fn begin_markdown_parse(
        &mut self,
        archive_id: u64,
        parser_version: &str,
        started_at_millis: i64,
    ) -> Result<MarkdownParseStart, Self::Error>;

    /// Reads one authenticated archived original inside the trusted Core.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the archive or encrypted object is
    /// unavailable or fails authentication.
    fn read_archived_content(&self, archive_id: u64) -> Result<Vec<u8>, Self::Error>;

    /// Atomically stores a complete parse artifact, marks the attempt
    /// `ACCEPTED`, and advances the archive to `EXTRACTED`.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the attempt is not `STARTED` or the
    /// transaction cannot be committed.
    fn accept_markdown_parse(
        &mut self,
        archive_id: u64,
        parser_version: &str,
        parsed: &ParsedMarkdownV1,
        finished_at_millis: i64,
    ) -> Result<(), Self::Error>;

    /// Atomically rejects the complete result and advances the archive to a
    /// stable `ARCHIVED_UNPARSED` reason without storing a partial artifact.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the attempt is not `STARTED` or the
    /// transaction cannot be committed.
    fn reject_markdown_parse(
        &mut self,
        archive_id: u64,
        parser_version: &str,
        reason: UnparsedReason,
        finished_at_millis: i64,
    ) -> Result<(), Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportOutcome {
    Discovered,
    AwaitingApproval { bytes: u64 },
    Rejected(RejectReason),
    Archived(ArchiveReceipt),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(length: u64, modified_at: SystemTime, kind: CandidateKind) -> FileObservation {
        FileObservation {
            length,
            modified_at: Some(modified_at),
            kind,
        }
    }

    #[test]
    fn stable_regular_file_advances_only_when_observations_match() {
        let policy = ImportPolicy::default();
        let first = observation(42, SystemTime::UNIX_EPOCH, CandidateKind::RegularFile);
        let changed = observation(
            43,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
            CandidateKind::RegularFile,
        );

        assert_eq!(
            evaluate_observations(&first, &first, &policy, false),
            IntakeDecision::Stable
        );
        assert_eq!(
            evaluate_observations(&first, &changed, &policy, false),
            IntakeDecision::Discovered
        );
    }

    #[test]
    fn oversized_file_waits_for_approval_but_hard_limit_rejects() {
        let policy = ImportPolicy {
            stability_window: Duration::ZERO,
            auto_import_limit_bytes: 10,
            hard_import_limit_bytes: 20,
        };
        let oversized = observation(11, SystemTime::UNIX_EPOCH, CandidateKind::RegularFile);
        let too_large = observation(21, SystemTime::UNIX_EPOCH, CandidateKind::RegularFile);

        assert_eq!(
            evaluate_observations(&oversized, &oversized, &policy, false),
            IntakeDecision::AwaitingApproval { bytes: 11 }
        );
        assert_eq!(
            evaluate_observations(&oversized, &oversized, &policy, true),
            IntakeDecision::Stable
        );
        assert_eq!(
            evaluate_observations(&too_large, &too_large, &policy, true),
            IntakeDecision::Rejected(RejectReason::HardLimitExceeded)
        );
    }

    #[test]
    fn reparse_points_and_non_files_are_rejected() {
        let policy = ImportPolicy::default();
        let regular = observation(1, SystemTime::UNIX_EPOCH, CandidateKind::RegularFile);
        let reparse = observation(1, SystemTime::UNIX_EPOCH, CandidateKind::ReparsePoint);
        let other = observation(1, SystemTime::UNIX_EPOCH, CandidateKind::Other);

        assert_eq!(
            evaluate_observations(&regular, &reparse, &policy, false),
            IntakeDecision::Rejected(RejectReason::ReparsePoint)
        );
        assert_eq!(
            evaluate_observations(&other, &other, &policy, false),
            IntakeDecision::Rejected(RejectReason::UnsupportedFileType)
        );
    }
}
