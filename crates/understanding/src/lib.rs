//! Selective, sourced, and rebuildable deep-understanding projections.
//!
//! A projection is a disposable routing aid. Its immutable evidence block
//! references remain the only authority, and no API in this crate enumerates
//! or pre-processes the whole vault.

use std::{collections::HashSet, error::Error, fmt};

use eam_ingestion::EvidenceBlockRef;
use sha2::{Digest, Sha256};

pub const UNDERSTANDING_CONTRACT_VERSION: &str = "eam-understanding-v1";
pub const MIN_REPEATED_RECALLS: u32 = 2;
pub const MAX_PROJECTION_STATEMENTS: usize = 64;
pub const MAX_PROJECTION_SOURCES: usize = 64;
const MAX_SCOPE_TEXT_BYTES: usize = 16 * 1_024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionTrigger {
    PersonDesignated { reason: String },
    RepeatedRecall { query: String, recall_count: u32 },
    ImportantChange { description: String },
    CurrentTask { task: String },
}

impl ProjectionTrigger {
    fn validate(&self) -> Result<(), UnderstandingError> {
        let text = match self {
            Self::PersonDesignated { reason } => reason,
            Self::RepeatedRecall {
                query,
                recall_count,
            } => {
                if *recall_count < MIN_REPEATED_RECALLS {
                    return Err(UnderstandingError::TriggerNotEligible);
                }
                query
            }
            Self::ImportantChange { description } => description,
            Self::CurrentTask { task } => task,
        };
        validate_text(text)
    }

