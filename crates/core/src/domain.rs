use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceId(u64);

impl EvidenceId {
    /// Creates an identifier value supplied by a typed adapter or runtime.
    ///
    /// Possessing an identifier does not grant access: Core still resolves it
    /// against the frozen working context before accepting a citation.
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClaimId(u64);

impl ClaimId {
    /// Restores an identifier supplied by a trusted persistence adapter.
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    #[must_use]
    pub const fn from_millis(value: i64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(String);

impl SessionId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    Person,
    Counterpart,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationEvidence {
    id: EvidenceId,
    session_id: SessionId,
    speaker: Speaker,
    verbatim: String,
    recorded_at: Timestamp,
}

impl ConversationEvidence {
    pub(crate) fn new(
        id: EvidenceId,
        session_id: SessionId,
        speaker: Speaker,
        verbatim: String,
        recorded_at: Timestamp,
    ) -> Self {
        Self {
            id,
            session_id,
            speaker,
            verbatim,
            recorded_at,
        }
    }

    /// Restores immutable evidence from a trusted persistence adapter.
    #[must_use]
    pub fn restore(
        id: EvidenceId,
        session_id: SessionId,
        speaker: Speaker,
        verbatim: String,
        recorded_at: Timestamp,
    ) -> Self {
        Self::new(id, session_id, speaker, verbatim, recorded_at)
    }

    #[must_use]
    pub const fn id(&self) -> EvidenceId {
        self.id
    }

    #[must_use]
    pub const fn speaker(&self) -> Speaker {
        self.speaker
    }

    #[must_use]
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    #[must_use]
    pub fn verbatim(&self) -> &str {
        &self.verbatim
    }

    #[must_use]
    pub const fn recorded_at(&self) -> Timestamp {
        self.recorded_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceCitation {
    evidence_id: EvidenceId,
    quote: String,
}

impl EvidenceCitation {
    #[must_use]
    pub fn new(evidence_id: EvidenceId, quote: impl Into<String>) -> Self {
        Self {
            evidence_id,
            quote: quote.into(),
        }
    }

    #[must_use]
    pub const fn evidence_id(&self) -> EvidenceId {
        self.evidence_id
    }

    #[must_use]
    pub fn quote(&self) -> &str {
        &self.quote
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimOwner {
    Person,
    Counterpart,
    Shared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uncertainty {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplicableTime {
    At(Timestamp),
    Since(Timestamp),
    Between { start: Timestamp, end: Timestamp },
    Unknown,
}

impl ApplicableTime {
    #[must_use]
    pub const fn is_valid(self) -> bool {
        match self {
            Self::Between { start, end } => start.as_millis() <= end.as_millis(),
            Self::At(_) | Self::Since(_) | Self::Unknown => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    id: ClaimId,
    owner: ClaimOwner,
    statement: String,
    support: Vec<EvidenceCitation>,
    uncertainty: Option<Uncertainty>,
    applicable_time: ApplicableTime,
    recorded_at: Timestamp,
}

impl Claim {
    pub(crate) fn new(
        id: ClaimId,
        owner: ClaimOwner,
        statement: String,
        support: Vec<EvidenceCitation>,
        uncertainty: Option<Uncertainty>,
        applicable_time: ApplicableTime,
        recorded_at: Timestamp,
    ) -> Self {
        Self {
            id,
            owner,
            statement,
            support,
            uncertainty,
            applicable_time,
            recorded_at,
        }
    }

    /// Restores an immutable ledger entry from a trusted persistence adapter.
    #[must_use]
    pub fn restore(
        id: ClaimId,
        owner: ClaimOwner,
        statement: String,
        support: Vec<EvidenceCitation>,
        uncertainty: Option<Uncertainty>,
        applicable_time: ApplicableTime,
        recorded_at: Timestamp,
    ) -> Self {
        Self::new(
            id,
            owner,
            statement,
            support,
            uncertainty,
            applicable_time,
            recorded_at,
        )
    }

    #[must_use]
    pub const fn id(&self) -> ClaimId {
        self.id
    }

    #[must_use]
    pub const fn owner(&self) -> ClaimOwner {
        self.owner
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub fn support(&self) -> &[EvidenceCitation] {
        &self.support
    }

    #[must_use]
    pub const fn uncertainty(&self) -> Option<Uncertainty> {
        self.uncertainty
    }

    #[must_use]
    pub const fn applicable_time(&self) -> ApplicableTime {
        self.applicable_time
    }

    #[must_use]
    pub const fn recorded_at(&self) -> Timestamp {
        self.recorded_at
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersonTurnClassification {
    DirectSelfReport,
    Question,
    Joke,
    Hypothetical,
    Quotation,
    Ambiguous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkingContext {
    evidence: Vec<ConversationEvidence>,
    retrieved: Vec<RetrievedContextItem>,
    retrieval_snapshot: Option<RetrievalSnapshot>,
    frozen_at: Timestamp,
}

impl WorkingContext {
    pub(crate) fn new(evidence: Vec<ConversationEvidence>, frozen_at: Timestamp) -> Self {
        Self {
            evidence,
            retrieved: Vec::new(),
            retrieval_snapshot: None,
            frozen_at,
        }
    }

    /// Freezes already-selected conversation evidence without repository access.
    #[must_use]
    pub fn from_selected_evidence(
        evidence: Vec<ConversationEvidence>,
        frozen_at: Timestamp,
    ) -> Self {
        Self::new(evidence, frozen_at)
    }

    /// Attaches one already-authority-resolved retrieval snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`WorkingContextError::BudgetExceeded`] when the frozen payload
    /// claims more estimated tokens than its immutable budget.
    pub fn with_retrieval(
        mut self,
        retrieved: Vec<RetrievedContextItem>,
        snapshot: RetrievalSnapshot,
    ) -> Result<Self, WorkingContextError> {
        if snapshot.used_tokens() > snapshot.token_budget() {
            return Err(WorkingContextError::BudgetExceeded);
        }
        let accounted = retrieved.iter().fold(0_usize, |total, item| {
            total.saturating_add(item.estimated_tokens())
        });
        if accounted != snapshot.used_tokens() {
            return Err(WorkingContextError::TokenAccountingMismatch);
        }
        if retrieved.iter().any(|item| {
            matches!(
                item,
                RetrievedContextItem::EvidenceWindow(window) if window.blocks().is_empty()
            )
        }) {
            return Err(WorkingContextError::EmptyEvidenceWindow);
        }
        self.retrieved = retrieved;
        self.retrieval_snapshot = Some(snapshot);
        Ok(self)
    }

    #[must_use]
    pub fn evidence(&self) -> &[ConversationEvidence] {
        &self.evidence
    }

    #[must_use]
    pub fn retrieved(&self) -> &[RetrievedContextItem] {
        &self.retrieved
    }

    #[must_use]
    pub const fn retrieval_snapshot(&self) -> Option<&RetrievalSnapshot> {
        self.retrieval_snapshot.as_ref()
    }

    #[must_use]
    pub const fn frozen_at(&self) -> Timestamp {
        self.frozen_at
    }

    #[must_use]
    pub fn contains(&self, evidence_id: EvidenceId) -> bool {
        self.evidence
            .iter()
            .any(|evidence| evidence.id() == evidence_id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCurrentness {
    Present,
    SourceRemoved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenEvidenceBlock {
    evidence_id: u64,
    block_id: u64,
    ordinal: usize,
    verbatim: String,
    source_record_id: u64,
    source_locator: String,
    currentness: SourceCurrentness,
    recorded_at: Timestamp,
}

impl FrozenEvidenceBlock {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        evidence_id: u64,
        block_id: u64,
        ordinal: usize,
        verbatim: String,
        source_record_id: u64,
        source_locator: String,
        currentness: SourceCurrentness,
        recorded_at: Timestamp,
    ) -> Self {
        Self {
            evidence_id,
            block_id,
            ordinal,
            verbatim,
            source_record_id,
            source_locator,
            currentness,
            recorded_at,
        }
    }

    #[must_use]
    pub const fn evidence_id(&self) -> u64 {
        self.evidence_id
    }

    #[must_use]
    pub const fn block_id(&self) -> u64 {
        self.block_id
    }

    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
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
    pub const fn currentness(&self) -> SourceCurrentness {
        self.currentness
    }

    #[must_use]
    pub const fn recorded_at(&self) -> Timestamp {
        self.recorded_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenRetrievalWindow {
    ordinal: usize,
    blocks: Vec<FrozenEvidenceBlock>,
    estimated_tokens: usize,
}

impl FrozenRetrievalWindow {
    #[must_use]
    pub const fn new(
        ordinal: usize,
        blocks: Vec<FrozenEvidenceBlock>,
        estimated_tokens: usize,
    ) -> Self {
        Self {
            ordinal,
            blocks,
            estimated_tokens,
        }
    }

    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    #[must_use]
    pub fn blocks(&self) -> &[FrozenEvidenceBlock] {
        &self.blocks
    }

    #[must_use]
    pub const fn estimated_tokens(&self) -> usize {
        self.estimated_tokens
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenLedgerClaim {
    claim: Claim,
    estimated_tokens: usize,
}

impl FrozenLedgerClaim {
    #[must_use]
    pub const fn new(claim: Claim, estimated_tokens: usize) -> Self {
        Self {
            claim,
            estimated_tokens,
        }
    }

    #[must_use]
    pub const fn claim(&self) -> &Claim {
        &self.claim
    }

    #[must_use]
    pub const fn estimated_tokens(&self) -> usize {
        self.estimated_tokens
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetrievedContextItem {
    EvidenceWindow(FrozenRetrievalWindow),
    LedgerClaim(FrozenLedgerClaim),
}

impl RetrievedContextItem {
    #[must_use]
    pub const fn estimated_tokens(&self) -> usize {
        match self {
            Self::EvidenceWindow(window) => window.estimated_tokens(),
            Self::LedgerClaim(claim) => claim.estimated_tokens(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievalSnapshot {
    retrieval_contract_version: String,
    vector_model_version: String,
    token_budget: usize,
    used_tokens: usize,
    replay_digest: [u8; 32],
}

impl RetrievalSnapshot {
    #[must_use]
    pub fn new(
        retrieval_contract_version: impl Into<String>,
        vector_model_version: impl Into<String>,
        token_budget: usize,
        used_tokens: usize,
        replay_digest: [u8; 32],
    ) -> Self {
        Self {
            retrieval_contract_version: retrieval_contract_version.into(),
            vector_model_version: vector_model_version.into(),
            token_budget,
            used_tokens,
            replay_digest,
        }
    }

    #[must_use]
    pub fn retrieval_contract_version(&self) -> &str {
        &self.retrieval_contract_version
    }

    #[must_use]
    pub fn vector_model_version(&self) -> &str {
        &self.vector_model_version
    }

    #[must_use]
    pub const fn token_budget(&self) -> usize {
        self.token_budget
    }

    #[must_use]
    pub const fn used_tokens(&self) -> usize {
        self.used_tokens
    }

    #[must_use]
    pub const fn replay_digest(&self) -> &[u8; 32] {
        &self.replay_digest
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingContextError {
    BudgetExceeded,
    TokenAccountingMismatch,
    EmptyEvidenceWindow,
}

impl fmt::Display for WorkingContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BudgetExceeded => "retrieved working context exceeds its frozen token budget",
            Self::TokenAccountingMismatch => {
                "retrieved working context token accounting does not match its frozen snapshot"
            }
            Self::EmptyEvidenceWindow => "retrieved working context contains an empty window",
        })
    }
}

impl std::error::Error for WorkingContextError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgmentProposal {
    statement: String,
    support: Vec<EvidenceCitation>,
    uncertainty: Uncertainty,
    applicable_time: ApplicableTime,
}

impl JudgmentProposal {
    #[must_use]
    pub fn new(
        statement: impl Into<String>,
        support: Vec<EvidenceCitation>,
        uncertainty: Uncertainty,
        applicable_time: ApplicableTime,
    ) -> Self {
        Self {
            statement: statement.into(),
            support,
            uncertainty,
            applicable_time,
        }
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub fn support(&self) -> &[EvidenceCitation] {
        &self.support
    }

    #[must_use]
    pub const fn uncertainty(&self) -> Uncertainty {
        self.uncertainty
    }

    #[must_use]
    pub const fn applicable_time(&self) -> ApplicableTime {
        self.applicable_time
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeResponse {
    text: String,
    citations: Vec<EvidenceCitation>,
    judgment_proposals: Vec<JudgmentProposal>,
    unsupported_operations: Vec<UnsupportedStructuredOperation>,
}

impl RuntimeResponse {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            citations: Vec::new(),
            judgment_proposals: Vec::new(),
            unsupported_operations: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_citation(mut self, citation: EvidenceCitation) -> Self {
        self.citations.push(citation);
        self
    }

    #[must_use]
    pub fn with_judgment(mut self, proposal: JudgmentProposal) -> Self {
        self.judgment_proposals.push(proposal);
        self
    }

    #[must_use]
    pub fn with_unsupported_operation(
        mut self,
        operation_index: usize,
        name: impl Into<String>,
    ) -> Self {
        self.unsupported_operations
            .push(UnsupportedStructuredOperation::new(operation_index, name));
        self
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn citations(&self) -> &[EvidenceCitation] {
        &self.citations
    }

    #[must_use]
    pub fn judgment_proposals(&self) -> &[JudgmentProposal] {
        &self.judgment_proposals
    }

    #[must_use]
    pub fn unsupported_operations(&self) -> &[UnsupportedStructuredOperation] {
        &self.unsupported_operations
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedStructuredOperation {
    operation_index: usize,
    name: String,
}

impl UnsupportedStructuredOperation {
    #[must_use]
    pub fn new(operation_index: usize, name: impl Into<String>) -> Self {
        Self {
            operation_index,
            name: name.into(),
        }
    }

    #[must_use]
    pub const fn operation_index(&self) -> usize {
        self.operation_index
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRequest {
    prompt: ConversationEvidence,
    working_context: WorkingContext,
}

impl RuntimeRequest {
    pub(crate) fn new(prompt: ConversationEvidence, working_context: WorkingContext) -> Self {
        Self {
            prompt,
            working_context,
        }
    }

    #[must_use]
    pub fn prompt(&self) -> &ConversationEvidence {
        &self.prompt
    }

    #[must_use]
    pub fn working_context(&self) -> &WorkingContext {
        &self.working_context
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JudgmentRejectionReason {
    EmptyStatement,
    MissingSupport,
    EvidenceOutsideWorkingContext(EvidenceId),
    EmptyQuote(EvidenceId),
    QuoteMismatch(EvidenceId),
    InvalidApplicableTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgmentRejection {
    proposal_index: usize,
    reason: JudgmentRejectionReason,
}

impl JudgmentRejection {
    pub(crate) const fn new(proposal_index: usize, reason: JudgmentRejectionReason) -> Self {
        Self {
            proposal_index,
            reason,
        }
    }

    #[must_use]
    pub const fn proposal_index(&self) -> usize {
        self.proposal_index
    }

    #[must_use]
    pub const fn reason(&self) -> &JudgmentRejectionReason {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructuredOperationRejectionReason {
    NotWhitelisted(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredOperationRejection {
    operation_index: usize,
    reason: StructuredOperationRejectionReason,
}

impl StructuredOperationRejection {
    pub(crate) const fn new(
        operation_index: usize,
        reason: StructuredOperationRejectionReason,
    ) -> Self {
        Self {
            operation_index,
            reason,
        }
    }

    #[must_use]
    pub const fn operation_index(&self) -> usize {
        self.operation_index
    }

    #[must_use]
    pub const fn reason(&self) -> &StructuredOperationRejectionReason {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnOutcome {
    person_evidence_id: EvidenceId,
    counterpart_evidence_id: EvidenceId,
    person_classification: PersonTurnClassification,
    accepted_judgment_ids: Vec<ClaimId>,
    rejected_judgments: Vec<JudgmentRejection>,
    rejected_operations: Vec<StructuredOperationRejection>,
    validated_citations: Vec<EvidenceCitation>,
}

impl TurnOutcome {
    pub(crate) fn new(
        person_evidence_id: EvidenceId,
        counterpart_evidence_id: EvidenceId,
        person_classification: PersonTurnClassification,
        accepted_judgment_ids: Vec<ClaimId>,
        rejected_judgments: Vec<JudgmentRejection>,
        rejected_operations: Vec<StructuredOperationRejection>,
        validated_citations: Vec<EvidenceCitation>,
    ) -> Self {
        Self {
            person_evidence_id,
            counterpart_evidence_id,
            person_classification,
            accepted_judgment_ids,
            rejected_judgments,
            rejected_operations,
            validated_citations,
        }
    }

    #[must_use]
    pub const fn person_evidence_id(&self) -> EvidenceId {
        self.person_evidence_id
    }

    #[must_use]
    pub const fn counterpart_evidence_id(&self) -> EvidenceId {
        self.counterpart_evidence_id
    }

    #[must_use]
    pub const fn person_classification(&self) -> PersonTurnClassification {
        self.person_classification
    }

    #[must_use]
    pub fn accepted_judgment_ids(&self) -> &[ClaimId] {
        &self.accepted_judgment_ids
    }

    #[must_use]
    pub fn rejected_judgments(&self) -> &[JudgmentRejection] {
        &self.rejected_judgments
    }

    #[must_use]
    pub fn rejected_operations(&self) -> &[StructuredOperationRejection] {
        &self.rejected_operations
    }

    #[must_use]
    pub fn validated_citations(&self) -> &[EvidenceCitation] {
        &self.validated_citations
    }
}
