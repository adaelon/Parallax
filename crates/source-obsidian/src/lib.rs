//! Read-only discovery for an existing Obsidian vault.
//!
//! This adapter deliberately exposes no write API. It only enumerates ordinary
//! files below one selected root and reports enough state for trusted Core
//! reconciliation.

use std::{
    error::Error,
    fmt, fs, io,
    io::Read,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

const EXCLUDED_DIRECTORY_NAMES: [&str; 2] = [".obsidian", ".trash"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceAvailability {
    Available,
    SourceUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRootLifecycle {
    Staged,
    Active,
    Detached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRecordState {
    Present,
    SourceRemoved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceUnavailableReason {
    NotFound,
    PermissionDenied,
    NotDirectory,
    ReparsePoint,
    Io,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFileKind {
    Markdown,
    Attachment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectedEntryReason {
    ReparsePoint,
    UnsupportedFileType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceReadOutcome {
    Stable(Vec<u8>),
    Changed,
    Rejected(RejectedEntryReason),
    HardLimitExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRelationKind {
    Link,
    Image,
    Autolink,
    Wikilink,
    Embed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRelation {
    kind: SourceRelationKind,
    target: String,
    alias: Option<String>,
    heading: Option<String>,
    block_id: Option<String>,
    resolved_source_record_id: Option<u64>,
}

impl SourceRelation {
    #[must_use]
    pub const fn new(
        kind: SourceRelationKind,
        target: String,
        alias: Option<String>,
        heading: Option<String>,
        block_id: Option<String>,
        resolved_source_record_id: Option<u64>,
    ) -> Self {
        Self {
            kind,
            target,
            alias,
            heading,
            block_id,
            resolved_source_record_id,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> SourceRelationKind {
        self.kind
    }

    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    #[must_use]
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    #[must_use]
    pub fn heading(&self) -> Option<&str> {
        self.heading.as_deref()
    }

    #[must_use]
    pub fn block_id(&self) -> Option<&str> {
        self.block_id.as_deref()
    }

    #[must_use]
    pub const fn resolved_source_record_id(&self) -> Option<u64> {
        self.resolved_source_record_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocumentProjection {
    evidence_id: u64,
    properties: Vec<(String, String)>,
    tags: Vec<String>,
    aliases: Vec<String>,
    relations: Vec<SourceRelation>,
}

impl SourceDocumentProjection {
    #[must_use]
    pub const fn new(
        evidence_id: u64,
        properties: Vec<(String, String)>,
        tags: Vec<String>,
        aliases: Vec<String>,
        relations: Vec<SourceRelation>,
    ) -> Self {
        Self {
            evidence_id,
            properties,
            tags,
            aliases,
            relations,
        }
    }

    #[must_use]
    pub const fn evidence_id(&self) -> u64 {
        self.evidence_id
    }

    #[must_use]
    pub fn properties(&self) -> &[(String, String)] {
        &self.properties
    }

    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    #[must_use]
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    #[must_use]
    pub fn relations(&self) -> &[SourceRelation] {
        &self.relations
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    relative_path: String,
    kind: SourceFileKind,
    byte_len: u64,
    modified_at_millis: Option<i64>,
}

impl SourceFile {
    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    #[must_use]
    pub const fn kind(&self) -> SourceFileKind {
        self.kind
    }

    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }

    #[must_use]
    pub const fn modified_at_millis(&self) -> Option<i64> {
        self.modified_at_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedEntry {
    relative_path: String,
    reason: RejectedEntryReason,
}

impl RejectedEntry {
    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    #[must_use]
    pub const fn reason(&self) -> RejectedEntryReason {
        self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceScan {
    availability: SourceAvailability,
    unavailable_reason: Option<SourceUnavailableReason>,
    files: Vec<SourceFile>,
    rejected: Vec<RejectedEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRoot {
    id: u64,
    locator: String,
    lifecycle: SourceRootLifecycle,
    availability: SourceAvailability,
    first_seen_at_millis: i64,
    last_reconciled_at_millis: Option<i64>,
}

impl SourceRoot {
    /// Restores a validated source-root snapshot.
    ///
    /// # Errors
    ///
    /// Rejects a zero identifier or an empty locator.
    pub fn new(
        id: u64,
        locator: String,
        lifecycle: SourceRootLifecycle,
        availability: SourceAvailability,
        first_seen_at_millis: i64,
        last_reconciled_at_millis: Option<i64>,
    ) -> Result<Self, SourceStateError> {
        if id == 0
            || locator.trim().is_empty()
            || (lifecycle != SourceRootLifecycle::Staged && last_reconciled_at_millis.is_none())
        {
            return Err(SourceStateError::InvalidRoot);
        }
        Ok(Self {
            id,
            locator,
            lifecycle,
            availability,
            first_seen_at_millis,
            last_reconciled_at_millis,
        })
    }

    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub fn locator(&self) -> &str {
        &self.locator
    }

    #[must_use]
    pub const fn lifecycle(&self) -> SourceRootLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub const fn availability(&self) -> SourceAvailability {
        self.availability
    }

    #[must_use]
    pub const fn first_seen_at_millis(&self) -> i64 {
        self.first_seen_at_millis
    }

    #[must_use]
    pub const fn last_reconciled_at_millis(&self) -> Option<i64> {
        self.last_reconciled_at_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRecord {
    id: u64,
    root_id: u64,
    relative_path: String,
    state: SourceRecordState,
    first_seen_at_millis: i64,
    last_seen_at_millis: i64,
    current_evidence_id: Option<u64>,
}

impl SourceRecord {
    /// Restores a validated stable source-record snapshot.
    ///
    /// # Errors
    ///
    /// Rejects zero identifiers or a non-normalized relative path.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u64,
        root_id: u64,
        relative_path: String,
        state: SourceRecordState,
        first_seen_at_millis: i64,
        last_seen_at_millis: i64,
        current_evidence_id: Option<u64>,
    ) -> Result<Self, SourceStateError> {
        if id == 0 || root_id == 0 || !is_normalized_relative_locator(&relative_path) {
            return Err(SourceStateError::InvalidRecord);
        }
        Ok(Self {
            id,
            root_id,
            relative_path,
            state,
            first_seen_at_millis,
            last_seen_at_millis,
            current_evidence_id,
        })
    }

    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub const fn root_id(&self) -> u64 {
        self.root_id
    }

    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    #[must_use]
    pub const fn state(&self) -> SourceRecordState {
        self.state
    }

    #[must_use]
    pub const fn first_seen_at_millis(&self) -> i64 {
        self.first_seen_at_millis
    }

    #[must_use]
    pub const fn last_seen_at_millis(&self) -> i64 {
        self.last_seen_at_millis
    }

    #[must_use]
    pub const fn current_evidence_id(&self) -> Option<u64> {
        self.current_evidence_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRootSnapshot {
    root: SourceRoot,
    records: Vec<SourceRecord>,
}

impl SourceRootSnapshot {
    #[must_use]
    pub const fn new(root: SourceRoot, records: Vec<SourceRecord>) -> Self {
        Self { root, records }
    }

    #[must_use]
    pub const fn root(&self) -> &SourceRoot {
        &self.root
    }

    #[must_use]
    pub fn records(&self) -> &[SourceRecord] {
        &self.records
    }
}

pub struct SourceArchiveInput<'a> {
    pub root_id: u64,
    pub relative_path: &'a str,
    pub observed_relative_paths: &'a [String],
    pub claimed_source_record_ids: &'a [u64],
    pub content: &'a [u8],
    pub kind: SourceFileKind,
    pub observed_at_millis: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceArchiveReceipt {
    source_record_id: u64,
    archive_id: u64,
    previous_relative_path: Option<String>,
    content_reused: bool,
    source_version_reused: bool,
}

impl SourceArchiveReceipt {
    #[must_use]
    pub const fn new(
        source_record_id: u64,
        archive_id: u64,
        previous_relative_path: Option<String>,
        content_reused: bool,
        source_version_reused: bool,
    ) -> Self {
        Self {
            source_record_id,
            archive_id,
            previous_relative_path,
            content_reused,
            source_version_reused,
        }
    }

    #[must_use]
    pub const fn source_record_id(&self) -> u64 {
        self.source_record_id
    }

    #[must_use]
    pub const fn archive_id(&self) -> u64 {
        self.archive_id
    }

    #[must_use]
    pub fn previous_relative_path(&self) -> Option<&str> {
        self.previous_relative_path.as_deref()
    }

    #[must_use]
    pub const fn content_reused(&self) -> bool {
        self.content_reused
    }

    #[must_use]
    pub const fn source_version_reused(&self) -> bool {
        self.source_version_reused
    }
}

pub trait ObsidianSourceRepository {
    type Error;

    /// Registers or restores one selected root without reading its children.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when encrypted source state cannot be stored.
    fn register_source_root(
        &mut self,
        root_locator: &str,
        observed_at_millis: i64,
    ) -> Result<SourceRoot, Self::Error>;

    /// Loads the complete current state needed for one reconciliation.
    ///
    /// # Errors
    ///
    /// Returns the adapter error for missing or corrupt encrypted state.
    fn load_source_root(&self, root_id: u64) -> Result<SourceRootSnapshot, Self::Error>;

    /// Loads the unique active source root, if one has been activated.
    ///
    /// # Errors
    ///
    /// Returns the adapter error for corrupt encrypted lifecycle state.
    fn load_active_source_root(&self) -> Result<Option<SourceRootSnapshot>, Self::Error>;

    /// Atomically activates one successfully reconciled root and detaches the
    /// previous active root without deleting either root's evidence.
    ///
    /// # Errors
    ///
    /// Returns the adapter error if the candidate is missing, unavailable, has
    /// never reconciled successfully, or the complete transition cannot commit.
    fn activate_source_root(
        &mut self,
        root_id: u64,
        observed_at_millis: i64,
    ) -> Result<SourceRootSnapshot, Self::Error>;

    /// Marks a root unavailable without changing any child record.
    ///
    /// # Errors
    ///
    /// Returns the adapter error if the transition cannot be committed.
    fn mark_source_unavailable(
        &mut self,
        root_id: u64,
        observed_at_millis: i64,
    ) -> Result<SourceRoot, Self::Error>;

    /// Archives one observed ordinary file and restores or advances its stable
    /// source record. A move is recognized only when one unclaimed missing-path
    /// record has the same encrypted object identity.
    ///
    /// # Errors
    ///
    /// Returns the adapter error without exposing a partially committed source
    /// version.
    fn archive_source_file(
        &mut self,
        input: SourceArchiveInput<'_>,
    ) -> Result<SourceArchiveReceipt, Self::Error>;

    /// Completes one successful full-root reconciliation. Unseen children are
    /// marked `SOURCE_REMOVED`; observed children remain `PRESENT`.
    ///
    /// # Errors
    ///
    /// Returns the adapter error without partially applying child removals.
    fn finish_source_reconciliation(
        &mut self,
        root_id: u64,
        observed_source_record_ids: &[u64],
        observed_at_millis: i64,
    ) -> Result<SourceRootSnapshot, Self::Error>;

    /// Rebuilds resolved internal relation targets from persisted relation text,
    /// aliases, and the current root snapshot.
    ///
    /// # Errors
    ///
    /// Returns the adapter error if the encrypted projection cannot be rebuilt.
    fn refresh_source_relations(&mut self, root_id: u64) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceStateError {
    InvalidRoot,
    InvalidRecord,
}

impl fmt::Display for SourceStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoot => formatter.write_str("Obsidian source root is invalid"),
            Self::InvalidRecord => formatter.write_str("Obsidian source record is invalid"),
        }
    }
}

impl Error for SourceStateError {}

impl SourceScan {
    fn unavailable(reason: SourceUnavailableReason) -> Self {
        Self {
            availability: SourceAvailability::SourceUnavailable,
            unavailable_reason: Some(reason),
            files: Vec::new(),
            rejected: Vec::new(),
        }
    }

    #[must_use]
    pub const fn availability(&self) -> SourceAvailability {
        self.availability
    }

    #[must_use]
    pub const fn unavailable_reason(&self) -> Option<SourceUnavailableReason> {
        self.unavailable_reason
    }

    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    #[must_use]
    pub fn rejected(&self) -> &[RejectedEntry] {
        &self.rejected
    }
}

#[derive(Debug)]
pub struct ScanError {
    relative_directory: String,
    source: io::Error,
}

impl ScanError {
    #[must_use]
    pub fn relative_directory(&self) -> &str {
        &self.relative_directory
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Obsidian source traversal failed at {}: {}",
            self.relative_directory, self.source
        )
    }
}

impl Error for ScanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Recursively scans one selected Obsidian root without following links or
/// mutating any directory entry.
///
/// A missing, inaccessible, non-directory, or reparse-point root is reported
/// as `SOURCE_UNAVAILABLE`; it is not represented as an empty successful scan.
/// Once the root is available, any traversal error fails the entire scan so a
/// caller cannot convert a partial listing into source removals.
///
/// # Errors
///
/// Returns a traversal error only after the root was proven available but a
/// child directory could not be read completely.
pub fn scan_obsidian_root(root: &Path) -> Result<SourceScan, ScanError> {
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) => return Ok(SourceScan::unavailable(unavailable_reason(&error))),
    };
    if is_reparse_point(&root_metadata) {
        return Ok(SourceScan::unavailable(
            SourceUnavailableReason::ReparsePoint,
        ));
    }
    if !root_metadata.is_dir() {
        return Ok(SourceScan::unavailable(
            SourceUnavailableReason::NotDirectory,
        ));
    }
    if let Err(error) = fs::read_dir(root) {
        return Ok(SourceScan::unavailable(unavailable_reason(&error)));
    }

    let mut files = Vec::new();
    let mut rejected = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative_directory) = pending.pop() {
        let absolute_directory = root.join(&relative_directory);
        let entries = fs::read_dir(&absolute_directory).map_err(|source| ScanError {
            relative_directory: render_relative_path(&relative_directory),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ScanError {
                relative_directory: render_relative_path(&relative_directory),
                source,
            })?;
            let relative_path = relative_directory.join(entry.file_name());
            let rendered = render_relative_path(&relative_path);
            let metadata = fs::symlink_metadata(entry.path()).map_err(|source| ScanError {
                relative_directory: rendered.clone(),
                source,
            })?;
            if is_reparse_point(&metadata) {
                rejected.push(RejectedEntry {
                    relative_path: rendered,
                    reason: RejectedEntryReason::ReparsePoint,
                });
            } else if metadata.is_dir() {
                if !is_excluded_directory(&entry.file_name()) {
                    pending.push(relative_path);
                }
            } else if metadata.is_file() {
                files.push(SourceFile {
                    relative_path: rendered,
                    kind: file_kind(&entry.path()),
                    byte_len: metadata.len(),
                    modified_at_millis: modified_at_millis(&metadata),
                });
            } else {
                rejected.push(RejectedEntry {
                    relative_path: rendered,
                    reason: RejectedEntryReason::UnsupportedFileType,
                });
            }
        }
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    rejected.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(SourceScan {
        availability: SourceAvailability::Available,
        unavailable_reason: None,
        files,
        rejected,
    })
}

/// Reads one file from a prior scan without following a replacement link and
/// verifies that its length and modification time stayed stable.
///
/// # Errors
///
/// Returns an I/O error for failures other than a disappeared/changed path.
pub fn read_scanned_source_file(
    root: &Path,
    source_file: &SourceFile,
    hard_limit_bytes: u64,
) -> io::Result<SourceReadOutcome> {
    let path = root.join(
        source_file
            .relative_path
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    );
    let before = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(SourceReadOutcome::Changed);
        }
        Err(error) => return Err(error),
    };
    if is_reparse_point(&before) {
        return Ok(SourceReadOutcome::Rejected(
            RejectedEntryReason::ReparsePoint,
        ));
    }
    if !before.is_file() {
        return Ok(SourceReadOutcome::Rejected(
            RejectedEntryReason::UnsupportedFileType,
        ));
    }
    if !matches_source_file(source_file, &before) {
        return Ok(SourceReadOutcome::Changed);
    }
    if before.len() > hard_limit_bytes {
        return Ok(SourceReadOutcome::HardLimitExceeded);
    }
    let mut file = open_without_following(&path)?;
    let opened = file.metadata()?;
    if is_reparse_point(&opened) || !matches_source_file(source_file, &opened) {
        return Ok(SourceReadOutcome::Changed);
    }
    let mut content = Vec::new();
    file.by_ref()
        .take(hard_limit_bytes.saturating_add(1))
        .read_to_end(&mut content)?;
    if u64::try_from(content.len()).unwrap_or(u64::MAX) > hard_limit_bytes {
        return Ok(SourceReadOutcome::HardLimitExceeded);
    }
    let after = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(SourceReadOutcome::Changed);
        }
        Err(error) => return Err(error),
    };
    if !matches_source_file(source_file, &after) || is_reparse_point(&after) {
        return Ok(SourceReadOutcome::Changed);
    }
    Ok(SourceReadOutcome::Stable(content))
}

fn unavailable_reason(error: &io::Error) -> SourceUnavailableReason {
    match error.kind() {
        io::ErrorKind::NotFound => SourceUnavailableReason::NotFound,
        io::ErrorKind::PermissionDenied => SourceUnavailableReason::PermissionDenied,
        _ => SourceUnavailableReason::Io,
    }
}

fn is_excluded_directory(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| EXCLUDED_DIRECTORY_NAMES.contains(&name))
}

fn file_kind(path: &Path) -> SourceFileKind {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        SourceFileKind::Markdown
    } else {
        SourceFileKind::Attachment
    }
}

fn modified_at_millis(metadata: &fs::Metadata) -> Option<i64> {
    let duration = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_millis()).ok()
}

fn matches_source_file(source_file: &SourceFile, metadata: &fs::Metadata) -> bool {
    metadata.is_file()
        && metadata.len() == source_file.byte_len
        && modified_at_millis(metadata) == source_file.modified_at_millis
}

fn render_relative_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn is_normalized_relative_locator(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    is_windows_reparse_point(metadata)
}

#[cfg(windows)]
fn open_without_following(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(windows))]
fn open_without_following(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new().read(true).open(path)
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
const fn is_windows_reparse_point(_: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn active_and_detached_roots_require_a_successful_reconciliation() {
        assert!(
            SourceRoot::new(
                1,
                "C:/notes/staged".to_owned(),
                SourceRootLifecycle::Staged,
                SourceAvailability::Available,
                10,
                None,
            )
            .is_ok()
        );
        for lifecycle in [SourceRootLifecycle::Active, SourceRootLifecycle::Detached] {
            assert!(
                SourceRoot::new(
                    1,
                    "C:/notes/reconciled".to_owned(),
                    lifecycle,
                    SourceAvailability::Available,
                    10,
                    None,
                )
                .is_err()
            );
            assert!(
                SourceRoot::new(
                    1,
                    "C:/notes/reconciled".to_owned(),
                    lifecycle,
                    SourceAvailability::Available,
                    10,
                    Some(20),
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn scans_only_ordinary_source_files_without_modifying_the_root() {
        let directory = tempdir().unwrap();
        fs::create_dir_all(directory.path().join("notes/sub")).unwrap();
        fs::create_dir_all(directory.path().join(".obsidian/plugins/demo")).unwrap();
        fs::create_dir_all(directory.path().join(".trash")).unwrap();
        fs::write(directory.path().join("root.md"), b"# Root").unwrap();
        fs::write(directory.path().join("notes/sub/nested.MD"), b"[[Root]]").unwrap();
        fs::write(directory.path().join("notes/image.png"), b"png").unwrap();
        fs::write(directory.path().join(".obsidian/config"), b"secret").unwrap();
        fs::write(directory.path().join(".trash/deleted.md"), b"gone").unwrap();
        let before = directory_digest(directory.path());

        let scan = scan_obsidian_root(directory.path()).unwrap();

        assert_eq!(scan.availability(), SourceAvailability::Available);
        assert_eq!(scan.unavailable_reason(), None);
        assert_eq!(
            scan.files()
                .iter()
                .map(|file| (file.relative_path(), file.kind()))
                .collect::<Vec<_>>(),
            vec![
                ("notes/image.png", SourceFileKind::Attachment),
                ("notes/sub/nested.MD", SourceFileKind::Markdown),
                ("root.md", SourceFileKind::Markdown),
            ]
        );
        assert_eq!(directory_digest(directory.path()), before);
    }

    #[test]
    fn missing_root_is_unavailable_instead_of_an_empty_scan() {
        let directory = tempdir().unwrap();
        let missing = directory.path().join("offline-vault");

        let scan = scan_obsidian_root(&missing).unwrap();

        assert_eq!(scan.availability(), SourceAvailability::SourceUnavailable);
        assert_eq!(
            scan.unavailable_reason(),
            Some(SourceUnavailableReason::NotFound)
        );
        assert!(scan.files().is_empty());
    }

    #[test]
    fn file_root_is_unavailable() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("note.md");
        fs::write(&root, b"not a vault").unwrap();

        let scan = scan_obsidian_root(&root).unwrap();

        assert_eq!(
            scan.unavailable_reason(),
            Some(SourceUnavailableReason::NotDirectory)
        );
    }

    #[test]
    fn scanned_read_is_bounded_and_detects_change() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("note.md");
        fs::write(&path, b"one").unwrap();
        let scan = scan_obsidian_root(directory.path()).unwrap();
        let source_file = &scan.files()[0];

        assert_eq!(
            read_scanned_source_file(directory.path(), source_file, 3).unwrap(),
            SourceReadOutcome::Stable(b"one".to_vec())
        );
        assert_eq!(
            read_scanned_source_file(directory.path(), source_file, 2).unwrap(),
            SourceReadOutcome::HardLimitExceeded
        );
        fs::write(&path, b"changed").unwrap();
        assert_eq!(
            read_scanned_source_file(directory.path(), source_file, 32).unwrap(),
            SourceReadOutcome::Changed
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_reported_and_never_followed() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("outside.md"), b"outside").unwrap();
        symlink(outside.path(), directory.path().join("linked")).unwrap();

        let scan = scan_obsidian_root(directory.path()).unwrap();

        assert!(scan.files().is_empty());
        assert_eq!(
            scan.rejected(),
            &[RejectedEntry {
                relative_path: "linked".to_owned(),
                reason: RejectedEntryReason::ReparsePoint,
            }]
        );
    }

    fn directory_digest(root: &Path) -> [u8; 32] {
        fn collect(root: &Path, current: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(current).unwrap() {
                let entry = entry.unwrap();
                let metadata = fs::symlink_metadata(entry.path()).unwrap();
                if metadata.is_dir() {
                    collect(root, &entry.path(), files);
                } else if metadata.is_file() {
                    let relative = entry.path().strip_prefix(root).unwrap().to_path_buf();
                    files.insert(
                        render_relative_path(&relative),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        collect(root, root, &mut files);
        let mut hasher = Sha256::new();
        for (path, bytes) in files {
            hasher.update(path.as_bytes());
            hasher.update([0]);
            hasher.update(bytes);
            hasher.update([0]);
        }
        hasher.finalize().into()
    }
}
