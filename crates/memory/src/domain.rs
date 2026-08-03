use eam_core::{ApplicableTime, ClaimId, EvidenceCitation, Timestamp};

pub const MAX_MEMORY_SOURCES: usize = 64;
pub const MAX_MEMORY_TEXT_BYTES: usize = 16 * 1_024;
pub const MAX_DISPUTE_EVIDENCE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemoryId(u64);

impl MemoryId {
    /// Restores a positive identifier supplied by a trusted repository.
    ///
    /// Returns `None` for zero because persistent identifiers are one-based.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            return None;
        }
        Some(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemorySubject {
    Person,
    Counterpart,
    Shared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryKind {
    Fact,
    Preference,
    Goal,
    Relationship,
    Hypothesis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryBasis {
    DirectEvidence,
    InterpretiveInference,
    PatternCandidate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryStatus {
    Active,
    Provisional,
    ProvisionalPattern,
    SupportedCounterpartView,
    Disputed,
    Superseded,
    Retracted,
    Weakened,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemoryDisputeId(u64);

impl MemoryDisputeId {
    /// Restores a positive identifier supplied by a trusted repository.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            return None;
        }
        Some(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryDisputeOutcome {
    Open,
    Retracted,
    Revised,
    Maintained,
    Weakened,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDisputeRequest {
    memory_id: MemoryId,
    expected_version: u64,
    reason: String,
    counter_evidence: Vec<EvidenceCitation>,
}

impl MemoryDisputeRequest {
    #[must_use]
    pub fn new(memory_id: MemoryId, expected_version: u64, reason: impl Into<String>) -> Self {
        Self {
            memory_id,
            expected_version,
            reason: reason.into(),
            counter_evidence: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_counter_evidence(mut self, evidence: EvidenceCitation) -> Self {
        self.counter_evidence.push(evidence);
        self
    }

    #[must_use]
    pub fn with_counter_evidence_all(
        mut self,
        evidence: impl IntoIterator<Item = EvidenceCitation>,
    ) -> Self {
        self.counter_evidence.extend(evidence);
        self
    }

    #[must_use]
    pub const fn memory_id(&self) -> MemoryId {
        self.memory_id
    }

    #[must_use]
    pub const fn expected_version(&self) -> u64 {
        self.expected_version
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub fn counter_evidence(&self) -> &[EvidenceCitation] {
        &self.counter_evidence
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryDisputeReviewDecision {
    Retract,
    Revise(MemoryProposal),
    Maintain,
    Weaken,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDisputeReview {
    dispute_id: MemoryDisputeId,
    rationale: String,
    evidence: Vec<EvidenceCitation>,
    decision: MemoryDisputeReviewDecision,
}

impl MemoryDisputeReview {
    #[must_use]
    pub fn maintain(dispute_id: MemoryDisputeId, rationale: impl Into<String>) -> Self {
        Self {
            dispute_id,
            rationale: rationale.into(),
            evidence: Vec::new(),
            decision: MemoryDisputeReviewDecision::Maintain,
        }
    }

    #[must_use]
    pub fn retract(dispute_id: MemoryDisputeId, rationale: impl Into<String>) -> Self {
        Self {
            dispute_id,
            rationale: rationale.into(),
            evidence: Vec::new(),
            decision: MemoryDisputeReviewDecision::Retract,
        }
    }

    #[must_use]
    pub fn weaken(dispute_id: MemoryDisputeId, rationale: impl Into<String>) -> Self {
        Self {
            dispute_id,
            rationale: rationale.into(),
            evidence: Vec::new(),
            decision: MemoryDisputeReviewDecision::Weaken,
        }
    }

    #[must_use]
    pub fn revise(
        dispute_id: MemoryDisputeId,
        rationale: impl Into<String>,
        proposal: MemoryProposal,
    ) -> Self {
        Self {
            dispute_id,
            rationale: rationale.into(),
            evidence: Vec::new(),
            decision: MemoryDisputeReviewDecision::Revise(proposal),
        }
    }

    #[must_use]
    pub fn with_evidence(mut self, evidence: EvidenceCitation) -> Self {
        self.evidence.push(evidence);
        self
    }

    #[must_use]
    pub fn with_evidence_all(
        mut self,
        evidence: impl IntoIterator<Item = EvidenceCitation>,
    ) -> Self {
        self.evidence.extend(evidence);
        self
    }

    #[must_use]
    pub const fn dispute_id(&self) -> MemoryDisputeId {
        self.dispute_id
    }

    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    #[must_use]
    pub fn evidence(&self) -> &[EvidenceCitation] {
        &self.evidence
    }

    #[must_use]
    pub const fn decision(&self) -> &MemoryDisputeReviewDecision {
        &self.decision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedMemoryDispute {
    memory_id: MemoryId,
    memory_version: u64,
    reason: String,
    counter_evidence: Vec<EvidenceCitation>,
}

impl ValidatedMemoryDispute {
    pub(crate) const fn new(
        memory_id: MemoryId,
        memory_version: u64,
        reason: String,
        counter_evidence: Vec<EvidenceCitation>,
    ) -> Self {
        Self {
            memory_id,
            memory_version,
            reason,
            counter_evidence,
        }
    }

    #[must_use]
    pub const fn memory_id(&self) -> MemoryId {
        self.memory_id
    }

    #[must_use]
    pub const fn memory_version(&self) -> u64 {
        self.memory_version
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub fn counter_evidence(&self) -> &[EvidenceCitation] {
        &self.counter_evidence
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedMemoryDisputeReview {
    dispute_id: MemoryDisputeId,
    outcome: MemoryDisputeOutcome,
    rationale: String,
    evidence: Vec<EvidenceCitation>,
    revision: Option<ValidatedMemoryProposal>,
}

impl ValidatedMemoryDisputeReview {
    pub(crate) const fn new(
        dispute_id: MemoryDisputeId,
        outcome: MemoryDisputeOutcome,
        rationale: String,
        evidence: Vec<EvidenceCitation>,
        revision: Option<ValidatedMemoryProposal>,
    ) -> Self {
        Self {
            dispute_id,
            outcome,
            rationale,
            evidence,
            revision,
        }
    }

    #[must_use]
    pub const fn dispute_id(&self) -> MemoryDisputeId {
        self.dispute_id
    }

    #[must_use]
    pub const fn outcome(&self) -> MemoryDisputeOutcome {
        self.outcome
    }

    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    #[must_use]
    pub fn evidence(&self) -> &[EvidenceCitation] {
        &self.evidence
    }

    #[must_use]
    pub const fn revision(&self) -> Option<&ValidatedMemoryProposal> {
        self.revision.as_ref()
    }

    #[must_use]
    pub fn into_revision(self) -> Option<ValidatedMemoryProposal> {
        self.revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDisputeReviewRecord {
    outcome: MemoryDisputeOutcome,
    rationale: String,
    evidence: Vec<EvidenceCitation>,
    reviewed_at: Timestamp,
}

impl MemoryDisputeReviewRecord {
    #[must_use]
    pub const fn restore(
        outcome: MemoryDisputeOutcome,
        rationale: String,
        evidence: Vec<EvidenceCitation>,
        reviewed_at: Timestamp,
    ) -> Self {
        Self {
            outcome,
            rationale,
            evidence,
            reviewed_at,
        }
    }

    #[must_use]
    pub const fn outcome(&self) -> MemoryDisputeOutcome {
        self.outcome
    }

    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    #[must_use]
    pub fn evidence(&self) -> &[EvidenceCitation] {
        &self.evidence
    }

    #[must_use]
    pub const fn reviewed_at(&self) -> Timestamp {
        self.reviewed_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDispute {
    id: MemoryDisputeId,
    memory_id: MemoryId,
    memory_version: u64,
    reason: String,
    counter_evidence: Vec<EvidenceCitation>,
    raised_at: Timestamp,
    outcome: MemoryDisputeOutcome,
    review: Option<MemoryDisputeReviewRecord>,
    revised_version: Option<u64>,
}

impl MemoryDispute {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn restore(
        id: MemoryDisputeId,
        memory_id: MemoryId,
        memory_version: u64,
        reason: String,
        counter_evidence: Vec<EvidenceCitation>,
        raised_at: Timestamp,
        outcome: MemoryDisputeOutcome,
        review: Option<MemoryDisputeReviewRecord>,
        revised_version: Option<u64>,
    ) -> Self {
        Self {
            id,
            memory_id,
            memory_version,
            reason,
            counter_evidence,
            raised_at,
            outcome,
            review,
            revised_version,
        }
    }

    pub(crate) fn set_review(
        &mut self,
        review: MemoryDisputeReviewRecord,
        revised_version: Option<u64>,
    ) {
        self.outcome = review.outcome();
        self.review = Some(review);
        self.revised_version = revised_version;
    }

    #[must_use]
    pub const fn id(&self) -> MemoryDisputeId {
        self.id
    }

    #[must_use]
    pub const fn memory_id(&self) -> MemoryId {
        self.memory_id
    }

    #[must_use]
    pub const fn memory_version(&self) -> u64 {
        self.memory_version
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub fn counter_evidence(&self) -> &[EvidenceCitation] {
        &self.counter_evidence
    }

    #[must_use]
    pub const fn raised_at(&self) -> Timestamp {
        self.raised_at
    }

    #[must_use]
    pub const fn outcome(&self) -> MemoryDisputeOutcome {
        self.outcome
    }

    #[must_use]
    pub const fn review(&self) -> Option<&MemoryDisputeReviewRecord> {
        self.review.as_ref()
    }

    #[must_use]
    pub const fn revised_version(&self) -> Option<u64> {
        self.revised_version
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDisputeResolution {
    dispute: MemoryDispute,
    memory: MemoryVersion,
}

impl MemoryDisputeResolution {
    #[must_use]
    pub const fn new(dispute: MemoryDispute, memory: MemoryVersion) -> Self {
        Self { dispute, memory }
    }

    #[must_use]
    pub const fn dispute(&self) -> &MemoryDispute {
        &self.dispute
    }

    #[must_use]
    pub const fn memory(&self) -> &MemoryVersion {
        &self.memory
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryTarget {
    New,
    Revise {
        memory_id: MemoryId,
        expected_version: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProposal {
    target: MemoryTarget,
    statement: String,
    subject: Option<MemorySubject>,
    kind: Option<MemoryKind>,
    source_claim_ids: Vec<ClaimId>,
    applicable_time: Option<ApplicableTime>,
    confidence: Option<MemoryConfidence>,
    salience_reason: String,
    basis: Option<MemoryBasis>,
    pattern_counterexample_review: Option<EvidenceCitation>,
}

impl MemoryProposal {
    #[must_use]
    pub fn new(statement: impl Into<String>) -> Self {
        Self {
            target: MemoryTarget::New,
            statement: statement.into(),
            subject: None,
            kind: None,
            source_claim_ids: Vec::new(),
            applicable_time: None,
            confidence: None,
            salience_reason: String::new(),
            basis: None,
            pattern_counterexample_review: None,
        }
    }

    #[must_use]
    pub const fn revising(mut self, memory_id: MemoryId, expected_version: u64) -> Self {
        self.target = MemoryTarget::Revise {
            memory_id,
            expected_version,
        };
        self
    }

    #[must_use]
    pub const fn with_subject(mut self, subject: MemorySubject) -> Self {
        self.subject = Some(subject);
        self
    }

    #[must_use]
    pub const fn with_kind(mut self, kind: MemoryKind) -> Self {
        self.kind = Some(kind);
        self
    }

    #[must_use]
    pub fn with_source_claim(mut self, claim_id: ClaimId) -> Self {
        self.source_claim_ids.push(claim_id);
        self
    }

    #[must_use]
    pub fn with_source_claims(mut self, claim_ids: impl IntoIterator<Item = ClaimId>) -> Self {
        self.source_claim_ids.extend(claim_ids);
        self
    }

    #[must_use]
    pub const fn with_applicable_time(mut self, applicable_time: ApplicableTime) -> Self {
        self.applicable_time = Some(applicable_time);
        self
    }

    #[must_use]
    pub const fn with_confidence(mut self, confidence: MemoryConfidence) -> Self {
        self.confidence = Some(confidence);
        self
    }

    #[must_use]
    pub fn with_salience_reason(mut self, salience_reason: impl Into<String>) -> Self {
        self.salience_reason = salience_reason.into();
        self
    }

    #[must_use]
    pub const fn with_basis(mut self, basis: MemoryBasis) -> Self {
        self.basis = Some(basis);
        self
    }

    #[must_use]
    pub fn with_pattern_counterexample_review(mut self, evidence: EvidenceCitation) -> Self {
        self.pattern_counterexample_review = Some(evidence);
        self
    }

    #[must_use]
    pub const fn target(&self) -> MemoryTarget {
        self.target
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub const fn subject(&self) -> Option<MemorySubject> {
        self.subject
    }

    #[must_use]
    pub const fn kind(&self) -> Option<MemoryKind> {
        self.kind
    }

    #[must_use]
    pub fn source_claim_ids(&self) -> &[ClaimId] {
        &self.source_claim_ids
    }

    #[must_use]
    pub const fn applicable_time(&self) -> Option<ApplicableTime> {
        self.applicable_time
    }

    #[must_use]
    pub const fn confidence(&self) -> Option<MemoryConfidence> {
        self.confidence
    }

    #[must_use]
    pub fn salience_reason(&self) -> &str {
        &self.salience_reason
    }

    #[must_use]
    pub const fn basis(&self) -> Option<MemoryBasis> {
        self.basis
    }

    #[must_use]
    pub const fn pattern_counterexample_review(&self) -> Option<&EvidenceCitation> {
        self.pattern_counterexample_review.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedMemoryProposal {
    target: MemoryTarget,
    statement: String,
    subject: MemorySubject,
    kind: MemoryKind,
    source_claim_ids: Vec<ClaimId>,
    applicable_time: ApplicableTime,
    confidence: MemoryConfidence,
    salience_reason: String,
    basis: MemoryBasis,
    initial_status: MemoryStatus,
    pattern_counterexample_review: Option<EvidenceCitation>,
}

impl ValidatedMemoryProposal {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        target: MemoryTarget,
        statement: String,
        subject: MemorySubject,
        kind: MemoryKind,
        source_claim_ids: Vec<ClaimId>,
        applicable_time: ApplicableTime,
        confidence: MemoryConfidence,
        salience_reason: String,
        basis: MemoryBasis,
        initial_status: MemoryStatus,
        pattern_counterexample_review: Option<EvidenceCitation>,
    ) -> Self {
        Self {
            target,
            statement,
            subject,
            kind,
            source_claim_ids,
            applicable_time,
            confidence,
            salience_reason,
            basis,
            initial_status,
            pattern_counterexample_review,
        }
    }

    #[must_use]
    pub const fn target(&self) -> MemoryTarget {
        self.target
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub const fn subject(&self) -> MemorySubject {
        self.subject
    }

    #[must_use]
    pub const fn kind(&self) -> MemoryKind {
        self.kind
    }

    #[must_use]
    pub fn source_claim_ids(&self) -> &[ClaimId] {
        &self.source_claim_ids
    }

    #[must_use]
    pub const fn applicable_time(&self) -> ApplicableTime {
        self.applicable_time
    }

    #[must_use]
    pub const fn confidence(&self) -> MemoryConfidence {
        self.confidence
    }

    #[must_use]
    pub fn salience_reason(&self) -> &str {
        &self.salience_reason
    }

    #[must_use]
    pub const fn basis(&self) -> MemoryBasis {
        self.basis
    }

    #[must_use]
    pub const fn initial_status(&self) -> MemoryStatus {
        self.initial_status
    }

    #[must_use]
    pub const fn pattern_counterexample_review(&self) -> Option<&EvidenceCitation> {
        self.pattern_counterexample_review.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedPatternMaturityProposal {
    memory_id: MemoryId,
    expected_version: u64,
    new_support_claim_ids: Vec<ClaimId>,
    all_source_claim_ids: Vec<ClaimId>,
    counter_evidence_refs: Vec<EvidenceCitation>,
    counterexample_review_ref: EvidenceCitation,
    discussion_evidence_refs: Vec<EvidenceCitation>,
    rationale: String,
}

impl ValidatedPatternMaturityProposal {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        memory_id: MemoryId,
        expected_version: u64,
        new_support_claim_ids: Vec<ClaimId>,
        all_source_claim_ids: Vec<ClaimId>,
        counter_evidence_refs: Vec<EvidenceCitation>,
        counterexample_review_ref: EvidenceCitation,
        discussion_evidence_refs: Vec<EvidenceCitation>,
        rationale: String,
    ) -> Self {
        Self {
            memory_id,
            expected_version,
            new_support_claim_ids,
            all_source_claim_ids,
            counter_evidence_refs,
            counterexample_review_ref,
            discussion_evidence_refs,
            rationale,
        }
    }

    #[must_use]
    pub const fn memory_id(&self) -> MemoryId {
        self.memory_id
    }

    #[must_use]
    pub const fn expected_version(&self) -> u64 {
        self.expected_version
    }

    #[must_use]
    pub fn new_support_claim_ids(&self) -> &[ClaimId] {
        &self.new_support_claim_ids
    }

    #[must_use]
    pub fn all_source_claim_ids(&self) -> &[ClaimId] {
        &self.all_source_claim_ids
    }

    #[must_use]
    pub fn counter_evidence_refs(&self) -> &[EvidenceCitation] {
        &self.counter_evidence_refs
    }

    #[must_use]
    pub const fn counterexample_review_ref(&self) -> &EvidenceCitation {
        &self.counterexample_review_ref
    }

    #[must_use]
    pub fn discussion_evidence_refs(&self) -> &[EvidenceCitation] {
        &self.discussion_evidence_refs
    }

    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternMaturityRecord {
    memory_id: MemoryId,
    from_version: u64,
    to_version: u64,
    new_support_claim_ids: Vec<ClaimId>,
    counter_evidence_refs: Vec<EvidenceCitation>,
    counterexample_review_ref: EvidenceCitation,
    discussion_evidence_refs: Vec<EvidenceCitation>,
    rationale: String,
    proposed_at: Timestamp,
}

impl PatternMaturityRecord {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        memory_id: MemoryId,
        from_version: u64,
        to_version: u64,
        new_support_claim_ids: Vec<ClaimId>,
        counter_evidence_refs: Vec<EvidenceCitation>,
        counterexample_review_ref: EvidenceCitation,
        discussion_evidence_refs: Vec<EvidenceCitation>,
        rationale: String,
        proposed_at: Timestamp,
    ) -> Self {
        Self {
            memory_id,
            from_version,
            to_version,
            new_support_claim_ids,
            counter_evidence_refs,
            counterexample_review_ref,
            discussion_evidence_refs,
            rationale,
            proposed_at,
        }
    }

    #[must_use]
    pub const fn memory_id(&self) -> MemoryId {
        self.memory_id
    }

    #[must_use]
    pub const fn from_version(&self) -> u64 {
        self.from_version
    }

    #[must_use]
    pub const fn to_version(&self) -> u64 {
        self.to_version
    }

    #[must_use]
    pub fn new_support_claim_ids(&self) -> &[ClaimId] {
        &self.new_support_claim_ids
    }

    #[must_use]
    pub fn counter_evidence_refs(&self) -> &[EvidenceCitation] {
        &self.counter_evidence_refs
    }

    #[must_use]
    pub const fn counterexample_review_ref(&self) -> &EvidenceCitation {
        &self.counterexample_review_ref
    }

    #[must_use]
    pub fn discussion_evidence(&self) -> &[EvidenceCitation] {
        &self.discussion_evidence_refs
    }

    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    #[must_use]
    pub const fn proposed_at(&self) -> Timestamp {
        self.proposed_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryVersion {
    id: MemoryId,
    version: u64,
    predecessor_version: Option<u64>,
    statement: String,
    subject: MemorySubject,
    kind: MemoryKind,
    source_claim_ids: Vec<ClaimId>,
    applicable_time: ApplicableTime,
    confidence: MemoryConfidence,
    salience_reason: String,
    basis: MemoryBasis,
    status: MemoryStatus,
    formed_at: Timestamp,
    pattern_counterexample_review: Option<EvidenceCitation>,
}

impl MemoryVersion {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn restore(
        id: MemoryId,
        version: u64,
        predecessor_version: Option<u64>,
        statement: String,
        subject: MemorySubject,
        kind: MemoryKind,
        source_claim_ids: Vec<ClaimId>,
        applicable_time: ApplicableTime,
        confidence: MemoryConfidence,
        salience_reason: String,
        basis: MemoryBasis,
        status: MemoryStatus,
        formed_at: Timestamp,
        pattern_counterexample_review: Option<EvidenceCitation>,
    ) -> Self {
        Self {
            id,
            version,
            predecessor_version,
            statement,
            subject,
            kind,
            source_claim_ids,
            applicable_time,
            confidence,
            salience_reason,
            basis,
            status,
            formed_at,
            pattern_counterexample_review,
        }
    }

    pub(crate) fn set_status(&mut self, status: MemoryStatus) {
        self.status = status;
    }

    #[must_use]
    pub const fn id(&self) -> MemoryId {
        self.id
    }

    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    #[must_use]
    pub const fn predecessor_version(&self) -> Option<u64> {
        self.predecessor_version
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub const fn subject(&self) -> MemorySubject {
        self.subject
    }

    #[must_use]
    pub const fn kind(&self) -> MemoryKind {
        self.kind
    }

    #[must_use]
    pub fn source_claim_ids(&self) -> &[ClaimId] {
        &self.source_claim_ids
    }

    #[must_use]
    pub const fn applicable_time(&self) -> ApplicableTime {
        self.applicable_time
    }

    #[must_use]
    pub const fn confidence(&self) -> MemoryConfidence {
        self.confidence
    }

    #[must_use]
    pub fn salience_reason(&self) -> &str {
        &self.salience_reason
    }

    #[must_use]
    pub const fn basis(&self) -> MemoryBasis {
        self.basis
    }

    #[must_use]
    pub const fn status(&self) -> MemoryStatus {
        self.status
    }

    #[must_use]
    pub const fn formed_at(&self) -> Timestamp {
        self.formed_at
    }

    #[must_use]
    pub const fn pattern_counterexample_review(&self) -> Option<&EvidenceCitation> {
        self.pattern_counterexample_review.as_ref()
    }
}
