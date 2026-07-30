use std::{collections::HashMap, error::Error, fmt};

use eam_markdown::{
    CONTRACT_VERSION, MarkdownBlockKind, MarkdownLocator as ParsedMarkdownLocator, ParsedMarkdownV1,
};
use sha2::{Digest, Sha256};

pub const MARKDOWN_LOCATOR_VERSION: &str = "eam-markdown-locator-v1";
pub const NATIVE_NAVIGATION_UNAVAILABLE: &str = "NATIVE_NAVIGATION_UNAVAILABLE";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExtractionRevisionId(u64);

impl ExtractionRevisionId {
    /// Restores a positive identifier allocated by the trusted repository.
    ///
    /// # Errors
    ///
    /// Returns `InvalidIdentifier` for zero.
    pub fn new(value: u64) -> Result<Self, EvidenceError> {
        positive_identifier(value).map(Self)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EvidenceBlockId(u64);

impl EvidenceBlockId {
    /// Restores a positive identifier allocated by the trusted repository.
    ///
    /// # Errors
    ///
    /// Returns `InvalidIdentifier` for zero.
    pub fn new(value: u64) -> Result<Self, EvidenceError> {
        positive_identifier(value).map(Self)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractionRevision {
    id: ExtractionRevisionId,
    evidence_id: u64,
    contract_version: String,
    canonical_digest: [u8; 32],
    accepted_at_millis: i64,
}

impl ExtractionRevision {
    /// Restores one immutable accepted extraction revision.
    ///
    /// # Errors
    ///
    /// Returns an error when an identifier or contract version is invalid.
    pub fn new(
        id: ExtractionRevisionId,
        evidence_id: u64,
        contract_version: String,
        canonical_digest: [u8; 32],
        accepted_at_millis: i64,
    ) -> Result<Self, EvidenceError> {
        positive_identifier(evidence_id)?;
        if contract_version.trim().is_empty() {
            return Err(EvidenceError::ContractVersion);
        }
        Ok(Self {
            id,
            evidence_id,
            contract_version,
            canonical_digest,
            accepted_at_millis,
        })
    }

    #[must_use]
    pub const fn id(&self) -> ExtractionRevisionId {
        self.id
    }

    #[must_use]
    pub const fn evidence_id(&self) -> u64 {
        self.evidence_id
    }

    #[must_use]
    pub fn contract_version(&self) -> &str {
        &self.contract_version
    }

    #[must_use]
    pub const fn canonical_digest(&self) -> &[u8; 32] {
        &self.canonical_digest
    }

    #[must_use]
    pub const fn accepted_at_millis(&self) -> i64 {
        self.accepted_at_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownLocator {
    version: String,
    value: MarkdownLocatorValue,
}

impl MarkdownLocator {
    /// Restores a versioned, data-only native Markdown locator.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown version or malformed value.
    pub fn new(version: String, value: MarkdownLocatorValue) -> Result<Self, EvidenceError> {
        if version != MARKDOWN_LOCATOR_VERSION || !value.is_valid() {
            return Err(EvidenceError::NativeLocator);
        }
        Ok(Self { version, value })
    }

    fn from_parsed(value: &ParsedMarkdownLocator) -> Option<Self> {
        let value = match value {
            ParsedMarkdownLocator::Heading { text } => {
                MarkdownLocatorValue::Heading { text: text.clone() }
            }
            ParsedMarkdownLocator::BlockId { id } => {
                MarkdownLocatorValue::BlockId { id: id.clone() }
            }
        };
        Self::new(MARKDOWN_LOCATOR_VERSION.to_owned(), value).ok()
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub const fn value(&self) -> &MarkdownLocatorValue {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownLocatorValue {
    Heading { text: String },
    BlockId { id: String },
}

impl MarkdownLocatorValue {
    fn is_valid(&self) -> bool {
        match self {
            Self::Heading { text } => !text.trim().is_empty(),
            Self::BlockId { id } => {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAnchor {
    start_byte: usize,
    end_byte: usize,
    native_locator: Option<MarkdownLocator>,
}

impl SourceAnchor {
    /// Creates a half-open UTF-8 byte anchor into one immutable canonical text.
    ///
    /// # Errors
    ///
    /// Returns `InvalidSourceAnchor` when either endpoint is out of range or
    /// is not a UTF-8 character boundary.
    pub fn new(
        canonical_text: &str,
        start_byte: usize,
        end_byte: usize,
        native_locator: Option<MarkdownLocator>,
    ) -> Result<Self, EvidenceError> {
        if start_byte > end_byte
            || end_byte > canonical_text.len()
            || !canonical_text.is_char_boundary(start_byte)
            || !canonical_text.is_char_boundary(end_byte)
        {
            return Err(EvidenceError::InvalidSourceAnchor);
        }
        Ok(Self {
            start_byte,
            end_byte,
            native_locator,
        })
    }

    #[must_use]
    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    #[must_use]
    pub const fn end_byte(&self) -> usize {
        self.end_byte
    }

    #[must_use]
    pub const fn native_locator(&self) -> Option<&MarkdownLocator> {
        self.native_locator.as_ref()
    }

    /// Returns the canonical quote without normalization or rewriting.
    ///
    /// # Errors
    ///
    /// Returns `InvalidSourceAnchor` if this anchor is applied to a different
    /// or corrupt canonical text.
    pub fn quote<'a>(&self, canonical_text: &'a str) -> Result<&'a str, EvidenceError> {
        canonical_text
            .get(self.start_byte..self.end_byte)
            .ok_or(EvidenceError::InvalidSourceAnchor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceBlockMetadata {
    heading_level: Option<u8>,
    list_start: Option<u64>,
    task_checked: Option<bool>,
    info_string: Option<String>,
}

impl EvidenceBlockMetadata {
    #[must_use]
    pub const fn new(
        heading_level: Option<u8>,
        list_start: Option<u64>,
        task_checked: Option<bool>,
        info_string: Option<String>,
    ) -> Self {
        Self {
            heading_level,
            list_start,
            task_checked,
            info_string,
        }
    }

    #[must_use]
    pub const fn heading_level(&self) -> Option<u8> {
        self.heading_level
    }

    #[must_use]
    pub const fn list_start(&self) -> Option<u64> {
        self.list_start
    }

    #[must_use]
    pub const fn task_checked(&self) -> Option<bool> {
        self.task_checked
    }

    #[must_use]
    pub fn info_string(&self) -> Option<&str> {
        self.info_string.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceBlockDraft {
    local_id: u64,
    parent_local_id: Option<u64>,
    ordinal: usize,
    kind: MarkdownBlockKind,
    anchor: SourceAnchor,
    metadata: EvidenceBlockMetadata,
}

impl EvidenceBlockDraft {
    #[must_use]
    pub const fn local_id(&self) -> u64 {
        self.local_id
    }

    #[must_use]
    pub const fn parent_local_id(&self) -> Option<u64> {
        self.parent_local_id
    }

    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    #[must_use]
    pub const fn kind(&self) -> MarkdownBlockKind {
        self.kind
    }

    #[must_use]
    pub const fn anchor(&self) -> &SourceAnchor {
        &self.anchor
    }

    #[must_use]
    pub const fn metadata(&self) -> &EvidenceBlockMetadata {
        &self.metadata
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedExtraction {
    evidence_id: u64,
    contract_version: String,
    canonical_digest: [u8; 32],
    accepted_at_millis: i64,
    blocks: Vec<EvidenceBlockDraft>,
}

impl ValidatedExtraction {
    #[must_use]
    pub const fn evidence_id(&self) -> u64 {
        self.evidence_id
    }

    #[must_use]
    pub fn contract_version(&self) -> &str {
        &self.contract_version
    }

    #[must_use]
    pub const fn canonical_digest(&self) -> &[u8; 32] {
        &self.canonical_digest
    }

    #[must_use]
    pub const fn accepted_at_millis(&self) -> i64 {
        self.accepted_at_millis
    }

    #[must_use]
    pub fn blocks(&self) -> &[EvidenceBlockDraft] {
        &self.blocks
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceBlock {
    id: EvidenceBlockId,
    evidence_id: u64,
    extraction_revision_id: ExtractionRevisionId,
    parent_id: Option<EvidenceBlockId>,
    ordinal: usize,
    kind: MarkdownBlockKind,
    anchor: SourceAnchor,
    metadata: EvidenceBlockMetadata,
}

impl EvidenceBlock {
    /// Restores one Core-owned immutable evidence block.
    ///
    /// # Errors
    ///
    /// Returns an error when its identifiers or parent relation are invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: EvidenceBlockId,
        evidence_id: u64,
        extraction_revision_id: ExtractionRevisionId,
        parent_id: Option<EvidenceBlockId>,
        ordinal: usize,
        kind: MarkdownBlockKind,
        anchor: SourceAnchor,
        metadata: EvidenceBlockMetadata,
    ) -> Result<Self, EvidenceError> {
        positive_identifier(evidence_id)?;
        if parent_id == Some(id) {
            return Err(EvidenceError::InvalidBlockParent);
        }
        Ok(Self {
            id,
            evidence_id,
            extraction_revision_id,
            parent_id,
            ordinal,
            kind,
            anchor,
            metadata,
        })
    }

    #[must_use]
    pub const fn id(&self) -> EvidenceBlockId {
        self.id
    }

    #[must_use]
    pub const fn evidence_id(&self) -> u64 {
        self.evidence_id
    }

    #[must_use]
    pub const fn extraction_revision_id(&self) -> ExtractionRevisionId {
        self.extraction_revision_id
    }

    #[must_use]
    pub const fn parent_id(&self) -> Option<EvidenceBlockId> {
        self.parent_id
    }

    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    #[must_use]
    pub const fn kind(&self) -> MarkdownBlockKind {
        self.kind
    }

    #[must_use]
    pub const fn anchor(&self) -> &SourceAnchor {
        &self.anchor
    }

    #[must_use]
    pub const fn metadata(&self) -> &EvidenceBlockMetadata {
        &self.metadata
    }

    #[must_use]
    pub const fn reference(&self) -> EvidenceBlockRef {
        EvidenceBlockRef {
            evidence_id: self.evidence_id,
            block_id: self.id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EvidenceBlockRef {
    evidence_id: u64,
    block_id: EvidenceBlockId,
}

impl EvidenceBlockRef {
    /// Creates a permanent reference to one exact evidence and block version.
    ///
    /// # Errors
    ///
    /// Returns `InvalidIdentifier` for a zero evidence identifier.
    pub fn new(evidence_id: u64, block_id: EvidenceBlockId) -> Result<Self, EvidenceError> {
        positive_identifier(evidence_id)?;
        Ok(Self {
            evidence_id,
            block_id,
        })
    }

    #[must_use]
    pub const fn evidence_id(self) -> u64 {
        self.evidence_id
    }

    #[must_use]
    pub const fn block_id(self) -> EvidenceBlockId {
        self.block_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedMarkdownSource {
    evidence_id: u64,
    canonical_bytes: Vec<u8>,
    parsed: ParsedMarkdownV1,
    accepted_at_millis: i64,
}

impl AcceptedMarkdownSource {
    /// Restores the encrypted S09 input consumed by S10.
    ///
    /// # Errors
    ///
    /// Returns `InvalidIdentifier` for a zero evidence identifier.
    pub fn new(
        evidence_id: u64,
        canonical_bytes: Vec<u8>,
        parsed: ParsedMarkdownV1,
        accepted_at_millis: i64,
    ) -> Result<Self, EvidenceError> {
        positive_identifier(evidence_id)?;
        Ok(Self {
            evidence_id,
            canonical_bytes,
            parsed,
            accepted_at_millis,
        })
    }

    #[must_use]
    pub const fn evidence_id(&self) -> u64 {
        self.evidence_id
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    #[must_use]
    pub const fn parsed(&self) -> &ParsedMarkdownV1 {
        &self.parsed
    }

    #[must_use]
    pub const fn accepted_at_millis(&self) -> i64 {
        self.accepted_at_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializedExtraction {
    revision: ExtractionRevision,
    blocks: Vec<EvidenceBlock>,
    reused: bool,
}

impl MaterializedExtraction {
    /// Creates the complete result of one atomic extraction commit.
    ///
    /// # Errors
    ///
    /// Returns an error when any block belongs to another revision/evidence or
    /// the block order is not contiguous.
    pub fn new(
        revision: ExtractionRevision,
        blocks: Vec<EvidenceBlock>,
        reused: bool,
    ) -> Result<Self, EvidenceError> {
        for (ordinal, block) in blocks.iter().enumerate() {
            if block.evidence_id() != revision.evidence_id()
                || block.extraction_revision_id() != revision.id()
                || block.ordinal() != ordinal
            {
                return Err(EvidenceError::InvalidBlockOrder);
            }
        }
        Ok(Self {
            revision,
            blocks,
            reused,
        })
    }

    #[must_use]
    pub const fn revision(&self) -> &ExtractionRevision {
        &self.revision
    }

    #[must_use]
    pub fn blocks(&self) -> &[EvidenceBlock] {
        &self.blocks
    }

    #[must_use]
    pub const fn reused(&self) -> bool {
        self.reused
    }
}

pub trait EvidenceExtractionRepository {
    type Error;

    /// Loads one S09 artifact and its authenticated archived original.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the accepted artifact or original is
    /// missing, corrupt, or cannot be authenticated.
    fn load_accepted_markdown(
        &self,
        evidence_id: u64,
        contract_version: &str,
    ) -> Result<AcceptedMarkdownSource, Self::Error>;

    /// Assigns Core-owned identifiers and atomically persists a revision with
    /// all ordered blocks.
    ///
    /// # Errors
    ///
    /// Returns the adapter error without leaving a partial revision or block
    /// set.
    fn commit_extraction(
        &mut self,
        extraction: &ValidatedExtraction,
    ) -> Result<MaterializedExtraction, Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalEvidenceBlockSource {
    block: EvidenceBlock,
    canonical_bytes: Vec<u8>,
}

impl CanonicalEvidenceBlockSource {
    #[must_use]
    pub const fn new(block: EvidenceBlock, canonical_bytes: Vec<u8>) -> Self {
        Self {
            block,
            canonical_bytes,
        }
    }

    #[must_use]
    pub const fn block(&self) -> &EvidenceBlock {
        &self.block
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
}

pub trait EvidenceBlockQueryRepository {
    type Error;

    /// Loads one exact immutable block reference and its authenticated
    /// canonical Markdown source.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when encrypted state is missing, corrupt, or
    /// cannot be authenticated.
    fn load_canonical_evidence_block(
        &self,
        reference: EvidenceBlockRef,
    ) -> Result<Option<CanonicalEvidenceBlockSource>, Self::Error>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceBlockView {
    block: EvidenceBlock,
    verbatim: String,
    ui_range: UiTextRange,
}

impl EvidenceBlockView {
    /// Opens a block against the exact canonical Markdown used by its revision.
    ///
    /// # Errors
    ///
    /// Returns an evidence error when the stored anchor no longer slices the
    /// canonical text exactly.
    pub fn new(block: EvidenceBlock, canonical_text: &str) -> Result<Self, EvidenceError> {
        let verbatim = block.anchor().quote(canonical_text)?.to_owned();
        let ui_range = project_utf8_span_to_utf16(canonical_text, block.anchor())?;
        Ok(Self {
            block,
            verbatim,
            ui_range,
        })
    }

    #[must_use]
    pub const fn reference(&self) -> EvidenceBlockRef {
        self.block.reference()
    }

    #[must_use]
    pub const fn block(&self) -> &EvidenceBlock {
        &self.block
    }

    #[must_use]
    pub fn verbatim(&self) -> &str {
        &self.verbatim
    }

    #[must_use]
    pub const fn ui_range(&self) -> UiTextRange {
        self.ui_range
    }

    /// Resolves the optional native navigation independently of canonical quote access.
    ///
    /// # Errors
    ///
    /// Returns `NATIVE_NAVIGATION_UNAVAILABLE` when the source adapter reports
    /// a missing or stale locator.
    pub fn native_navigation(
        &self,
        locator_is_available: bool,
    ) -> Result<&MarkdownLocator, EvidenceError> {
        resolve_native_navigation(self.block.anchor(), locator_is_available)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiTextRange {
    start_utf16: usize,
    end_utf16: usize,
}

impl UiTextRange {
    #[must_use]
    pub const fn start_utf16(self) -> usize {
        self.start_utf16
    }

    #[must_use]
    pub const fn end_utf16(self) -> usize {
        self.end_utf16
    }
}

/// Converts the sole persisted UTF-8 anchor to an ephemeral `WebView` UTF-16 range.
///
/// # Errors
///
/// Returns `InvalidSourceAnchor` when the anchor does not apply to this text.
pub fn project_utf8_span_to_utf16(
    canonical_text: &str,
    anchor: &SourceAnchor,
) -> Result<UiTextRange, EvidenceError> {
    anchor.quote(canonical_text)?;
    let start_utf16 = canonical_text[..anchor.start_byte].encode_utf16().count();
    let end_utf16 = start_utf16
        + canonical_text[anchor.start_byte..anchor.end_byte]
            .encode_utf16()
            .count();
    Ok(UiTextRange {
        start_utf16,
        end_utf16,
    })
}

/// Returns a native locator only when the source adapter confirmed it is usable.
///
/// Canonical quote access remains independent through `SourceAnchor::quote`.
///
/// # Errors
///
/// Returns `NativeNavigationUnavailable` when the optional locator is absent or stale.
pub fn resolve_native_navigation(
    anchor: &SourceAnchor,
    locator_is_available: bool,
) -> Result<&MarkdownLocator, EvidenceError> {
    if !locator_is_available {
        return Err(EvidenceError::NativeNavigationUnavailable);
    }
    anchor
        .native_locator()
        .ok_or(EvidenceError::NativeNavigationUnavailable)
}

/// Validates an accepted S09 artifact against its immutable UTF-8 original.
///
/// # Errors
///
/// Rejects contract mismatches, invalid UTF-8 boundaries, non-deterministic
/// ordinals, duplicate local identifiers, and invalid parent containment.
pub fn validate_accepted_markdown(
    evidence_id: u64,
    canonical_text: &str,
    parsed: &ParsedMarkdownV1,
    accepted_at_millis: i64,
) -> Result<ValidatedExtraction, EvidenceError> {
    positive_identifier(evidence_id)?;
    if parsed.contract_version != CONTRACT_VERSION {
        return Err(EvidenceError::ContractVersion);
    }

    let mut by_local_id = HashMap::<u64, (usize, usize, usize)>::new();
    let mut blocks = Vec::with_capacity(parsed.blocks.len());
    for (expected_ordinal, block) in parsed.blocks.iter().enumerate() {
        if block.local_id == 0
            || block.ordinal != expected_ordinal
            || by_local_id
                .insert(
                    block.local_id,
                    (
                        block.ordinal,
                        block.source_span.start_byte,
                        block.source_span.end_byte,
                    ),
                )
                .is_some()
        {
            return Err(EvidenceError::InvalidBlockOrder);
        }
        if let Some(parent_local_id) = block.parent_local_id {
            let Some((parent_ordinal, parent_start, parent_end)) =
                by_local_id.get(&parent_local_id).copied()
            else {
                return Err(EvidenceError::InvalidBlockParent);
            };
            if parent_ordinal >= block.ordinal
                || parent_start > block.source_span.start_byte
                || parent_end < block.source_span.end_byte
            {
                return Err(EvidenceError::InvalidBlockParent);
            }
        }
        let native_locator = block
            .native_locator
            .as_ref()
            .and_then(MarkdownLocator::from_parsed);
        let anchor = SourceAnchor::new(
            canonical_text,
            block.source_span.start_byte,
            block.source_span.end_byte,
            native_locator,
        )?;
        blocks.push(EvidenceBlockDraft {
            local_id: block.local_id,
            parent_local_id: block.parent_local_id,
            ordinal: block.ordinal,
            kind: block.kind,
            anchor,
            metadata: EvidenceBlockMetadata {
                heading_level: block.heading_level,
                list_start: block.list_start,
                task_checked: block.task_checked,
                info_string: block.info_string.clone(),
            },
        });
    }

    Ok(ValidatedExtraction {
        evidence_id,
        contract_version: parsed.contract_version.clone(),
        canonical_digest: Sha256::digest(canonical_text.as_bytes()).into(),
        accepted_at_millis,
        blocks,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceError {
    InvalidIdentifier,
    InvalidCanonicalEncoding,
    ContractVersion,
    InvalidSourceAnchor,
    InvalidBlockOrder,
    InvalidBlockParent,
    NativeLocator,
    NativeNavigationUnavailable,
    BlockNotFound,
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentifier => "evidence identifier must be positive",
            Self::InvalidCanonicalEncoding => "canonical Markdown is not valid UTF-8",
            Self::ContractVersion => "Markdown extraction contract version is invalid",
            Self::InvalidSourceAnchor => "source anchor is not a valid UTF-8 byte range",
            Self::InvalidBlockOrder => "evidence block order or local identity is invalid",
            Self::InvalidBlockParent => "evidence block parent relation is invalid",
            Self::NativeLocator => "native Markdown locator is invalid",
            Self::NativeNavigationUnavailable => NATIVE_NAVIGATION_UNAVAILABLE,
            Self::BlockNotFound => "evidence block reference was not found",
        })
    }
}

impl Error for EvidenceError {}

fn positive_identifier(value: u64) -> Result<u64, EvidenceError> {
    if value == 0 {
        Err(EvidenceError::InvalidIdentifier)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use eam_markdown::{ParseLimits, SourceSpan, parse_markdown};

    use super::*;

    #[test]
    fn multilingual_quote_and_utf16_projection_are_exact() {
        let source = "前文 e\u{301} 😀 日本語 后文";
        let start = source.find('e').unwrap();
        let end = source.find(" 后文").unwrap();
        let anchor = SourceAnchor::new(source, start, end, None).unwrap();

        assert_eq!(anchor.quote(source).unwrap(), "e\u{301} 😀 日本語");
        let projected = project_utf8_span_to_utf16(source, &anchor).unwrap();
        assert_eq!(
            projected.start_utf16(),
            source[..start].encode_utf16().count()
        );
        assert_eq!(projected.end_utf16(), source[..end].encode_utf16().count());
    }

    #[test]
    fn byte_offsets_inside_cjk_combining_or_emoji_are_rejected() {
        let source = "中e\u{301}😀";
        assert_eq!(
            SourceAnchor::new(source, 1, source.len(), None),
            Err(EvidenceError::InvalidSourceAnchor)
        );
        let emoji = source.find('😀').unwrap();
        assert_eq!(
            SourceAnchor::new(source, 0, emoji + 1, None),
            Err(EvidenceError::InvalidSourceAnchor)
        );
        let combining = source.find('\u{301}').unwrap();
        assert_eq!(
            SourceAnchor::new(source, combining + 1, source.len(), None),
            Err(EvidenceError::InvalidSourceAnchor)
        );
    }

    #[test]
    fn accepted_markdown_becomes_validated_verbatim_block_drafts() {
        let source = "# 标题 😀\n\nCafe\u{301} 与日本語 ^stable-id\n";
        let parsed = parse_markdown(source, ParseLimits::default()).unwrap();

        let extraction = validate_accepted_markdown(7, source, &parsed, 90).unwrap();

        assert_eq!(extraction.evidence_id(), 7);
        assert_eq!(extraction.contract_version(), CONTRACT_VERSION);
        assert!(!extraction.blocks().is_empty());
        for block in extraction.blocks() {
            assert_eq!(
                block.anchor().quote(source).unwrap(),
                &source[block.anchor().start_byte()..block.anchor().end_byte()]
            );
        }
    }

    #[test]
    fn accepted_artifact_with_illegal_boundary_is_rejected() {
        let source = "标题 😀";
        let mut parsed = parse_markdown(source, ParseLimits::default()).unwrap();
        parsed.blocks[0].source_span = SourceSpan {
            start_byte: 1,
            end_byte: source.len(),
        };

        assert_eq!(
            validate_accepted_markdown(1, source, &parsed, 10),
            Err(EvidenceError::InvalidSourceAnchor)
        );
    }

    #[test]
    fn stale_native_locator_does_not_invalidate_canonical_quote() {
        let source = "## 规范引用";
        let locator = MarkdownLocator::new(
            MARKDOWN_LOCATOR_VERSION.to_owned(),
            MarkdownLocatorValue::Heading {
                text: "规范引用".to_owned(),
            },
        )
        .unwrap();
        let anchor = SourceAnchor::new(source, 0, source.len(), Some(locator)).unwrap();

        assert_eq!(
            resolve_native_navigation(&anchor, false),
            Err(EvidenceError::NativeNavigationUnavailable)
        );
        assert_eq!(
            EvidenceError::NativeNavigationUnavailable.to_string(),
            NATIVE_NAVIGATION_UNAVAILABLE
        );
        assert_eq!(anchor.quote(source).unwrap(), source);
    }
}
