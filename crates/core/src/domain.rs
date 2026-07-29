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
    frozen_at: Timestamp,
}

impl WorkingContext {
    pub(crate) fn new(evidence: Vec<ConversationEvidence>, frozen_at: Timestamp) -> Self {
        Self {
            evidence,
            frozen_at,
        }
    }

    #[must_use]
    pub fn evidence(&self) -> &[ConversationEvidence] {
        &self.evidence
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
}

impl RuntimeResponse {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            citations: Vec::new(),
            judgment_proposals: Vec::new(),
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
pub struct TurnOutcome {
    person_evidence_id: EvidenceId,
    counterpart_evidence_id: EvidenceId,
    person_classification: PersonTurnClassification,
    accepted_judgment_ids: Vec<ClaimId>,
    rejected_judgments: Vec<JudgmentRejection>,
    validated_citations: Vec<EvidenceCitation>,
}

impl TurnOutcome {
    pub(crate) fn new(
        person_evidence_id: EvidenceId,
        counterpart_evidence_id: EvidenceId,
        person_classification: PersonTurnClassification,
        accepted_judgment_ids: Vec<ClaimId>,
        rejected_judgments: Vec<JudgmentRejection>,
        validated_citations: Vec<EvidenceCitation>,
    ) -> Self {
        Self {
            person_evidence_id,
            counterpart_evidence_id,
            person_classification,
            accepted_judgment_ids,
            rejected_judgments,
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
    pub fn validated_citations(&self) -> &[EvidenceCitation] {
        &self.validated_citations
    }
}
