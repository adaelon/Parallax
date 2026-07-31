//! Authoritative lexical, temporal, and relation retrieval contracts.
//!
//! Persistent indexes are disposable candidate finders. Every returned item is
//! resolved by the trusted repository to an immutable evidence block or a
//! sourced ledger claim before it leaves this crate.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use eam_core::{
    ActiveRelationalConstraint, Claim, ClaimId, ClaimStatus, DecisionImpact, DisputeState,
    EvidenceCitation, SharedAgreementCandidate, SharedAgreementCandidateStatus, SharedExperience,
    SharedExperienceKind, Timestamp,
};
use eam_ingestion::{EvidenceBlockRef, EvidenceBlockView};

mod context;
mod vector;

pub use context::{
    DEFAULT_TOKEN_BUDGET, FreezeFailure, MAX_TOKEN_BUDGET, MIN_TOKEN_BUDGET, TokenBudget,
    freeze_working_context,
};
pub use vector::{
    EMBEDDING_MODEL_VERSION, EmbeddingError, VECTOR_BYTES, VECTOR_DIMENSIONS, VECTOR_MIN_SCORE_BPS,
    VectorEmbedding, cosine_similarity_bps, embed_text,
};

pub const RETRIEVAL_INDEX_VERSION: &str = "eam-retrieval-v2";
pub const DEFAULT_RESULT_LIMIT: usize = 20;
pub const MAX_RESULT_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceScope {
    #[default]
    Current,
    Historical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    start_millis: i64,
    end_millis: i64,
}

impl TimeRange {
    /// Creates one inclusive time interval.
    ///
    /// # Errors
    ///
    /// Returns `InvalidTimeRange` when the end precedes the start.
    pub const fn new(start_millis: i64, end_millis: i64) -> Result<Self, RetrievalError> {
        if start_millis > end_millis {
            return Err(RetrievalError::InvalidTimeRange);
        }
        Ok(Self {
            start_millis,
            end_millis,
        })
    }

    #[must_use]
    pub const fn at(at_millis: i64) -> Self {
        Self {
            start_millis: at_millis,
            end_millis: at_millis,
        }
    }

    #[must_use]
    pub const fn start_millis(self) -> i64 {
        self.start_millis
    }

