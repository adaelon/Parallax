use eam_core::{ApplicableTime, ClaimId, Timestamp};

pub const MAX_MEMORY_SOURCES: usize = 64;
pub const MAX_MEMORY_TEXT_BYTES: usize = 16 * 1_024;

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
    Superseded,
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
}
