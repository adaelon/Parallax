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
pub struct SharedAgreementCandidateId(u64);

impl SharedAgreementCandidateId {
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
pub enum ForgetTarget {
    ConversationEvidence(EvidenceId),
    ArchivedEvidence(u64),
}

impl ForgetTarget {
    #[must_use]
    pub const fn identifier(self) -> u64 {
        match self {
            Self::ConversationEvidence(id) => id.get(),
            Self::ArchivedEvidence(id) => id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForgetRequest {
    target: ForgetTarget,
    confirmed_by_person: bool,
}

impl ForgetRequest {
    #[must_use]
    pub const fn new(target: ForgetTarget, confirmed_by_person: bool) -> Self {
        Self {
            target,
            confirmed_by_person,
        }
    }

    #[must_use]
    pub const fn target(self) -> ForgetTarget {
        self.target
    }

    #[must_use]
    pub const fn confirmed_by_person(self) -> bool {
        self.confirmed_by_person
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForgetReceipt {
    deletion_intent_id: u64,
    target: ForgetTarget,
    removed_authority_records: usize,
    removed_derived_records: usize,
    released_object_references: usize,
}

impl ForgetReceipt {
    #[must_use]
    pub const fn new(
        deletion_intent_id: u64,
        target: ForgetTarget,
        removed_authority_records: usize,
        removed_derived_records: usize,
        released_object_references: usize,
    ) -> Self {
        Self {
            deletion_intent_id,
            target,
            removed_authority_records,
            removed_derived_records,
            released_object_references,
        }
    }

    #[must_use]
    pub const fn deletion_intent_id(self) -> u64 {
        self.deletion_intent_id
    }

    #[must_use]
    pub const fn target(self) -> ForgetTarget {
        self.target
    }

    #[must_use]
    pub const fn removed_authority_records(self) -> usize {
        self.removed_authority_records
    }

    #[must_use]
    pub const fn removed_derived_records(self) -> usize {
        self.removed_derived_records
    }

    #[must_use]
    pub const fn released_object_references(self) -> usize {
        self.released_object_references
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
pub enum ClaimStatus {
    Current,
    Superseded,
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
    status: ClaimStatus,
    supersedes: Option<ClaimId>,
    superseded_by: Option<ClaimId>,
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
        Self::new_versioned(
            id,
            owner,
            statement,
            support,
            uncertainty,
            applicable_time,
            recorded_at,
            ClaimStatus::Current,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn correction(
        id: ClaimId,
        owner: ClaimOwner,
        statement: String,
        support: Vec<EvidenceCitation>,
        uncertainty: Option<Uncertainty>,
        applicable_time: ApplicableTime,
        recorded_at: Timestamp,
        supersedes: ClaimId,
    ) -> Self {
        Self::new_versioned(
            id,
            owner,
            statement,
            support,
            uncertainty,
            applicable_time,
            recorded_at,
            ClaimStatus::Current,
            Some(supersedes),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_versioned(
        id: ClaimId,
        owner: ClaimOwner,
        statement: String,
        support: Vec<EvidenceCitation>,
        uncertainty: Option<Uncertainty>,
        applicable_time: ApplicableTime,
        recorded_at: Timestamp,
        status: ClaimStatus,
        supersedes: Option<ClaimId>,
        superseded_by: Option<ClaimId>,
    ) -> Self {
        Self {
            id,
            owner,
            statement,
            support,
            uncertainty,
            applicable_time,
            recorded_at,
            status,
            supersedes,
            superseded_by,
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

    /// Restores a temporal ledger entry and its current-state projection from
    /// a trusted persistence adapter.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn restore_versioned(
        id: ClaimId,
        owner: ClaimOwner,
        statement: String,
        support: Vec<EvidenceCitation>,
        uncertainty: Option<Uncertainty>,
        applicable_time: ApplicableTime,
        recorded_at: Timestamp,
        status: ClaimStatus,
        supersedes: Option<ClaimId>,
        superseded_by: Option<ClaimId>,
    ) -> Self {
        Self::new_versioned(
            id,
            owner,
            statement,
            support,
            uncertainty,
            applicable_time,
            recorded_at,
            status,
            supersedes,
            superseded_by,
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

    #[must_use]
    pub const fn status(&self) -> ClaimStatus {
        self.status
    }

    #[must_use]
    pub const fn supersedes(&self) -> Option<ClaimId> {
        self.supersedes
    }

    #[must_use]
    pub const fn superseded_by(&self) -> Option<ClaimId> {
        self.superseded_by
    }

    pub(crate) fn mark_superseded_by(&mut self, successor: ClaimId) {
        self.status = ClaimStatus::Superseded;
        self.superseded_by = Some(successor);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClaimCorrectionReceipt {
    correction_evidence_id: EvidenceId,
    superseded_claim_id: ClaimId,
    replacement_claim_id: ClaimId,
    invalidated_memories: usize,
    rebuilt_memories: usize,
    invalidated_projections: usize,
    reindexed_claims: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedExperienceKind {
    Agreement,
    SubstantiveDisagreement,
    RelationshipChange,
    SharedAchievement,
    AgreementBreach,
    AgreementWithdrawal,
}

impl SharedExperienceKind {
    #[must_use]
    pub const fn requires_person_confirmation(self) -> bool {
        matches!(self, Self::Agreement)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedAgreementCandidateStatus {
    AwaitingCounterpart,
    AwaitingPerson,
    Deferred,
    Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedAgreementDecision {
    Confirm,
    Defer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedAgreementCandidate {
    id: SharedAgreementCandidateId,
    version: u64,
    predecessor_candidate_id: Option<SharedAgreementCandidateId>,
    statement: String,
    scope: Option<String>,
    effective_from: Option<Timestamp>,
    effective_until: Option<Timestamp>,
    end_condition: Option<String>,
    supersedes_agreement_ids: Vec<ClaimId>,
    support: Vec<EvidenceCitation>,
    occurred_at: Timestamp,
    recorded_at: Timestamp,
    status: SharedAgreementCandidateStatus,
    counterpart_assented_at: Option<Timestamp>,
    decided_at: Option<Timestamp>,
    claim_id: Option<ClaimId>,
}

impl SharedAgreementCandidate {
    pub(crate) fn awaiting_person(
        id: SharedAgreementCandidateId,
        agreement: SharedAgreementRevision,
        support: Vec<EvidenceCitation>,
        occurred_at: Timestamp,
        recorded_at: Timestamp,
    ) -> Self {
        Self {
            id,
            version: 1,
            predecessor_candidate_id: None,
            statement: agreement.statement,
            scope: Some(agreement.scope),
            effective_from: Some(agreement.effective_from),
            effective_until: agreement.effective_until,
            end_condition: agreement.end_condition,
            supersedes_agreement_ids: agreement.supersedes_agreement_ids,
            support,
            occurred_at,
            recorded_at,
            status: SharedAgreementCandidateStatus::AwaitingPerson,
            counterpart_assented_at: Some(recorded_at),
            decided_at: None,
            claim_id: None,
        }
    }

    pub(crate) fn awaiting_counterpart(
        id: SharedAgreementCandidateId,
        version: u64,
        predecessor_candidate_id: SharedAgreementCandidateId,
        revision: SharedAgreementRevision,
        support: Vec<EvidenceCitation>,
        occurred_at: Timestamp,
        recorded_at: Timestamp,
    ) -> Self {
        Self {
            id,
            version,
            predecessor_candidate_id: Some(predecessor_candidate_id),
            statement: revision.statement,
            scope: Some(revision.scope),
            effective_from: Some(revision.effective_from),
            effective_until: revision.effective_until,
            end_condition: revision.end_condition,
            supersedes_agreement_ids: revision.supersedes_agreement_ids,
            support,
            occurred_at,
            recorded_at,
            status: SharedAgreementCandidateStatus::AwaitingCounterpart,
            counterpart_assented_at: None,
            decided_at: None,
            claim_id: None,
        }
    }

    /// Restores a candidate and its admission state from trusted persistence.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: SharedAgreementCandidateId,
        version: u64,
        predecessor_candidate_id: Option<SharedAgreementCandidateId>,
        statement: String,
        scope: Option<String>,
        effective_from: Option<Timestamp>,
        effective_until: Option<Timestamp>,
        end_condition: Option<String>,
        supersedes_agreement_ids: Vec<ClaimId>,
        support: Vec<EvidenceCitation>,
        occurred_at: Timestamp,
        recorded_at: Timestamp,
        status: SharedAgreementCandidateStatus,
        counterpart_assented_at: Option<Timestamp>,
        decided_at: Option<Timestamp>,
        claim_id: Option<ClaimId>,
    ) -> Self {
        Self {
            id,
            version,
            predecessor_candidate_id,
            statement,
            scope,
            effective_from,
            effective_until,
            end_condition,
            supersedes_agreement_ids,
            support,
            occurred_at,
            recorded_at,
            status,
            counterpart_assented_at,
            decided_at,
            claim_id,
        }
    }

    #[must_use]
    pub const fn id(&self) -> SharedAgreementCandidateId {
        self.id
    }

    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    #[must_use]
    pub const fn predecessor_candidate_id(&self) -> Option<SharedAgreementCandidateId> {
        self.predecessor_candidate_id
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    #[must_use]
    pub const fn effective_from(&self) -> Option<Timestamp> {
        self.effective_from
    }

    #[must_use]
    pub const fn effective_until(&self) -> Option<Timestamp> {
        self.effective_until
    }

    #[must_use]
    pub fn end_condition(&self) -> Option<&str> {
        self.end_condition.as_deref()
    }

    #[must_use]
    pub fn supersedes_agreement_ids(&self) -> &[ClaimId] {
        &self.supersedes_agreement_ids
    }

    #[must_use]
    pub fn support(&self) -> &[EvidenceCitation] {
        &self.support
    }

    #[must_use]
    pub const fn occurred_at(&self) -> Timestamp {
        self.occurred_at
    }

    #[must_use]
    pub const fn recorded_at(&self) -> Timestamp {
        self.recorded_at
    }

    #[must_use]
    pub const fn status(&self) -> SharedAgreementCandidateStatus {
        self.status
    }

    #[must_use]
    pub const fn counterpart_assented_at(&self) -> Option<Timestamp> {
        self.counterpart_assented_at
    }

    #[must_use]
    pub const fn decided_at(&self) -> Option<Timestamp> {
        self.decided_at
    }

    #[must_use]
    pub const fn claim_id(&self) -> Option<ClaimId> {
        self.claim_id
    }

    pub(crate) fn resolve(
        &mut self,
        status: SharedAgreementCandidateStatus,
        decided_at: Timestamp,
        claim_id: Option<ClaimId>,
    ) {
        self.status = status;
        self.decided_at = Some(decided_at);
        self.claim_id = claim_id;
    }

    pub(crate) fn accept_counterpart_assent(
        &mut self,
        citation: EvidenceCitation,
        assented_at: Timestamp,
    ) {
        self.support.push(citation);
        self.status = SharedAgreementCandidateStatus::AwaitingPerson;
        self.counterpart_assented_at = Some(assented_at);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedAgreementRevision {
    statement: String,
    scope: String,
    effective_from: Timestamp,
    effective_until: Option<Timestamp>,
    end_condition: Option<String>,
    supersedes_agreement_ids: Vec<ClaimId>,
}

impl SharedAgreementRevision {
    #[must_use]
    pub fn new(
        statement: impl Into<String>,
        scope: impl Into<String>,
        effective_from: Timestamp,
        effective_until: Option<Timestamp>,
        end_condition: Option<String>,
    ) -> Self {
        Self {
            statement: statement.into(),
            scope: scope.into(),
            effective_from,
            effective_until,
            end_condition,
            supersedes_agreement_ids: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_superseded_agreements(mut self, agreement_claim_ids: Vec<ClaimId>) -> Self {
        self.supersedes_agreement_ids = agreement_claim_ids;
        self
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    #[must_use]
    pub const fn effective_from(&self) -> Timestamp {
        self.effective_from
    }

    #[must_use]
    pub const fn effective_until(&self) -> Option<Timestamp> {
        self.effective_until
    }

    #[must_use]
    pub fn end_condition(&self) -> Option<&str> {
        self.end_condition.as_deref()
    }

    #[must_use]
    pub fn supersedes_agreement_ids(&self) -> &[ClaimId] {
        &self.supersedes_agreement_ids
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.statement.trim().is_empty()
            && !self.scope.trim().is_empty()
            && self
                .effective_until
                .is_none_or(|until| until.as_millis() >= self.effective_from.as_millis())
            && self
                .end_condition
                .as_deref()
                .is_none_or(|condition| !condition.trim().is_empty())
            && self
                .supersedes_agreement_ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.supersedes_agreement_ids.len()
    }

    #[must_use]
    pub fn canonical_text(&self) -> String {
        format!(
            "约定：{}\n范围：{}\n生效时间：{}\n终止时间：{}\n终止条件：{}\n整份取代约定 Claim：{}",
            self.statement,
            self.scope,
            self.effective_from.as_millis(),
            self.effective_until
                .map_or_else(|| "无".to_owned(), |value| value.as_millis().to_string()),
            self.end_condition.as_deref().unwrap_or("无"),
            if self.supersedes_agreement_ids.is_empty() {
                "无".to_owned()
            } else {
                self.supersedes_agreement_ids
                    .iter()
                    .map(|id| id.get().to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            }
        )
    }
}

/// Conservatively detects a direct contradiction between one proposed
/// agreement revision and one already-signed agreement.
///
/// This detector deliberately requires overlapping validity, overlapping
/// scope, shared obligation terms, and opposite explicit negation polarity.
/// It does not subtract natural-language ranges or infer residual duties.
#[must_use]
pub fn shared_agreements_conflict(
    proposed: &SharedAgreementRevision,
    existing: &SharedAgreementCandidate,
) -> bool {
    let Some(existing_scope) = existing.scope() else {
        return false;
    };
    let Some(existing_from) = existing.effective_from() else {
        return false;
    };
    if !validity_intervals_overlap(
        proposed.effective_from(),
        proposed.effective_until(),
        existing_from,
        existing.effective_until(),
    ) || !agreement_scopes_overlap(proposed.scope(), existing_scope)
    {
        return false;
    }

    contains_explicit_negation(proposed.statement())
        != contains_explicit_negation(existing.statement())
        && agreement_texts_overlap(proposed.statement(), existing.statement())
}

/// Returns whether one confirmed agreement still contributes future
/// relational constraints at the supplied instant.
#[must_use]
pub fn agreement_is_active_at(
    agreement_claim_id: ClaimId,
    candidates: &[SharedAgreementCandidate],
    experiences: &[SharedExperience],
    at: Timestamp,
) -> bool {
    let agreement_exists = experiences.iter().any(|experience| {
        experience.kind() == SharedExperienceKind::Agreement
            && experience.claim().id() == agreement_claim_id
            && experience.claim().status() == ClaimStatus::Current
    });
    let Some(original) = candidates.iter().find(|candidate| {
        candidate.status() == SharedAgreementCandidateStatus::Confirmed
            && candidate.claim_id() == Some(agreement_claim_id)
    }) else {
        return false;
    };
    if !agreement_exists
        || original
            .effective_from()
            .is_none_or(|from| at.as_millis() < from.as_millis())
        || original
            .effective_until()
            .is_some_and(|until| at.as_millis() > until.as_millis())
    {
        return false;
    }
    let superseded = candidates.iter().any(|candidate| {
        candidate.status() == SharedAgreementCandidateStatus::Confirmed
            && candidate.claim_id().is_some_and(|claim_id| {
                experiences.iter().any(|experience| {
                    experience.kind() == SharedExperienceKind::Agreement
                        && experience.claim().id() == claim_id
                        && experience.claim().status() == ClaimStatus::Current
                })
            })
            && candidate
                .effective_from()
                .is_some_and(|from| from.as_millis() <= at.as_millis())
            && candidate
                .supersedes_agreement_ids()
                .contains(&agreement_claim_id)
    });
    let withdrawn = experiences.iter().any(|experience| {
        experience.agreement_withdrawal().is_some_and(|withdrawal| {
            withdrawal.agreement_claim_id() == agreement_claim_id
                && withdrawal.effective_at().as_millis() <= at.as_millis()
                && experience.claim().status() == ClaimStatus::Current
        })
    });
    !superseded && !withdrawn
}

fn agreement_scopes_overlap(left: &str, right: &str) -> bool {
    agreement_texts_overlap(
        &left.replace("双方", " ").replace("共同", " "),
        &right.replace("双方", " ").replace("共同", " "),
    )
}

fn validity_intervals_overlap(
    left_from: Timestamp,
    left_until: Option<Timestamp>,
    right_from: Timestamp,
    right_until: Option<Timestamp>,
) -> bool {
    left_until.is_none_or(|until| until.as_millis() >= right_from.as_millis())
        && right_until.is_none_or(|until| until.as_millis() >= left_from.as_millis())
}

fn agreement_texts_overlap(left: &str, right: &str) -> bool {
    let left = agreement_terms(left);
    let right = agreement_terms(right);
    left.iter()
        .filter(|term| right.contains(*term))
        .take(2)
        .count()
        == 2
}

fn agreement_terms(value: &str) -> std::collections::BTreeSet<String> {
    let mut terms = std::collections::BTreeSet::new();
    let mut run = String::new();
    let flush = |run: &mut String, terms: &mut std::collections::BTreeSet<String>| {
        if run.is_empty() {
            return;
        }
        if run.is_ascii() {
            if run.chars().count() >= 2 && !is_negation_term(run) {
                terms.insert(run.clone());
            }
        } else {
            let characters = run.chars().collect::<Vec<_>>();
            terms.extend(
                characters
                    .windows(2)
                    .map(|pair| pair.iter().collect::<String>())
                    .filter(|term| !is_negation_term(term)),
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
    terms
}

fn contains_explicit_negation(value: &str) -> bool {
    let lowercase = value.to_lowercase();
    ["不要", "不得", "不能", "不再", "禁止", "避免", "停止"]
        .iter()
        .any(|marker| lowercase.contains(marker))
        || lowercase.contains("do not")
        || lowercase.contains("must not")
        || lowercase
            .split(|character: char| !character.is_alphanumeric())
            .any(|term| matches!(term, "not" | "never"))
}

fn is_negation_term(value: &str) -> bool {
    matches!(
        value,
        "不要" | "不得" | "不能" | "不再" | "禁止" | "避免" | "停止" | "not" | "never"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedAgreementResolution {
    candidate_id: SharedAgreementCandidateId,
    status: SharedAgreementCandidateStatus,
    claim_id: Option<ClaimId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationalConstraintPriority {
    BelowConstitutionSafetyAndActionAuthorization,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveRelationalConstraint {
    agreement_claim_id: ClaimId,
    statement: String,
    scope: String,
    effective_from: Timestamp,
    effective_until: Option<Timestamp>,
}

impl ActiveRelationalConstraint {
    /// Constructs a constraint projected from one already-confirmed agreement.
    ///
    /// # Errors
    ///
    /// Returns [`RelationalConstraintError`] when the immutable agreement
    /// boundaries cannot form a usable runtime constraint.
    pub fn new(
        agreement_claim_id: ClaimId,
        statement: impl Into<String>,
        scope: impl Into<String>,
        effective_from: Timestamp,
        effective_until: Option<Timestamp>,
    ) -> Result<Self, RelationalConstraintError> {
        let statement = statement.into();
        let scope = scope.into();
        if statement.trim().is_empty() || scope.trim().is_empty() {
            return Err(RelationalConstraintError::EmptyBoundary);
        }
        if effective_until.is_some_and(|until| until.as_millis() < effective_from.as_millis()) {
            return Err(RelationalConstraintError::InvalidValidity);
        }
        Ok(Self {
            agreement_claim_id,
            statement,
            scope,
            effective_from,
            effective_until,
        })
    }

    #[must_use]
    pub const fn agreement_claim_id(&self) -> ClaimId {
        self.agreement_claim_id
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    #[must_use]
    pub const fn effective_from(&self) -> Timestamp {
        self.effective_from
    }

    #[must_use]
    pub const fn effective_until(&self) -> Option<Timestamp> {
        self.effective_until
    }

    #[must_use]
    pub const fn priority(&self) -> RelationalConstraintPriority {
        RelationalConstraintPriority::BelowConstitutionSafetyAndActionAuthorization
    }

    #[must_use]
    pub const fn is_active_at(&self, at: Timestamp) -> bool {
        at.as_millis() >= self.effective_from.as_millis()
            && match self.effective_until {
                Some(until) => at.as_millis() <= until.as_millis(),
                None => true,
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationalConstraintError {
    EmptyBoundary,
    InvalidValidity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationalConstraintDeparture {
    agreement_claim_id: ClaimId,
    reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgreementWithdrawalActor {
    Person,
    Counterpart,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgreementWithdrawal {
    id: ClaimId,
    agreement_claim_id: ClaimId,
    actor: AgreementWithdrawalActor,
    effective_at: Timestamp,
    reason: Option<String>,
    evidence_refs: Vec<EvidenceCitation>,
}

impl AgreementWithdrawal {
    pub(crate) fn recorded(
        id: ClaimId,
        agreement_claim_id: ClaimId,
        actor: AgreementWithdrawalActor,
        effective_at: Timestamp,
        reason: Option<String>,
        evidence_refs: Vec<EvidenceCitation>,
    ) -> Self {
        Self {
            id,
            agreement_claim_id,
            actor,
            effective_at,
            reason,
            evidence_refs,
        }
    }

    /// Restores a withdrawal from trusted persistence.
    #[must_use]
    pub fn restore(
        id: ClaimId,
        agreement_claim_id: ClaimId,
        actor: AgreementWithdrawalActor,
        effective_at: Timestamp,
        reason: Option<String>,
        evidence_refs: Vec<EvidenceCitation>,
    ) -> Self {
        Self::recorded(
            id,
            agreement_claim_id,
            actor,
            effective_at,
            reason,
            evidence_refs,
        )
    }

    #[must_use]
    pub const fn id(&self) -> ClaimId {
        self.id
    }

    #[must_use]
    pub const fn agreement_claim_id(&self) -> ClaimId {
        self.agreement_claim_id
    }

    #[must_use]
    pub const fn actor(&self) -> AgreementWithdrawalActor {
        self.actor
    }

    #[must_use]
    pub const fn effective_at(&self) -> Timestamp {
        self.effective_at
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    #[must_use]
    pub fn evidence_refs(&self) -> &[EvidenceCitation] {
        &self.evidence_refs
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgreementWithdrawalProposal {
    agreement_claim_id: ClaimId,
    reason: String,
}

impl AgreementWithdrawalProposal {
    #[must_use]
    pub fn new(agreement_claim_id: ClaimId, reason: impl Into<String>) -> Self {
        Self {
            agreement_claim_id,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub const fn agreement_claim_id(&self) -> ClaimId {
        self.agreement_claim_id
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl RelationalConstraintDeparture {
    #[must_use]
    pub fn new(agreement_claim_id: ClaimId, reason: impl Into<String>) -> Self {
        Self {
            agreement_claim_id,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub const fn agreement_claim_id(&self) -> ClaimId {
        self.agreement_claim_id
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl SharedAgreementResolution {
    #[must_use]
    pub const fn new(
        candidate_id: SharedAgreementCandidateId,
        status: SharedAgreementCandidateStatus,
        claim_id: Option<ClaimId>,
    ) -> Self {
        Self {
            candidate_id,
            status,
            claim_id,
        }
    }

    #[must_use]
    pub const fn candidate_id(&self) -> SharedAgreementCandidateId {
        self.candidate_id
    }

    #[must_use]
    pub const fn status(&self) -> SharedAgreementCandidateStatus {
        self.status
    }

    #[must_use]
    pub const fn claim_id(&self) -> Option<ClaimId> {
        self.claim_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedExperience {
    kind: SharedExperienceKind,
    claim: Claim,
    ceremony_dismissed: bool,
    constraint_departure: Option<RelationalConstraintDeparture>,
    agreement_withdrawal: Option<AgreementWithdrawal>,
}

impl SharedExperience {
    pub(crate) const fn admitted(kind: SharedExperienceKind, claim: Claim) -> Self {
        Self {
            kind,
            claim,
            ceremony_dismissed: false,
            constraint_departure: None,
            agreement_withdrawal: None,
        }
    }

    pub(crate) const fn agreement_breach(
        claim: Claim,
        departure: RelationalConstraintDeparture,
    ) -> Self {
        Self {
            kind: SharedExperienceKind::AgreementBreach,
            claim,
            ceremony_dismissed: false,
            constraint_departure: Some(departure),
            agreement_withdrawal: None,
        }
    }

    pub(crate) const fn admitted_agreement_withdrawal(
        claim: Claim,
        withdrawal: AgreementWithdrawal,
    ) -> Self {
        Self {
            kind: SharedExperienceKind::AgreementWithdrawal,
            claim,
            ceremony_dismissed: false,
            constraint_departure: None,
            agreement_withdrawal: Some(withdrawal),
        }
    }

    /// Restores an admitted shared experience from trusted persistence.
    #[must_use]
    pub const fn restore(
        kind: SharedExperienceKind,
        claim: Claim,
        ceremony_dismissed: bool,
    ) -> Self {
        Self {
            kind,
            claim,
            ceremony_dismissed,
            constraint_departure: None,
            agreement_withdrawal: None,
        }
    }

    /// Restores a constraint departure from trusted persistence.
    #[must_use]
    pub const fn restore_agreement_breach(
        claim: Claim,
        ceremony_dismissed: bool,
        departure: RelationalConstraintDeparture,
    ) -> Self {
        Self {
            kind: SharedExperienceKind::AgreementBreach,
            claim,
            ceremony_dismissed,
            constraint_departure: Some(departure),
            agreement_withdrawal: None,
        }
    }

    /// Restores an agreement withdrawal from trusted persistence.
    #[must_use]
    pub const fn restore_agreement_withdrawal(
        claim: Claim,
        ceremony_dismissed: bool,
        withdrawal: AgreementWithdrawal,
    ) -> Self {
        Self {
            kind: SharedExperienceKind::AgreementWithdrawal,
            claim,
            ceremony_dismissed,
            constraint_departure: None,
            agreement_withdrawal: Some(withdrawal),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> SharedExperienceKind {
        self.kind
    }

    #[must_use]
    pub const fn claim(&self) -> &Claim {
        &self.claim
    }

    #[must_use]
    pub const fn ceremony_dismissed(&self) -> bool {
        self.ceremony_dismissed
    }

    #[must_use]
    pub const fn constraint_departure(&self) -> Option<&RelationalConstraintDeparture> {
        self.constraint_departure.as_ref()
    }

    #[must_use]
    pub const fn agreement_withdrawal(&self) -> Option<&AgreementWithdrawal> {
        self.agreement_withdrawal.as_ref()
    }

    pub(crate) fn dismiss_ceremony(&mut self) {
        self.ceremony_dismissed = true;
    }
}

impl ClaimCorrectionReceipt {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        correction_evidence_id: EvidenceId,
        superseded_claim_id: ClaimId,
        replacement_claim_id: ClaimId,
        invalidated_memories: usize,
        rebuilt_memories: usize,
        invalidated_projections: usize,
        reindexed_claims: usize,
    ) -> Self {
        Self {
            correction_evidence_id,
            superseded_claim_id,
            replacement_claim_id,
            invalidated_memories,
            rebuilt_memories,
            invalidated_projections,
            reindexed_claims,
        }
    }

    #[must_use]
    pub const fn correction_evidence_id(self) -> EvidenceId {
        self.correction_evidence_id
    }

    #[must_use]
    pub const fn superseded_claim_id(self) -> ClaimId {
        self.superseded_claim_id
    }

    #[must_use]
    pub const fn replacement_claim_id(self) -> ClaimId {
        self.replacement_claim_id
    }

    #[must_use]
    pub const fn invalidated_memories(self) -> usize {
        self.invalidated_memories
    }

    #[must_use]
    pub const fn rebuilt_memories(self) -> usize {
        self.rebuilt_memories
    }

    #[must_use]
    pub const fn invalidated_projections(self) -> usize {
        self.invalidated_projections
    }

    #[must_use]
    pub const fn reindexed_claims(self) -> usize {
        self.reindexed_claims
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DecisionImpact {
    #[default]
    Ordinary,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkingContext {
    evidence: Vec<ConversationEvidence>,
    retrieved: Vec<RetrievedContextItem>,
    retrieval_snapshot: Option<RetrievalSnapshot>,
    active_relational_constraints: Vec<ActiveRelationalConstraint>,
    decision_impact: DecisionImpact,
    frozen_at: Timestamp,
}

impl WorkingContext {
    pub(crate) fn new(evidence: Vec<ConversationEvidence>, frozen_at: Timestamp) -> Self {
        Self {
            evidence,
            retrieved: Vec::new(),
            retrieval_snapshot: None,
            active_relational_constraints: Vec::new(),
            decision_impact: DecisionImpact::Ordinary,
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

    /// Attaches the task-relevant agreement constraints selected by trusted
    /// retrieval. Every constraint must be active at this snapshot and unique.
    ///
    /// # Errors
    ///
    /// Returns [`WorkingContextError`] for an expired/future or duplicate
    /// agreement projection.
    pub fn with_active_relational_constraints(
        mut self,
        constraints: Vec<ActiveRelationalConstraint>,
    ) -> Result<Self, WorkingContextError> {
        let mut seen = std::collections::BTreeSet::new();
        for constraint in &constraints {
            if !constraint.is_active_at(self.frozen_at) {
                return Err(WorkingContextError::InactiveRelationalConstraint(
                    constraint.agreement_claim_id(),
                ));
            }
            if !seen.insert(constraint.agreement_claim_id()) {
                return Err(WorkingContextError::DuplicateRelationalConstraint(
                    constraint.agreement_claim_id(),
                ));
            }
        }
        self.active_relational_constraints = constraints;
        Ok(self)
    }

    #[must_use]
    pub const fn with_decision_impact(mut self, impact: DecisionImpact) -> Self {
        self.decision_impact = impact;
        self
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
    pub fn active_relational_constraints(&self) -> &[ActiveRelationalConstraint] {
        &self.active_relational_constraints
    }

    #[must_use]
    pub const fn decision_impact(&self) -> DecisionImpact {
        self.decision_impact
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisputeState {
    Open,
    Maintained,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenMemoryDispute {
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
    estimated_tokens: usize,
}

impl FrozenMemoryDispute {
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
        estimated_tokens: usize,
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
            estimated_tokens,
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

    #[must_use]
    pub const fn estimated_tokens(&self) -> usize {
        self.estimated_tokens
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetrievedContextItem {
    EvidenceWindow(FrozenRetrievalWindow),
    LedgerClaim(FrozenLedgerClaim),
    MemoryDispute(FrozenMemoryDispute),
}

impl RetrievedContextItem {
    #[must_use]
    pub const fn estimated_tokens(&self) -> usize {
        match self {
            Self::EvidenceWindow(window) => window.estimated_tokens(),
            Self::LedgerClaim(claim) => claim.estimated_tokens(),
            Self::MemoryDispute(dispute) => dispute.estimated_tokens(),
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
    InactiveRelationalConstraint(ClaimId),
    DuplicateRelationalConstraint(ClaimId),
}

impl fmt::Display for WorkingContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BudgetExceeded => "retrieved working context exceeds its frozen token budget",
            Self::TokenAccountingMismatch => {
                "retrieved working context token accounting does not match its frozen snapshot"
            }
            Self::EmptyEvidenceWindow => "retrieved working context contains an empty window",
            Self::InactiveRelationalConstraint(_) => {
                "working context contains a relational constraint outside its validity interval"
            }
            Self::DuplicateRelationalConstraint(_) => {
                "working context contains the same relational constraint more than once"
            }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedExperienceProposal {
    kind: SharedExperienceKind,
    statement: String,
    person_support: Vec<EvidenceCitation>,
    counterpart_quote: String,
    occurred_at: Timestamp,
    agreement_scope: Option<String>,
    agreement_effective_from: Option<Timestamp>,
    agreement_effective_until: Option<Timestamp>,
    agreement_end_condition: Option<String>,
    supersedes_agreement_ids: Vec<ClaimId>,
}

impl SharedExperienceProposal {
    #[must_use]
    pub fn new(
        kind: SharedExperienceKind,
        statement: impl Into<String>,
        person_support: Vec<EvidenceCitation>,
        counterpart_quote: impl Into<String>,
        occurred_at: Timestamp,
    ) -> Self {
        Self {
            kind,
            statement: statement.into(),
            person_support,
            counterpart_quote: counterpart_quote.into(),
            occurred_at,
            agreement_scope: None,
            agreement_effective_from: None,
            agreement_effective_until: None,
            agreement_end_condition: None,
            supersedes_agreement_ids: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_agreement_terms(
        mut self,
        scope: impl Into<String>,
        effective_from: Timestamp,
        effective_until: Option<Timestamp>,
        end_condition: Option<String>,
    ) -> Self {
        self.agreement_scope = Some(scope.into());
        self.agreement_effective_from = Some(effective_from);
        self.agreement_effective_until = effective_until;
        self.agreement_end_condition = end_condition;
        self
    }

    #[must_use]
    pub fn with_agreement_scope(mut self, scope: impl Into<String>) -> Self {
        self.agreement_scope = Some(scope.into());
        self
    }

    #[must_use]
    pub fn with_superseded_agreements(mut self, agreement_claim_ids: Vec<ClaimId>) -> Self {
        self.supersedes_agreement_ids = agreement_claim_ids;
        self
    }

    #[must_use]
    pub const fn kind(&self) -> SharedExperienceKind {
        self.kind
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub fn person_support(&self) -> &[EvidenceCitation] {
        &self.person_support
    }

    #[must_use]
    pub fn counterpart_quote(&self) -> &str {
        &self.counterpart_quote
    }

    #[must_use]
    pub const fn occurred_at(&self) -> Timestamp {
        self.occurred_at
    }

    #[must_use]
    pub fn agreement_scope(&self) -> Option<&str> {
        self.agreement_scope.as_deref()
    }

    #[must_use]
    pub const fn agreement_effective_from(&self) -> Option<Timestamp> {
        self.agreement_effective_from
    }

    #[must_use]
    pub const fn agreement_effective_until(&self) -> Option<Timestamp> {
        self.agreement_effective_until
    }

    #[must_use]
    pub fn agreement_end_condition(&self) -> Option<&str> {
        self.agreement_end_condition.as_deref()
    }

    #[must_use]
    pub fn supersedes_agreement_ids(&self) -> &[ClaimId] {
        &self.supersedes_agreement_ids
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedAgreementAssent {
    candidate_id: SharedAgreementCandidateId,
    version: u64,
    counterpart_quote: String,
}

impl SharedAgreementAssent {
    #[must_use]
    pub fn new(
        candidate_id: SharedAgreementCandidateId,
        version: u64,
        counterpart_quote: impl Into<String>,
    ) -> Self {
        Self {
            candidate_id,
            version,
            counterpart_quote: counterpart_quote.into(),
        }
    }

    #[must_use]
    pub const fn candidate_id(&self) -> SharedAgreementCandidateId {
        self.candidate_id
    }

    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    #[must_use]
    pub fn counterpart_quote(&self) -> &str {
        &self.counterpart_quote
    }
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
    shared_experience_proposals: Vec<SharedExperienceProposal>,
    shared_agreement_assents: Vec<SharedAgreementAssent>,
    relational_constraint_departures: Vec<RelationalConstraintDeparture>,
    agreement_withdrawals: Vec<AgreementWithdrawalProposal>,
    unsupported_operations: Vec<UnsupportedStructuredOperation>,
}

impl RuntimeResponse {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            citations: Vec::new(),
            judgment_proposals: Vec::new(),
            shared_experience_proposals: Vec::new(),
            shared_agreement_assents: Vec::new(),
            relational_constraint_departures: Vec::new(),
            agreement_withdrawals: Vec::new(),
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
    pub fn with_shared_experience(mut self, proposal: SharedExperienceProposal) -> Self {
        self.shared_experience_proposals.push(proposal);
        self
    }

    #[must_use]
    pub fn with_shared_agreement_assent(mut self, assent: SharedAgreementAssent) -> Self {
        self.shared_agreement_assents.push(assent);
        self
    }

    #[must_use]
    pub fn with_relational_constraint_departure(
        mut self,
        departure: RelationalConstraintDeparture,
    ) -> Self {
        self.relational_constraint_departures.push(departure);
        self
    }

    #[must_use]
    pub fn with_agreement_withdrawal(mut self, withdrawal: AgreementWithdrawalProposal) -> Self {
        self.agreement_withdrawals.push(withdrawal);
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
    pub fn shared_experience_proposals(&self) -> &[SharedExperienceProposal] {
        &self.shared_experience_proposals
    }

    #[must_use]
    pub fn shared_agreement_assents(&self) -> &[SharedAgreementAssent] {
        &self.shared_agreement_assents
    }

    #[must_use]
    pub fn relational_constraint_departures(&self) -> &[RelationalConstraintDeparture] {
        &self.relational_constraint_departures
    }

    #[must_use]
    pub fn agreement_withdrawals(&self) -> &[AgreementWithdrawalProposal] {
        &self.agreement_withdrawals
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
    pending_agreement_candidates: Vec<SharedAgreementCandidate>,
}

impl RuntimeRequest {
    pub(crate) fn new(
        prompt: ConversationEvidence,
        working_context: WorkingContext,
        pending_agreement_candidates: Vec<SharedAgreementCandidate>,
    ) -> Self {
        Self {
            prompt,
            working_context,
            pending_agreement_candidates,
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

    #[must_use]
    pub fn pending_agreement_candidates(&self) -> &[SharedAgreementCandidate] {
        &self.pending_agreement_candidates
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedExperienceRejectionReason {
    EmptyStatement,
    AgreementBreachRequiresConstraintDeparture,
    MissingPersonSupport,
    EvidenceOutsideWorkingContext(EvidenceId),
    EvidenceNotFromPerson(EvidenceId),
    EmptyPersonQuote(EvidenceId),
    PersonQuoteMismatch(EvidenceId),
    EmptyCounterpartQuote,
    CounterpartQuoteMismatch,
    MissingAgreementScope,
    MissingAgreementEffectiveFrom,
    InvalidAgreementValidity,
    ConflictingAgreementsRequireExplicitSupersession(Vec<ClaimId>),
    SupersededAgreementNotActive(ClaimId),
    UnexpectedAgreementTerms,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedAgreementAssentRejectionReason {
    CandidateNotFound(SharedAgreementCandidateId),
    CandidateNotAwaitingCounterpart(SharedAgreementCandidateId),
    VersionMismatch {
        candidate_id: SharedAgreementCandidateId,
        expected: u64,
        actual: u64,
    },
    EmptyCounterpartQuote,
    CounterpartQuoteMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedAgreementAssentRejection {
    proposal_index: usize,
    reason: SharedAgreementAssentRejectionReason,
}

impl SharedAgreementAssentRejection {
    pub(crate) const fn new(
        proposal_index: usize,
        reason: SharedAgreementAssentRejectionReason,
    ) -> Self {
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
    pub const fn reason(&self) -> &SharedAgreementAssentRejectionReason {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedExperienceRejection {
    proposal_index: usize,
    reason: SharedExperienceRejectionReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelationalConstraintDepartureRejectionReason {
    ConstraintNotActive(ClaimId),
    AgreementNotFound(ClaimId),
    EmptyReason,
    ReasonNotInResponse,
    DuplicateDeparture(ClaimId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationalConstraintDepartureRejection {
    proposal_index: usize,
    reason: RelationalConstraintDepartureRejectionReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgreementWithdrawalRejectionReason {
    ConstraintNotActive(ClaimId),
    AgreementNotFound(ClaimId),
    EmptyReason,
    ReasonNotInResponse,
    DuplicateWithdrawal(ClaimId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgreementWithdrawalRejection {
    proposal_index: usize,
    reason: AgreementWithdrawalRejectionReason,
}

impl AgreementWithdrawalRejection {
    pub(crate) const fn new(
        proposal_index: usize,
        reason: AgreementWithdrawalRejectionReason,
    ) -> Self {
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
    pub const fn reason(&self) -> &AgreementWithdrawalRejectionReason {
        &self.reason
    }
}

impl RelationalConstraintDepartureRejection {
    pub(crate) const fn new(
        proposal_index: usize,
        reason: RelationalConstraintDepartureRejectionReason,
    ) -> Self {
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
    pub const fn reason(&self) -> &RelationalConstraintDepartureRejectionReason {
        &self.reason
    }
}

impl SharedExperienceRejection {
    pub(crate) const fn new(
        proposal_index: usize,
        reason: SharedExperienceRejectionReason,
    ) -> Self {
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
    pub const fn reason(&self) -> &SharedExperienceRejectionReason {
        &self.reason
    }
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
    pending_agreement_candidate_ids: Vec<SharedAgreementCandidateId>,
    admitted_shared_experience_ids: Vec<ClaimId>,
    rejected_shared_experiences: Vec<SharedExperienceRejection>,
    assented_agreement_candidate_ids: Vec<SharedAgreementCandidateId>,
    rejected_agreement_assents: Vec<SharedAgreementAssentRejection>,
    recorded_constraint_departure_ids: Vec<ClaimId>,
    rejected_constraint_departures: Vec<RelationalConstraintDepartureRejection>,
    recorded_agreement_withdrawal_ids: Vec<ClaimId>,
    rejected_agreement_withdrawals: Vec<AgreementWithdrawalRejection>,
    rejected_operations: Vec<StructuredOperationRejection>,
    validated_citations: Vec<EvidenceCitation>,
}

impl TurnOutcome {
    pub(crate) fn new(
        person_evidence_id: EvidenceId,
        counterpart_evidence_id: EvidenceId,
        person_classification: PersonTurnClassification,
        validated_citations: Vec<EvidenceCitation>,
    ) -> Self {
        Self {
            person_evidence_id,
            counterpart_evidence_id,
            person_classification,
            accepted_judgment_ids: Vec::new(),
            rejected_judgments: Vec::new(),
            pending_agreement_candidate_ids: Vec::new(),
            admitted_shared_experience_ids: Vec::new(),
            rejected_shared_experiences: Vec::new(),
            assented_agreement_candidate_ids: Vec::new(),
            rejected_agreement_assents: Vec::new(),
            recorded_constraint_departure_ids: Vec::new(),
            rejected_constraint_departures: Vec::new(),
            recorded_agreement_withdrawal_ids: Vec::new(),
            rejected_agreement_withdrawals: Vec::new(),
            rejected_operations: Vec::new(),
            validated_citations,
        }
    }

    pub(crate) fn with_judgments(
        mut self,
        accepted: Vec<ClaimId>,
        rejected: Vec<JudgmentRejection>,
    ) -> Self {
        self.accepted_judgment_ids = accepted;
        self.rejected_judgments = rejected;
        self
    }

    pub(crate) fn with_shared_experiences(
        mut self,
        pending_agreements: Vec<SharedAgreementCandidateId>,
        admitted: Vec<ClaimId>,
        rejected: Vec<SharedExperienceRejection>,
    ) -> Self {
        self.pending_agreement_candidate_ids = pending_agreements;
        self.admitted_shared_experience_ids = admitted;
        self.rejected_shared_experiences = rejected;
        self
    }

    pub(crate) fn with_agreement_assents(
        mut self,
        assented: Vec<SharedAgreementCandidateId>,
        rejected: Vec<SharedAgreementAssentRejection>,
    ) -> Self {
        self.assented_agreement_candidate_ids = assented;
        self.rejected_agreement_assents = rejected;
        self
    }

    pub(crate) fn with_constraint_departures(
        mut self,
        recorded: Vec<ClaimId>,
        rejected: Vec<RelationalConstraintDepartureRejection>,
    ) -> Self {
        self.recorded_constraint_departure_ids = recorded;
        self.rejected_constraint_departures = rejected;
        self
    }

    pub(crate) fn with_agreement_withdrawals(
        mut self,
        recorded: Vec<ClaimId>,
        rejected: Vec<AgreementWithdrawalRejection>,
    ) -> Self {
        self.recorded_agreement_withdrawal_ids = recorded;
        self.rejected_agreement_withdrawals = rejected;
        self
    }

    pub(crate) fn with_rejected_operations(
        mut self,
        rejected: Vec<StructuredOperationRejection>,
    ) -> Self {
        self.rejected_operations = rejected;
        self
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
    pub fn pending_agreement_candidate_ids(&self) -> &[SharedAgreementCandidateId] {
        &self.pending_agreement_candidate_ids
    }

    #[must_use]
    pub fn admitted_shared_experience_ids(&self) -> &[ClaimId] {
        &self.admitted_shared_experience_ids
    }

    #[must_use]
    pub fn rejected_shared_experiences(&self) -> &[SharedExperienceRejection] {
        &self.rejected_shared_experiences
    }

    #[must_use]
    pub fn assented_agreement_candidate_ids(&self) -> &[SharedAgreementCandidateId] {
        &self.assented_agreement_candidate_ids
    }

    #[must_use]
    pub fn rejected_agreement_assents(&self) -> &[SharedAgreementAssentRejection] {
        &self.rejected_agreement_assents
    }

    #[must_use]
    pub fn recorded_constraint_departure_ids(&self) -> &[ClaimId] {
        &self.recorded_constraint_departure_ids
    }

    #[must_use]
    pub fn rejected_constraint_departures(&self) -> &[RelationalConstraintDepartureRejection] {
        &self.rejected_constraint_departures
    }

    #[must_use]
    pub fn recorded_agreement_withdrawal_ids(&self) -> &[ClaimId] {
        &self.recorded_agreement_withdrawal_ids
    }

    #[must_use]
    pub fn rejected_agreement_withdrawals(&self) -> &[AgreementWithdrawalRejection] {
        &self.rejected_agreement_withdrawals
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

#[cfg(test)]
mod tests {
    use super::*;

    fn confirmed_agreement(scope: &str) -> SharedAgreementCandidate {
        SharedAgreementCandidate::restore(
            SharedAgreementCandidateId::from_raw(1),
            1,
            None,
            "项目复盘时直接提醒休息".to_owned(),
            Some(scope.to_owned()),
            Some(Timestamp::from_millis(1_000)),
            None,
            None,
            Vec::new(),
            Vec::new(),
            Timestamp::from_millis(1_000),
            Timestamp::from_millis(1_000),
            SharedAgreementCandidateStatus::Confirmed,
            Some(Timestamp::from_millis(1_000)),
            Some(Timestamp::from_millis(1_000)),
            Some(ClaimId::from_raw(1)),
        )
    }

    #[test]
    fn conflict_detection_ignores_generic_relational_scope_words() {
        let existing = confirmed_agreement("双方共同项目复盘");
        let unrelated = SharedAgreementRevision::new(
            "健康管理时不要直接提醒休息",
            "双方共同健康管理",
            Timestamp::from_millis(2_000),
            None,
            None,
        );
        let same_scope = SharedAgreementRevision::new(
            "项目复盘时不要直接提醒休息",
            "双方共同项目复盘",
            Timestamp::from_millis(2_000),
            None,
            None,
        );

        assert!(!shared_agreements_conflict(&unrelated, &existing));
        assert!(shared_agreements_conflict(&same_scope, &existing));
    }
}