    #[must_use]
    pub const fn end_millis(self) -> i64 {
        self.end_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievalQuery {
    text: Option<String>,
    time: Option<TimeRange>,
    entities: Vec<String>,
    source_scope: SourceScope,
    decision_impact: DecisionImpact,
    limit: usize,
}

impl RetrievalQuery {
    #[must_use]
    pub fn lexical(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            time: None,
            entities: Vec::new(),
            source_scope: SourceScope::Current,
            decision_impact: DecisionImpact::Ordinary,
            limit: DEFAULT_RESULT_LIMIT,
        }
    }

    #[must_use]
    pub const fn temporal(time: TimeRange) -> Self {
        Self {
            text: None,
            time: Some(time),
            entities: Vec::new(),
            source_scope: SourceScope::Current,
            decision_impact: DecisionImpact::Ordinary,
            limit: DEFAULT_RESULT_LIMIT,
        }
    }

    #[must_use]
    pub fn related_to(entity: impl Into<String>) -> Self {
        Self {
            text: None,
            time: None,
            entities: vec![entity.into()],
            source_scope: SourceScope::Current,
            decision_impact: DecisionImpact::Ordinary,
            limit: DEFAULT_RESULT_LIMIT,
        }
    }

    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    #[must_use]
    pub const fn with_time(mut self, time: TimeRange) -> Self {
        self.time = Some(time);
        self
    }

    #[must_use]
    pub fn with_entity(mut self, entity: impl Into<String>) -> Self {
        self.entities.push(entity.into());
        self
    }

    #[must_use]
    pub const fn with_source_scope(mut self, source_scope: SourceScope) -> Self {
        self.source_scope = source_scope;
        self
    }

    #[must_use]
    pub const fn with_decision_impact(mut self, impact: DecisionImpact) -> Self {
        self.decision_impact = impact;
        self
    }

    #[must_use]
    pub const fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    #[must_use]
    pub const fn time(&self) -> Option<TimeRange> {
        self.time
    }

    #[must_use]
    pub fn entities(&self) -> &[String] {
        &self.entities
    }

    #[must_use]
    pub const fn source_scope(&self) -> SourceScope {
        self.source_scope
    }

    #[must_use]
    pub const fn decision_impact(&self) -> DecisionImpact {
        self.decision_impact
    }

    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    fn validate(&self) -> Result<(), RetrievalError> {
        let has_text = self
            .text
            .as_ref()
            .is_some_and(|text| !search_terms(text).is_empty());
        let has_entity = self
            .entities
            .iter()
            .any(|entity| !search_terms(entity).is_empty());
        if !has_text && self.time.is_none() && !has_entity {
            return Err(RetrievalError::EmptyQuery);
        }
        if self.limit == 0 || self.limit > MAX_RESULT_LIMIT {
            return Err(RetrievalError::InvalidLimit);
        }
        Ok(())
    }
}

/// Projects confirmed agreements whose immutable scope overlaps the current
/// task text into active runtime constraints.
///
/// Scope matching is deliberately lexical and conservative: CJK single
/// characters and one-character ASCII tokens are ignored, while a composite
/// scope needs two distinct term matches before a task inherits its obligation.
#[must_use]
pub fn project_active_relational_constraints(
    query: &RetrievalQuery,
    candidates: &[SharedAgreementCandidate],
    experiences: &[SharedExperience],
    frozen_at: Timestamp,
) -> Vec<ActiveRelationalConstraint> {
    let task_terms = query
        .text()
        .into_iter()
        .chain(query.entities().iter().map(String::as_str))
        .flat_map(search_terms)
        .filter(|term| term.chars().count() >= 2)
        .collect::<BTreeSet<_>>();
    if task_terms.is_empty() {
        return Vec::new();
    }

    let agreement_claim_exists = |claim_id: ClaimId| {
        experiences.iter().any(|experience| {
            experience.kind() == SharedExperienceKind::Agreement
                && experience.claim().id() == claim_id
                && experience.claim().status() == ClaimStatus::Current
        })
    };
    let superseded_claim_ids = candidates
        .iter()
        .filter(|candidate| candidate.status() == SharedAgreementCandidateStatus::Confirmed)
        .filter(|candidate| candidate.claim_id().is_some_and(&agreement_claim_exists))
        .filter(|candidate| {
            candidate
                .effective_from()
                .is_some_and(|from| from.as_millis() <= frozen_at.as_millis())
        })
        .flat_map(|candidate| candidate.supersedes_agreement_ids().iter().copied())
        .collect::<BTreeSet<_>>();

    let mut projected = Vec::new();
    let mut seen = BTreeSet::new();
    for candidate in candidates {
        if candidate.status() != SharedAgreementCandidateStatus::Confirmed {
            continue;
        }
        let Some(claim_id) = candidate.claim_id() else {
            continue;
        };
        if superseded_claim_ids.contains(&claim_id) {
            continue;
        }
        if !seen.insert(claim_id) {
            continue;
        }
        let Some(scope) = candidate.scope() else {
            continue;
        };
        let scope_terms = scope_relevance_terms(scope);
        let required_matches = scope_terms.len().min(2);
        let scope_is_relevant = required_matches > 0
            && scope_terms
                .iter()
                .filter(|term| task_terms.contains(*term))
                .take(required_matches)
                .count()
                == required_matches;
        if !scope_is_relevant {
            continue;
        }
        let Some(effective_from) = candidate.effective_from() else {
            continue;
        };
        if frozen_at.as_millis() < effective_from.as_millis()
            || candidate
                .effective_until()
                .is_some_and(|until| frozen_at.as_millis() > until.as_millis())
        {
            continue;
        }
        let agreement_exists = experiences.iter().any(|experience| {
            experience.kind() == SharedExperienceKind::Agreement
                && experience.claim().id() == claim_id
                && experience.claim().status() == ClaimStatus::Current
        });
        if !agreement_exists {
            continue;
        }
        if let Ok(constraint) = ActiveRelationalConstraint::new(
            claim_id,
            candidate.statement(),
            scope,
            effective_from,
            candidate.effective_until(),
        ) {
            projected.push(constraint);
        }
    }
    projected
}

fn scope_relevance_terms(scope: &str) -> Vec<String> {
    search_terms(&scope.replace("双方", " ").replace("共同", " "))
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexDisposition {
    Current,
    Rebuilt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexBuildReceipt {
    disposition: IndexDisposition,
    evidence_blocks: usize,
    ledger_claims: usize,
    relations: usize,
}

impl IndexBuildReceipt {
    #[must_use]
    pub const fn new(
        disposition: IndexDisposition,
        evidence_blocks: usize,
        ledger_claims: usize,
        relations: usize,
    ) -> Self {
        Self {
            disposition,
            evidence_blocks,
            ledger_claims,
            relations,
        }
    }

    #[must_use]
    pub const fn disposition(self) -> IndexDisposition {
        self.disposition
    }

    #[must_use]
    pub const fn evidence_blocks(self) -> usize {
        self.evidence_blocks
    }

    #[must_use]
    pub const fn ledger_claims(self) -> usize {
        self.ledger_claims
    }

    #[must_use]
    pub const fn relations(self) -> usize {
        self.relations
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateRef {
    Evidence { evidence_id: u64, block_id: u64 },
    Ledger { claim_id: u64 },
}

impl CandidateRef {
    #[must_use]
    pub fn evidence(reference: EvidenceBlockRef) -> Self {
        Self::Evidence {
            evidence_id: reference.evidence_id(),
            block_id: reference.block_id().get(),
        }
    }

    #[must_use]
    pub const fn ledger(claim_id: ClaimId) -> Self {
        Self::Ledger {
            claim_id: claim_id.get(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecallChannels {
    bits: u8,
}

impl RecallChannels {
    const LEXICAL: u8 = 1 << 0;
    const VECTOR: u8 = 1 << 1;
    const TEMPORAL: u8 = 1 << 2;
    const RELATION: u8 = 1 << 3;
    const LONG_TERM_MEMORY: u8 = 1 << 4;
    const UNDERSTANDING: u8 = 1 << 5;

    #[must_use]
    pub const fn lexical() -> Self {
        Self {
            bits: Self::LEXICAL,
        }
    }

    #[must_use]
    pub const fn vector() -> Self {
        Self { bits: Self::VECTOR }
    }

    #[must_use]
    pub const fn temporal() -> Self {
        Self {
            bits: Self::TEMPORAL,
        }
    }

    #[must_use]
    pub const fn relation() -> Self {
        Self {
            bits: Self::RELATION,
        }
    }

    #[must_use]
    pub const fn long_term_memory() -> Self {
        Self {
            bits: Self::LONG_TERM_MEMORY,
        }
    }

    #[must_use]
    pub const fn understanding() -> Self {
        Self {
            bits: Self::UNDERSTANDING,
        }
    }

    #[must_use]
    pub const fn contains_lexical(self) -> bool {
        self.bits & Self::LEXICAL != 0
    }

    #[must_use]
    pub const fn contains_vector(self) -> bool {
        self.bits & Self::VECTOR != 0
    }

    #[must_use]
    pub const fn contains_temporal(self) -> bool {
        self.bits & Self::TEMPORAL != 0
    }

    #[must_use]
    pub const fn contains_relation(self) -> bool {
        self.bits & Self::RELATION != 0
    }

    #[must_use]
    pub const fn contains_long_term_memory(self) -> bool {
        self.bits & Self::LONG_TERM_MEMORY != 0
    }

    #[must_use]
    pub const fn contains_understanding(self) -> bool {
        self.bits & Self::UNDERSTANDING != 0
    }

    const fn merged(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    fn count(self) -> u8 {
        u8::try_from(self.bits.count_ones()).unwrap_or(u8::MAX)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallHit {
    reference: CandidateRef,
    channels: RecallChannels,
    lexical_score: u32,
    vector_score_bps: u16,
}

impl RecallHit {
    #[must_use]
    pub const fn new(
        reference: CandidateRef,
        channels: RecallChannels,
        lexical_score: u32,
    ) -> Self {
        Self {
            reference,
            channels,
            lexical_score,
            vector_score_bps: 0,
        }
    }

    #[must_use]
    pub const fn vector(reference: CandidateRef, vector_score_bps: u16) -> Self {
        Self {
            reference,
            channels: RecallChannels::vector(),
            lexical_score: 0,
            vector_score_bps,
        }
    }

    #[must_use]
    pub const fn reference(self) -> CandidateRef {
        self.reference
    }

    #[must_use]
    pub const fn channels(self) -> RecallChannels {
        self.channels
    }

    #[must_use]
    pub const fn lexical_score(self) -> u32 {
        self.lexical_score
    }

    #[must_use]
    pub const fn vector_score_bps(self) -> u16 {
        self.vector_score_bps
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCurrentness {
    Present,
    SourceRemoved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoritativeEvidence {
    view: EvidenceBlockView,
    source_record_id: u64,
    source_locator: String,
    currentness: SourceCurrentness,
    recorded_at_millis: i64,
}

impl AuthoritativeEvidence {
    #[must_use]
    pub const fn new(
        view: EvidenceBlockView,
        source_record_id: u64,
        source_locator: String,
        currentness: SourceCurrentness,
        recorded_at_millis: i64,
    ) -> Self {
        Self {
            view,
            source_record_id,
            source_locator,
            currentness,
            recorded_at_millis,
        }
    }

    #[must_use]
    pub const fn view(&self) -> &EvidenceBlockView {
        &self.view
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
    pub const fn currentness(&self) -> SourceCurrentness {
        self.currentness
    }

    #[must_use]
    pub const fn recorded_at_millis(&self) -> i64 {
        self.recorded_at_millis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthoritativeCandidate {
    Evidence(AuthoritativeEvidence),
    Ledger(Claim),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisputedMemoryRecall {
    dispute_id: u64,
    memory_id: u64,
    memory_version: u64,
    counterpart_view: String,
    counterpart_sources: Vec<Claim>,
    person_position: String,
    person_evidence: Vec<EvidenceCitation>,
    review_rationale: Option<String>,
    review_evidence: Vec<EvidenceCitation>,
    state: DisputeState,
}

impl DisputedMemoryRecall {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        dispute_id: u64,
        memory_id: u64,
        memory_version: u64,
        counterpart_view: String,
        counterpart_sources: Vec<Claim>,
        person_position: String,
        person_evidence: Vec<EvidenceCitation>,
        review_rationale: Option<String>,
        review_evidence: Vec<EvidenceCitation>,
        state: DisputeState,
    ) -> Self {
        Self {
            dispute_id,
            memory_id,
            memory_version,
            counterpart_view,
            counterpart_sources,
            person_position,
            person_evidence,
            review_rationale,
            review_evidence,
            state,
        }
    }

    #[must_use]
    pub const fn dispute_id(&self) -> u64 {
        self.dispute_id
    }

    #[must_use]
    pub const fn memory_id(&self) -> u64 {
        self.memory_id
    }

    #[must_use]
    pub const fn memory_version(&self) -> u64 {
        self.memory_version
    }

    #[must_use]
    pub fn counterpart_view(&self) -> &str {
        &self.counterpart_view
    }

    #[must_use]
    pub fn counterpart_sources(&self) -> &[Claim] {
        &self.counterpart_sources
    }

    #[must_use]
    pub fn person_position(&self) -> &str {
        &self.person_position
    }

    #[must_use]
    pub fn person_evidence(&self) -> &[EvidenceCitation] {
        &self.person_evidence
    }

    #[must_use]
    pub fn review_rationale(&self) -> Option<&str> {
        self.review_rationale.as_deref()
    }

    #[must_use]
    pub fn review_evidence(&self) -> &[EvidenceCitation] {
        &self.review_evidence
    }

    #[must_use]
    pub const fn state(&self) -> DisputeState {
        self.state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievalCandidate {
    reference: CandidateRef,
    authority: AuthoritativeCandidate,
    channels: RecallChannels,
    lexical_score: u32,
    vector_score_bps: u16,
}

impl RetrievalCandidate {
    #[must_use]
    pub const fn new(
        reference: CandidateRef,
        authority: AuthoritativeCandidate,
        channels: RecallChannels,
        lexical_score: u32,
        vector_score_bps: u16,
    ) -> Self {
        Self {
            reference,
            authority,
            channels,
            lexical_score,
            vector_score_bps,
        }
    }

    #[must_use]
    pub const fn reference(&self) -> CandidateRef {
        self.reference
    }

    #[must_use]
    pub const fn authority(&self) -> &AuthoritativeCandidate {
        &self.authority
    }

    #[must_use]
    pub const fn channels(&self) -> RecallChannels {
        self.channels
    }

    #[must_use]
    pub const fn lexical_score(&self) -> u32 {
        self.lexical_score
    }

    #[must_use]
    pub const fn vector_score_bps(&self) -> u16 {
        self.vector_score_bps
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievalResult {
    index: IndexBuildReceipt,
    candidates: Vec<RetrievalCandidate>,
    disputed_memories: Vec<DisputedMemoryRecall>,
}

impl RetrievalResult {
    #[must_use]
    pub const fn index(&self) -> IndexBuildReceipt {
        self.index
    }

    #[must_use]
    pub fn candidates(&self) -> &[RetrievalCandidate] {
        &self.candidates
    }

    #[must_use]
    pub fn disputed_memories(&self) -> &[DisputedMemoryRecall] {
        &self.disputed_memories
    }
}

pub trait RetrievalRepository {
    type Error;

    /// Validates the disposable index against authority and rebuilds it when
    /// absent, stale, or corrupt.
    ///
    /// # Errors
    ///
    /// Returns the adapter error without modifying evidence or ledger rows.
    fn ensure_retrieval_index(&mut self) -> Result<IndexBuildReceipt, Self::Error>;

    /// Returns index-only candidate references. These are not facts and must
    /// still be resolved through `resolve_authoritative`.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the encrypted index cannot be queried.
    fn recall_candidates(&self, query: &RetrievalQuery) -> Result<Vec<RecallHit>, Self::Error>;

    /// Recalls authoritative source references selected by long-term memories.
    /// S14 adapters return an empty set until S16 persists explicit memories.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the memory index cannot be queried.
    fn recall_long_term_memory_candidates(
        &self,
        _query: &RetrievalQuery,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        Ok(Vec::new())
    }

    /// Returns only directly relevant disputed memories as indivisible pairs:
    /// counterpart view and sources, person objection and evidence, plus the
    /// current dispute state. Implementations must not emit one side alone.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when durable dispute state cannot be queried.
    fn recall_disputed_memories(
        &self,
        _query: &RetrievalQuery,
    ) -> Result<Vec<DisputedMemoryRecall>, Self::Error> {
        Ok(Vec::new())
    }

    /// Recalls authority references selected by active deep-understanding
    /// projections. Projection text itself never leaves the routing layer.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the disposable projection index cannot
    /// be queried.
    fn recall_understanding_candidates(
        &self,
        _query: &RetrievalQuery,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        Ok(Vec::new())
    }

    /// Returns a bounded, non-recursive set of structural, temporal, and
    /// relation neighbors for one already-ranked seed.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when neighbor indexes cannot be queried.
    fn recall_neighbors(
        &self,
        _reference: CandidateRef,
        _scope: SourceScope,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        Ok(Vec::new())
    }

    /// Resolves a candidate to current authoritative evidence or a sourced
    /// ledger entry. `None` means the hit is outside the requested source scope.
    ///
    /// # Errors
    ///
    /// Returns the adapter error for missing, corrupt, or unauthenticated authority.
    fn resolve_authoritative(
        &self,
        reference: CandidateRef,
        scope: SourceScope,
    ) -> Result<Option<AuthoritativeCandidate>, Self::Error>;
}

/// Performs multi-channel recall and requires authority resolution for every result.
///
/// # Errors
///
/// Returns query validation failures or the repository adapter error.
pub fn retrieve<R: RetrievalRepository>(
    repository: &mut R,
    query: &RetrievalQuery,
) -> Result<RetrievalResult, RetrievalFailure<R::Error>> {
    query.validate().map_err(RetrievalFailure::Query)?;
    let index = repository
        .ensure_retrieval_index()
        .map_err(RetrievalFailure::Repository)?;
    let disputed_memories = repository
        .recall_disputed_memories(query)
        .map_err(RetrievalFailure::Repository)?;
    let mut hits = repository
        .recall_candidates(query)
        .map_err(RetrievalFailure::Repository)?;
    hits.extend(
        repository
            .recall_long_term_memory_candidates(query)
            .map_err(RetrievalFailure::Repository)?,
    );
    hits.extend(
        repository
            .recall_understanding_candidates(query)
            .map_err(RetrievalFailure::Repository)?,
    );

    let mut merged = BTreeMap::<CandidateRef, RecallHit>::new();
    for hit in hits {
        merged
            .entry(hit.reference)
            .and_modify(|existing| {
                existing.channels = existing.channels.merged(hit.channels);
                existing.lexical_score = existing.lexical_score.saturating_add(hit.lexical_score);
                existing.vector_score_bps = existing.vector_score_bps.max(hit.vector_score_bps);
            })
            .or_insert(hit);
    }
    let mut hits = merged.into_values().collect::<Vec<_>>();
    hits.sort_by_key(|hit| {
        (
            std::cmp::Reverse(hit.channels.count()),
            std::cmp::Reverse(hit.lexical_score),
            std::cmp::Reverse(hit.vector_score_bps),
            hit.reference,
        )
    });

    let mut candidates = Vec::with_capacity(query.limit());
    for hit in hits {
        let Some(authority) = repository
            .resolve_authoritative(hit.reference, query.source_scope())
            .map_err(RetrievalFailure::Repository)?
        else {
            continue;
        };
        candidates.push(RetrievalCandidate::new(
            hit.reference,
            authority,
            hit.channels,
            hit.lexical_score,
            hit.vector_score_bps,
        ));
        if candidates.len() == query.limit() {
            break;
        }
    }
    Ok(RetrievalResult {
        index,
        candidates,
        disputed_memories,
    })
}

/// Normalizes text and emits deterministic word plus Unicode n-gram terms.
///
/// ASCII word runs remain whole. Non-ASCII alphanumeric runs emit the whole
/// run, individual characters, and adjacent bigrams so CJK substring queries
/// do not depend on an external tokenizer.
#[must_use]
pub fn search_terms(value: &str) -> Vec<String> {
    let mut terms = BTreeSet::new();
    let mut run = String::new();
    let flush = |run: &mut String, terms: &mut BTreeSet<String>| {
        if run.is_empty() {
            return;
        }
        terms.insert(run.clone());
        if !run.is_ascii() {
            let characters = run.chars().collect::<Vec<_>>();
            terms.extend(characters.iter().map(char::to_string));
            terms.extend(
                characters
                    .windows(2)
                    .map(|pair| pair.iter().collect::<String>()),
            );
        }
        run.clear();
    };
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            run.push(character);
        } else {
            flush(&mut run, &mut terms);
        }
    }
    flush(&mut run, &mut terms);
    terms.into_iter().collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetrievalError {
    EmptyQuery,
    InvalidTimeRange,
    InvalidLimit,
    InvalidTokenBudget,
}

impl fmt::Display for RetrievalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyQuery => "retrieval query has no lexical, temporal, or entity input",
            Self::InvalidTimeRange => "retrieval time range is reversed",
            Self::InvalidLimit => "retrieval result limit is outside 1..=100",
            Self::InvalidTokenBudget => "retrieval token budget is outside 128..=32768",
        })
    }
}

impl Error for RetrievalError {}

#[derive(Debug)]
pub enum RetrievalFailure<E> {
    Query(RetrievalError),
    Repository(E),
}

impl<E: fmt::Display> fmt::Display for RetrievalFailure<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(error) => write!(formatter, "retrieval query rejected: {error}"),
            Self::Repository(error) => write!(formatter, "retrieval repository failed: {error}"),
        }
    }
}

impl<E: Error + 'static> Error for RetrievalFailure<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Query(error) => Some(error),
            Self::Repository(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_validation_rejects_empty_reversed_and_unbounded_inputs() {
        assert_eq!(
            RetrievalQuery::lexical(" \n ").validate(),
            Err(RetrievalError::EmptyQuery)
        );
        assert_eq!(TimeRange::new(2, 1), Err(RetrievalError::InvalidTimeRange));
        assert_eq!(
            RetrievalQuery::lexical("valid")
                .with_limit(MAX_RESULT_LIMIT + 1)
                .validate(),
            Err(RetrievalError::InvalidLimit)
        );
    }

    #[test]
    fn terms_are_case_folded_and_cjk_substrings_are_addressable() {
        assert_eq!(search_terms("Project PROJECT"), vec!["project"]);
        assert_eq!(
            search_terms("个人项目"),
            vec!["个", "个人", "个人项目", "人", "人项", "目", "项", "项目"]
        );
    }
}
