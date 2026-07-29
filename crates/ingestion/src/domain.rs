use std::time::{Duration, SystemTime};

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveStatus {
    Archived,
    ArchivedUnparsed(UnparsedReason),
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