    #[must_use]
    pub const fn kind(&self) -> ProjectionTriggerKind {
        match self {
            Self::PersonDesignated { .. } => ProjectionTriggerKind::PersonDesignated,
            Self::RepeatedRecall { .. } => ProjectionTriggerKind::RepeatedRecall,
            Self::ImportantChange { .. } => ProjectionTriggerKind::ImportantChange,
            Self::CurrentTask { .. } => ProjectionTriggerKind::CurrentTask,
        }
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::PersonDesignated { reason } => reason,
            Self::RepeatedRecall { query, .. } => query,
            Self::ImportantChange { description } => description,
            Self::CurrentTask { task } => task,
        }
    }

    #[must_use]
    pub const fn recall_count(&self) -> Option<u32> {
        match self {
            Self::RepeatedRecall { recall_count, .. } => Some(*recall_count),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionTriggerKind {
    PersonDesignated,
    RepeatedRecall,
    ImportantChange,
    CurrentTask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionKind {
    EventChain,
    PersonTopicRelations,
    PhaseSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcedStatement {
    text: String,
    sources: Vec<EvidenceBlockRef>,
}

impl SourcedStatement {
    /// Creates one interpretation that remains explicitly linked to authority.
    ///
    /// # Errors
    ///
    /// Rejects empty or oversized text, empty sources, or duplicate references.
    pub fn new(
        text: impl Into<String>,
        sources: Vec<EvidenceBlockRef>,
    ) -> Result<Self, UnderstandingError> {
        let text = text.into();
        validate_text(&text)?;
        if sources.is_empty() || sources.len() > MAX_PROJECTION_SOURCES {
            return Err(UnderstandingError::InvalidSourceScope);
        }
        let unique = sources.iter().copied().collect::<HashSet<_>>();
        if unique.len() != sources.len() {
            return Err(UnderstandingError::InvalidSourceScope);
        }
        Ok(Self { text, sources })
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn sources(&self) -> &[EvidenceBlockRef] {
        &self.sources
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionContent {
    EventChain(Vec<SourcedStatement>),
    PersonTopicRelations(Vec<SourcedStatement>),
    PhaseSummary(SourcedStatement),
}

impl ProjectionContent {
    #[must_use]
    pub const fn kind(&self) -> ProjectionKind {
        match self {
            Self::EventChain(_) => ProjectionKind::EventChain,
            Self::PersonTopicRelations(_) => ProjectionKind::PersonTopicRelations,
            Self::PhaseSummary(_) => ProjectionKind::PhaseSummary,
        }
    }

    #[must_use]
    pub fn statements(&self) -> &[SourcedStatement] {
        match self {
            Self::EventChain(statements) | Self::PersonTopicRelations(statements) => statements,
            Self::PhaseSummary(statement) => std::slice::from_ref(statement),
        }
    }

    fn validate(&self) -> Result<(), UnderstandingError> {
        let statements = self.statements();
        if statements.is_empty() || statements.len() > MAX_PROJECTION_STATEMENTS {
            return Err(UnderstandingError::InvalidProjectionContent);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionRecipe {
    trigger: ProjectionTrigger,
    subject: String,
    content: ProjectionContent,
    requested_at_millis: i64,
}

impl ProjectionRecipe {
    /// Creates one finite, explicitly triggered projection recipe.
    ///
    /// # Errors
    ///
    /// Rejects an ineligible trigger, invalid subject/content, or more than 64
    /// distinct evidence blocks across the complete recipe.
    pub fn new(
        trigger: ProjectionTrigger,
        subject: impl Into<String>,
        content: ProjectionContent,
        requested_at_millis: i64,
    ) -> Result<Self, UnderstandingError> {
        trigger.validate()?;
        let subject = subject.into();
        validate_text(&subject)?;
        content.validate()?;
        let sources = content
            .statements()
            .iter()
            .flat_map(SourcedStatement::sources)
            .copied()
            .collect::<HashSet<_>>();
        if sources.is_empty() || sources.len() > MAX_PROJECTION_SOURCES {
            return Err(UnderstandingError::InvalidSourceScope);
        }
        Ok(Self {
            trigger,
            subject,
            content,
            requested_at_millis,
        })
    }

    #[must_use]
    pub const fn trigger(&self) -> &ProjectionTrigger {
        &self.trigger
    }

    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    #[must_use]
    pub const fn content(&self) -> &ProjectionContent {
        &self.content
    }

    #[must_use]
    pub const fn requested_at_millis(&self) -> i64 {
        self.requested_at_millis
    }

    #[must_use]
    pub fn sources(&self) -> Vec<EvidenceBlockRef> {
        let mut sources = self
            .content
            .statements()
            .iter()
            .flat_map(SourcedStatement::sources)
            .copied()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        sources.sort_by_key(|reference| (reference.evidence_id(), reference.block_id().get()));
        sources
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionSource {
    reference: EvidenceBlockRef,
    verbatim: String,
    source_record_id: u64,
    source_locator: String,
    recorded_at_millis: i64,
}

impl ProjectionSource {
    #[must_use]
    pub const fn new(
        reference: EvidenceBlockRef,
        verbatim: String,
        source_record_id: u64,
        source_locator: String,
        recorded_at_millis: i64,
    ) -> Self {
        Self {
            reference,
            verbatim,
            source_record_id,
            source_locator,
            recorded_at_millis,
        }
    }

    #[must_use]
    pub const fn reference(&self) -> EvidenceBlockRef {
        self.reference
    }

    #[must_use]
    pub fn verbatim(&self) -> &str {
        &self.verbatim
    }

    #[must_use]
    pub const fn source_record_id(&self) -> u64 {
        self.source_record_id
    }

    #[must_use]
    pub fn source_locator(&self) -> &str {
        &self.source_locator
    }

    #[must_use]
    pub const fn recorded_at_millis(&self) -> i64 {
        self.recorded_at_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionBuild {
    recipe: ProjectionRecipe,
    sources: Vec<ProjectionSource>,
    material_digest: [u8; 32],
}

impl ProjectionBuild {
    #[must_use]
    pub const fn recipe(&self) -> &ProjectionRecipe {
        &self.recipe
    }

    #[must_use]
    pub fn sources(&self) -> &[ProjectionSource] {
        &self.sources
    }

    #[must_use]
    pub const fn material_digest(&self) -> &[u8; 32] {
        &self.material_digest
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectionId(u64);

impl ProjectionId {
    /// Restores a positive persistent projection identifier.
    ///
    /// # Errors
    ///
    /// Rejects zero because stored identifiers are one-based.
    pub const fn new(value: u64) -> Result<Self, UnderstandingError> {
        if value == 0 {
            return Err(UnderstandingError::InvalidProjectionId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionStatus {
    Active,
    Invalidated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredProjection {
    id: ProjectionId,
    generation: u64,
    status: ProjectionStatus,
    material_digest: [u8; 32],
}

impl StoredProjection {
    #[must_use]
    pub const fn new(
        id: ProjectionId,
        generation: u64,
        status: ProjectionStatus,
        material_digest: [u8; 32],
    ) -> Self {
        Self {
            id,
            generation,
            status,
            material_digest,
        }
    }

    #[must_use]
    pub const fn id(&self) -> ProjectionId {
        self.id
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn status(&self) -> ProjectionStatus {
        self.status
    }

    #[must_use]
    pub const fn material_digest(&self) -> &[u8; 32] {
        &self.material_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredProjectionRecipe {
    projection: StoredProjection,
    recipe: ProjectionRecipe,
}

impl StoredProjectionRecipe {
    #[must_use]
    pub const fn new(projection: StoredProjection, recipe: ProjectionRecipe) -> Self {
        Self { projection, recipe }
    }

    #[must_use]
    pub const fn projection(&self) -> &StoredProjection {
        &self.projection
    }

    #[must_use]
    pub const fn recipe(&self) -> &ProjectionRecipe {
        &self.recipe
    }
}

pub trait UnderstandingRepository {
    type Error;

    /// Resolves one immutable block reference to authenticated authority.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when authority cannot be read or verified.
    fn resolve_projection_source(
        &self,
        reference: EvidenceBlockRef,
    ) -> Result<Option<ProjectionSource>, Self::Error>;

    /// Atomically persists one validated recipe and its disposable artifact.
    ///
    /// # Errors
    ///
    /// Returns the adapter error without leaving partial projection state.
    fn commit_projection(
        &mut self,
        build: &ProjectionBuild,
    ) -> Result<StoredProjection, Self::Error>;

    /// Loads the durable rebuild recipe and current projection state.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when persisted state cannot be decoded.
    fn load_projection_recipe(
        &self,
        id: ProjectionId,
    ) -> Result<Option<StoredProjectionRecipe>, Self::Error>;

    /// Replaces only the disposable artifact for an existing active recipe.
    ///
    /// # Errors
    ///
    /// Returns the adapter error without changing the durable recipe.
    fn replace_projection_artifact(
        &mut self,
        id: ProjectionId,
        build: &ProjectionBuild,
    ) -> Result<StoredProjection, Self::Error>;
}

/// Materializes only the evidence explicitly named by one eligible trigger.
///
/// # Errors
///
/// Fails closed on a missing authority reference or repository error, without
/// returning a partial projection.
pub fn materialize_projection<R: UnderstandingRepository>(
    repository: &mut R,
    recipe: ProjectionRecipe,
) -> Result<StoredProjection, ProjectionFailure<R::Error>> {
    let build = build_projection(repository, recipe)?;
    repository
        .commit_projection(&build)
        .map_err(ProjectionFailure::Repository)
}

/// Rebuilds a missing disposable artifact from its stored recipe and current
/// immutable authority while retaining the projection's contract generation.
///
/// # Errors
///
/// Rejects unknown or invalidated projections and missing source authority.
pub fn rebuild_projection<R: UnderstandingRepository>(
    repository: &mut R,
    id: ProjectionId,
) -> Result<StoredProjection, ProjectionFailure<R::Error>> {
    let stored = repository
        .load_projection_recipe(id)
        .map_err(ProjectionFailure::Repository)?
        .ok_or(ProjectionFailure::ProjectionNotFound(id))?;
    if stored.projection().status() == ProjectionStatus::Invalidated {
        return Err(ProjectionFailure::ProjectionInvalidated(id));
    }
    let build = build_projection(repository, stored.recipe().clone())?;
    repository
        .replace_projection_artifact(id, &build)
        .map_err(ProjectionFailure::Repository)
}

fn build_projection<R: UnderstandingRepository>(
    repository: &R,
    recipe: ProjectionRecipe,
) -> Result<ProjectionBuild, ProjectionFailure<R::Error>> {
    let mut sources = Vec::with_capacity(recipe.sources().len());
    for reference in recipe.sources() {
        let source = repository
            .resolve_projection_source(reference)
            .map_err(ProjectionFailure::Repository)?
            .ok_or(ProjectionFailure::SourceNotFound(reference))?;
        if source.reference() != reference {
            return Err(ProjectionFailure::InvalidAuthority(reference));
        }
        sources.push(source);
    }
    ProjectionBuild::from_resolved_sources(recipe, sources).map_err(ProjectionFailure::InvalidBuild)
}

impl ProjectionBuild {
    /// Reconstructs a deterministic artifact from an exact recipe/source set.
    ///
    /// # Errors
    ///
    /// Rejects missing, extra, duplicate, or mismatched source references.
    pub fn from_resolved_sources(
        recipe: ProjectionRecipe,
        mut sources: Vec<ProjectionSource>,
    ) -> Result<Self, UnderstandingError> {
        sources.sort_by_key(|source| {
            (
                source.reference().evidence_id(),
                source.reference().block_id().get(),
            )
        });
        let expected = recipe.sources();
        if sources.len() != expected.len()
            || sources
                .iter()
                .map(ProjectionSource::reference)
                .ne(expected.iter().copied())
        {
            return Err(UnderstandingError::InvalidSourceScope);
        }
        let material_digest = projection_digest(&recipe, &sources);
        Ok(Self {
            recipe,
            sources,
            material_digest,
        })
    }
}

fn projection_digest(recipe: &ProjectionRecipe, sources: &[ProjectionSource]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_bytes(&mut hasher, UNDERSTANDING_CONTRACT_VERSION.as_bytes());
    hasher.update([encode_trigger_kind(recipe.trigger().kind())]);
    hash_bytes(&mut hasher, recipe.trigger().detail().as_bytes());
    hash_u64(
        &mut hasher,
        u64::from(recipe.trigger().recall_count().unwrap_or_default()),
    );
    hasher.update([encode_projection_kind(recipe.content().kind())]);
    hash_bytes(&mut hasher, recipe.subject().as_bytes());
    hash_i64(&mut hasher, recipe.requested_at_millis());
    for statement in recipe.content().statements() {
        hash_bytes(&mut hasher, statement.text().as_bytes());
        for reference in statement.sources() {
            hash_reference(&mut hasher, *reference);
        }
    }
    for source in sources {
        hash_reference(&mut hasher, source.reference());
        hash_bytes(&mut hasher, source.verbatim().as_bytes());
        hash_u64(&mut hasher, source.source_record_id());
        hash_bytes(&mut hasher, source.source_locator().as_bytes());
        hash_i64(&mut hasher, source.recorded_at_millis());
    }
    hasher.finalize().into()
}

fn validate_text(value: &str) -> Result<(), UnderstandingError> {
    if value.trim().is_empty() || value.len() > MAX_SCOPE_TEXT_BYTES {
        return Err(UnderstandingError::InvalidText);
    }
    Ok(())
}

fn hash_reference(hasher: &mut Sha256, reference: EvidenceBlockRef) {
    hash_u64(hasher, reference.evidence_id());
    hash_u64(hasher, reference.block_id().get());
}

fn hash_bytes(hasher: &mut Sha256, value: &[u8]) {
    hash_u64(hasher, u64::try_from(value.len()).unwrap_or(u64::MAX));
    hasher.update(value);
}

fn hash_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_le_bytes());
}

fn hash_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_le_bytes());
}

const fn encode_trigger_kind(kind: ProjectionTriggerKind) -> u8 {
    match kind {
        ProjectionTriggerKind::PersonDesignated => 0,
        ProjectionTriggerKind::RepeatedRecall => 1,
        ProjectionTriggerKind::ImportantChange => 2,
        ProjectionTriggerKind::CurrentTask => 3,
    }
}

const fn encode_projection_kind(kind: ProjectionKind) -> u8 {
    match kind {
        ProjectionKind::EventChain => 0,
        ProjectionKind::PersonTopicRelations => 1,
        ProjectionKind::PhaseSummary => 2,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderstandingError {
    InvalidText,
    TriggerNotEligible,
    InvalidProjectionContent,
    InvalidSourceScope,
    InvalidProjectionId,
}

impl fmt::Display for UnderstandingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidText => "projection text must be non-empty and within the size limit",
            Self::TriggerNotEligible => "projection trigger is not eligible for deep understanding",
            Self::InvalidProjectionContent => "projection content is empty or exceeds its bound",
            Self::InvalidSourceScope => "projection source scope is empty, duplicate, or unbounded",
            Self::InvalidProjectionId => "projection identifier must be positive",
        })
    }
}

impl Error for UnderstandingError {}

#[derive(Debug)]
pub enum ProjectionFailure<E> {
    Repository(E),
    InvalidBuild(UnderstandingError),
    SourceNotFound(EvidenceBlockRef),
    InvalidAuthority(EvidenceBlockRef),
    ProjectionNotFound(ProjectionId),
    ProjectionInvalidated(ProjectionId),
}

impl<E: fmt::Display> fmt::Display for ProjectionFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => {
                write!(formatter, "understanding repository failed: {error}")
            }
            Self::InvalidBuild(error) => write!(formatter, "understanding build rejected: {error}"),
            Self::SourceNotFound(reference) => write!(
                formatter,
                "projection source not found: evidence {} block {}",
                reference.evidence_id(),
                reference.block_id().get()
            ),
            Self::InvalidAuthority(reference) => write!(
                formatter,
                "projection source resolved to the wrong authority: evidence {} block {}",
                reference.evidence_id(),
                reference.block_id().get()
            ),
            Self::ProjectionNotFound(id) => {
                write!(
                    formatter,
                    "understanding projection {} was not found",
                    id.get()
                )
            }
            Self::ProjectionInvalidated(id) => write!(
                formatter,
                "understanding projection {} is invalidated and cannot be rebuilt",
                id.get()
            ),
        }
    }
}

impl<E: Error + 'static> Error for ProjectionFailure<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::InvalidBuild(error) => Some(error),
            Self::SourceNotFound(_)
            | Self::InvalidAuthority(_)
            | Self::ProjectionNotFound(_)
            | Self::ProjectionInvalidated(_) => None,
        }
    }
}
