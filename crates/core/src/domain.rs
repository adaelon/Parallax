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
pub struct ReflectionInvitationId(u64);

impl ReflectionInvitationId {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterpartReplyAttribution {
    PreIdentityUnbound,
    IdentityBound(u64),
}

impl CounterpartReplyAttribution {
    #[must_use]
    pub const fn identity_version(self) -> Option<u64> {
        match self {
            Self::PreIdentityUnbound => None,
            Self::IdentityBound(version) => Some(version),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationEvidence {
    id: EvidenceId,
    session_id: SessionId,
    speaker: Speaker,
    verbatim: String,
    recorded_at: Timestamp,
    counterpart_reply_attribution: Option<CounterpartReplyAttribution>,
}

impl ConversationEvidence {
    pub(crate) fn new(
        id: EvidenceId,
        session_id: SessionId,
        speaker: Speaker,
        verbatim: String,
        recorded_at: Timestamp,
        counterpart_reply_attribution: Option<CounterpartReplyAttribution>,
    ) -> Self {
        Self {
            id,
            session_id,
            speaker,
            verbatim,
            recorded_at,
            counterpart_reply_attribution,
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
        let counterpart_reply_attribution = match speaker {
            Speaker::Person => None,
            Speaker::Counterpart => Some(CounterpartReplyAttribution::PreIdentityUnbound),
        };
        Self::new(
            id,
            session_id,
            speaker,
            verbatim,
            recorded_at,
            counterpart_reply_attribution,
        )
    }

    /// Restores a counterpart reply together with its persisted identity attribution.
    #[must_use]
    pub fn restore_counterpart(
        id: EvidenceId,
        session_id: SessionId,
        verbatim: String,
        recorded_at: Timestamp,
        attribution: CounterpartReplyAttribution,
    ) -> Self {
        Self::new(
            id,
            session_id,
            Speaker::Counterpart,
            verbatim,
            recorded_at,
            Some(attribution),
        )
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

    #[must_use]
    pub const fn counterpart_reply_attribution(&self) -> Option<CounterpartReplyAttribution> {
        self.counterpart_reply_attribution
    }

    #[must_use]
    pub const fn can_support_counterpart_knowledge(&self) -> bool {
        match self.speaker {
            Speaker::Person => true,
            Speaker::Counterpart => matches!(
                self.counterpart_reply_attribution,
                Some(CounterpartReplyAttribution::IdentityBound(_))
            ),
        }
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

pub const MAX_PERSON_FACT_PROPOSALS_PER_TURN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonFactProposal {
    owner: ClaimOwner,
    statement: String,
    citation: EvidenceCitation,
    applicable_time: ApplicableTime,
}

impl PersonFactProposal {
    #[must_use]
    pub fn new(
        owner: ClaimOwner,
        statement: impl Into<String>,
        citation: EvidenceCitation,
        applicable_time: ApplicableTime,
    ) -> Self {
        Self {
            owner,
            statement: statement.into(),
            citation,
            applicable_time,
        }
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
    pub const fn citation(&self) -> &EvidenceCitation {
        &self.citation
    }

    #[must_use]
    pub const fn applicable_time(&self) -> ApplicableTime {
        self.applicable_time
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersonFactProposalBatchLimitError {
    actual: usize,
    maximum: usize,
}

impl PersonFactProposalBatchLimitError {
    #[must_use]
    pub const fn actual(self) -> usize {
        self.actual
    }

    #[must_use]
    pub const fn maximum(self) -> usize {
        self.maximum
    }
}

impl fmt::Display for PersonFactProposalBatchLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "person fact proposal batch contains {} items; maximum is {}",
            self.actual, self.maximum
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersonFactProposalBatch {
    proposals: Vec<PersonFactProposal>,
}

impl PersonFactProposalBatch {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            proposals: Vec::new(),
        }
    }

    pub fn try_new(
        proposals: impl IntoIterator<Item = PersonFactProposal>,
    ) -> Result<Self, PersonFactProposalBatchLimitError> {
        let proposals = proposals.into_iter().collect::<Vec<_>>();
        if proposals.len() > MAX_PERSON_FACT_PROPOSALS_PER_TURN {
            return Err(PersonFactProposalBatchLimitError {
                actual: proposals.len(),
                maximum: MAX_PERSON_FACT_PROPOSALS_PER_TURN,
            });
        }
        Ok(Self { proposals })
    }

    #[must_use]
    pub fn proposals(&self) -> &[PersonFactProposal] {
        &self.proposals
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.proposals.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.proposals.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonFactProposalRejectionReason {
    OwnerNotPerson(ClaimOwner),
    EmptyStatement,
    EvidenceMismatch(EvidenceId),
    EmptyQuote,
    QuoteMismatch(EvidenceId),
    StatementNotVerbatim,
    InvalidApplicableTime,
    DuplicateFact,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonFactProposalRejection {
    proposal_index: usize,
    reason: PersonFactProposalRejectionReason,
}

impl PersonFactProposalRejection {
    #[must_use]
    pub const fn new(proposal_index: usize, reason: PersonFactProposalRejectionReason) -> Self {
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
    pub const fn reason(&self) -> &PersonFactProposalRejectionReason {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonTurnObservation {
    evidence_id: EvidenceId,
    accepted_person_fact_ids: Vec<ClaimId>,
    rejected_person_fact_proposals: Vec<PersonFactProposalRejection>,
}

impl PersonTurnObservation {
    pub(crate) fn new(
        evidence_id: EvidenceId,
        accepted_person_fact_ids: Vec<ClaimId>,
        rejected_person_fact_proposals: Vec<PersonFactProposalRejection>,
    ) -> Self {
        Self {
            evidence_id,
            accepted_person_fact_ids,
            rejected_person_fact_proposals,
        }
    }

    #[must_use]
    pub const fn evidence_id(&self) -> EvidenceId {
        self.evidence_id
    }

    #[must_use]
    pub fn accepted_person_fact_ids(&self) -> &[ClaimId] {
        &self.accepted_person_fact_ids
    }

    #[must_use]
    pub fn rejected_person_fact_proposals(&self) -> &[PersonFactProposalRejection] {
        &self.rejected_person_fact_proposals
    }
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
    relevant_counterpart_experience_refs: Vec<String>,
    reflection_opportunity: ReflectionOpportunity,
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
            relevant_counterpart_experience_refs: Vec::new(),
            reflection_opportunity: ReflectionOpportunity::UnrelatedTask,
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

    /// Attaches the Self Bundle experience references selected as relevant to
    /// this frozen turn. Core still verifies every reference against the
    /// current persisted Self Bundle before exposing it to a runtime.
    ///
    /// # Errors
    ///
    /// Returns [`WorkingContextError`] for empty or duplicate references.
    pub fn with_relevant_counterpart_experiences(
        mut self,
        references: Vec<String>,
    ) -> Result<Self, WorkingContextError> {
        let mut seen = std::collections::BTreeSet::new();
        for reference in &references {
            if reference.trim().is_empty() {
                return Err(WorkingContextError::EmptyCounterpartExperienceReference);
            }
            if !seen.insert(reference.as_str()) {
                return Err(WorkingContextError::DuplicateCounterpartExperienceReference);
            }
        }
        self.relevant_counterpart_experience_refs = references;
        Ok(self)
    }

    #[must_use]
    pub const fn with_decision_impact(mut self, impact: DecisionImpact) -> Self {
        self.decision_impact = impact;
        self
    }

    #[must_use]
    pub fn with_reflection_opportunity(mut self, opportunity: ReflectionOpportunity) -> Self {
        self.reflection_opportunity = opportunity;
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
    pub fn relevant_counterpart_experience_refs(&self) -> &[String] {
        &self.relevant_counterpart_experience_refs
    }

    #[must_use]
    pub const fn decision_impact(&self) -> DecisionImpact {
        self.decision_impact
    }

    #[must_use]
    pub const fn reflection_opportunity(&self) -> &ReflectionOpportunity {
        &self.reflection_opportunity
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
    EmptyCounterpartExperienceReference,
    DuplicateCounterpartExperienceReference,
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
            Self::EmptyCounterpartExperienceReference => {
                "working context contains an empty counterpart experience reference"
            }
            Self::DuplicateCounterpartExperienceReference => {
                "working context contains the same counterpart experience reference more than once"
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

pub const REFLECTION_SCHEDULE_CONTRACT_VERSION: &str = "g08-reflection-v1";
pub const REFLECTION_DEFER_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
pub const REFLECTION_PROACTIVE_COOLDOWN_MILLIS: i64 = 24 * 60 * 60 * 1_000;
pub const MAX_OPEN_REFLECTION_INVITATIONS: usize = 32;
pub const MAX_REFLECTION_TOPIC_BYTES: usize = 128;
pub const MAX_REFLECTION_OBSERVATION_BYTES: usize = 4_096;
pub const MAX_REFLECTION_WHY_NOW_BYTES: usize = 1_024;
pub const MAX_REFLECTION_EVIDENCE_REFS: usize = 8;
pub const G08_IMMEDIATE_SAFETY_FIXTURE_ID: &str = "g08-immediate-safety-v1";
pub const G08_IMMEDIATE_SAFETY_QUOTE: &str = "我现在正打算伤害自己。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionInvitationBasis {
    ImportantSingleChange,
    RepeatedPattern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReflectionImportance {
    Ordinary,
    Important,
    ImmediateSafetyRisk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionInvitationState {
    Pending,
    Offered,
    Deferred,
    MutedByPerson,
    Resolved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReflectionOpportunity {
    UnrelatedTask,
    RelatedTopic(String),
    ConversationIdle,
    ScheduledReview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionDelivery {
    Queued,
    Offer,
    DiscussOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionDecision {
    Defer,
    Mute,
    Resolve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionTransitionError {
    NotOfferable,
    NotAwaitingDecision,
    DeferCountOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflectionInvitationProposal {
    topic_key: String,
    observation: String,
    evidence_refs: Vec<EvidenceCitation>,
    why_now: String,
    importance: ReflectionImportance,
    basis: ReflectionInvitationBasis,
}

impl ReflectionInvitationProposal {
    #[must_use]
    pub fn new(
        topic_key: impl Into<String>,
        observation: impl Into<String>,
        evidence_refs: Vec<EvidenceCitation>,
        why_now: impl Into<String>,
        importance: ReflectionImportance,
        basis: ReflectionInvitationBasis,
    ) -> Self {
        Self {
            topic_key: topic_key.into(),
            observation: observation.into(),
            evidence_refs,
            why_now: why_now.into(),
            importance,
            basis,
        }
    }

    #[must_use]
    pub fn topic_key(&self) -> &str {
        &self.topic_key
    }
    #[must_use]
    pub fn observation(&self) -> &str {
        &self.observation
    }
    #[must_use]
    pub fn evidence_refs(&self) -> &[EvidenceCitation] {
        &self.evidence_refs
    }
    #[must_use]
    pub fn why_now(&self) -> &str {
        &self.why_now
    }
    #[must_use]
    pub const fn importance(&self) -> ReflectionImportance {
        self.importance
    }
    #[must_use]
    pub const fn basis(&self) -> ReflectionInvitationBasis {
        self.basis
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflectionInvitation {
    id: ReflectionInvitationId,
    topic_key: String,
    observation: String,
    evidence_refs: Vec<EvidenceCitation>,
    why_now: String,
    importance: ReflectionImportance,
    basis: ReflectionInvitationBasis,
    state: ReflectionInvitationState,
    created_at: Timestamp,
    updated_at: Timestamp,
    next_eligible_at: Option<Timestamp>,
    last_offered_at: Option<Timestamp>,
    defer_count: u32,
    mute_prompted: bool,
}

impl ReflectionInvitation {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn restore(
        id: ReflectionInvitationId,
        topic_key: impl Into<String>,
        observation: impl Into<String>,
        evidence_refs: Vec<EvidenceCitation>,
        why_now: impl Into<String>,
        importance: ReflectionImportance,
        basis: ReflectionInvitationBasis,
        state: ReflectionInvitationState,
        created_at: Timestamp,
        updated_at: Timestamp,
        next_eligible_at: Option<Timestamp>,
        last_offered_at: Option<Timestamp>,
        defer_count: u32,
        mute_prompted: bool,
    ) -> Self {
        Self {
            id,
            topic_key: topic_key.into(),
            observation: observation.into(),
            evidence_refs,
            why_now: why_now.into(),
            importance,
            basis,
            state,
            created_at,
            updated_at,
            next_eligible_at,
            last_offered_at,
            defer_count,
            mute_prompted,
        }
    }

    #[must_use]
    pub const fn id(&self) -> ReflectionInvitationId {
        self.id
    }
    #[must_use]
    pub fn topic_key(&self) -> &str {
        &self.topic_key
    }
    #[must_use]
    pub fn observation(&self) -> &str {
        &self.observation
    }
    #[must_use]
    pub fn evidence_refs(&self) -> &[EvidenceCitation] {
        &self.evidence_refs
    }
    #[must_use]
    pub fn why_now(&self) -> &str {
        &self.why_now
    }
    #[must_use]
    pub const fn importance(&self) -> ReflectionImportance {
        self.importance
    }
    #[must_use]
    pub const fn basis(&self) -> ReflectionInvitationBasis {
        self.basis
    }
    #[must_use]
    pub const fn state(&self) -> ReflectionInvitationState {
        self.state
    }
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
    #[must_use]
    pub const fn next_eligible_at(&self) -> Option<Timestamp> {
        self.next_eligible_at
    }
    #[must_use]
    pub const fn last_offered_at(&self) -> Option<Timestamp> {
        self.last_offered_at
    }
    #[must_use]
    pub const fn defer_count(&self) -> u32 {
        self.defer_count
    }
    #[must_use]
    pub const fn mute_prompted(&self) -> bool {
        self.mute_prompted
    }
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.state != ReflectionInvitationState::Resolved
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReflectionInvitationReceipt {
    id: ReflectionInvitationId,
    state: ReflectionInvitationState,
}

impl ReflectionInvitationReceipt {
    #[must_use]
    pub const fn new(id: ReflectionInvitationId, state: ReflectionInvitationState) -> Self {
        Self { id, state }
    }
    #[must_use]
    pub const fn id(self) -> ReflectionInvitationId {
        self.id
    }
    #[must_use]
    pub const fn state(self) -> ReflectionInvitationState {
        self.state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReflectionInvitationRejectionReason {
    DuplicateProposal,
    EmptyTopic,
    TopicTooLong,
    EmptyObservation,
    ObservationTooLong,
    EmptyWhyNow,
    WhyNowTooLong,
    MissingEvidence,
    TooManyEvidenceReferences,
    RepeatedPatternRequiresS27,
    PatternLanguageForSingleChange,
    ImmediateSafetyFixtureMismatch,
    EvidenceOutsideWorkingContext(EvidenceId),
    EmptyQuote(EvidenceId),
    QuoteMismatch(EvidenceId),
    DuplicateOpenTopic,
    OpenInvitationBudgetExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflectionInvitationRejection {
    proposal_index: usize,
    reason: ReflectionInvitationRejectionReason,
}

impl ReflectionInvitationRejection {
    pub(crate) const fn new(
        proposal_index: usize,
        reason: ReflectionInvitationRejectionReason,
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
    pub const fn reason(&self) -> &ReflectionInvitationRejectionReason {
        &self.reason
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectionRuntimeDisposition {
    Offer,
    DiscussOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflectionRuntimeContext {
    invitation: ReflectionInvitation,
    disposition: ReflectionRuntimeDisposition,
}

impl ReflectionRuntimeContext {
    #[must_use]
    pub const fn new(
        invitation: ReflectionInvitation,
        disposition: ReflectionRuntimeDisposition,
    ) -> Self {
        Self {
            invitation,
            disposition,
        }
    }
    #[must_use]
    pub const fn invitation(&self) -> &ReflectionInvitation {
        &self.invitation
    }
    #[must_use]
    pub const fn disposition(&self) -> ReflectionRuntimeDisposition {
        self.disposition
    }
}

#[must_use]
pub fn reflection_delivery(
    invitation: &ReflectionInvitation,
    opportunity: &ReflectionOpportunity,
    now: Timestamp,
    last_proactive_offer_at: Option<Timestamp>,
) -> ReflectionDelivery {
    if matches!(
        invitation.state(),
        ReflectionInvitationState::Resolved | ReflectionInvitationState::Offered
    ) {
        return ReflectionDelivery::Queued;
    }
    if invitation.importance() == ReflectionImportance::ImmediateSafetyRisk {
        return ReflectionDelivery::Offer;
    }
    if invitation.state() == ReflectionInvitationState::MutedByPerson {
        return match opportunity {
            ReflectionOpportunity::RelatedTopic(topic) if topic == invitation.topic_key() => {
                ReflectionDelivery::DiscussOnly
            }
            _ => ReflectionDelivery::Queued,
        };
    }
    if invitation
        .next_eligible_at()
        .is_some_and(|eligible_at| eligible_at.as_millis() > now.as_millis())
    {
        return ReflectionDelivery::Queued;
    }
    match opportunity {
        ReflectionOpportunity::UnrelatedTask => ReflectionDelivery::Queued,
        ReflectionOpportunity::RelatedTopic(topic) => {
            if topic == invitation.topic_key() {
                ReflectionDelivery::Offer
            } else {
                ReflectionDelivery::Queued
            }
        }
        ReflectionOpportunity::ConversationIdle | ReflectionOpportunity::ScheduledReview => {
            if last_proactive_offer_at.is_some_and(|offered_at| {
                offered_at
                    .as_millis()
                    .saturating_add(REFLECTION_PROACTIVE_COOLDOWN_MILLIS)
                    > now.as_millis()
            }) {
                ReflectionDelivery::Queued
            } else {
                ReflectionDelivery::Offer
            }
        }
    }
}

/// Moves one eligible invitation into the offered state.
///
/// # Errors
///
/// Returns [`ReflectionTransitionError::NotOfferable`] for any state other
/// than pending/deferred, except the fixed immediate-risk mute override.
pub fn offer_reflection_invitation(
    invitation: &ReflectionInvitation,
    now: Timestamp,
) -> Result<ReflectionInvitation, ReflectionTransitionError> {
    let offerable = matches!(
        invitation.state(),
        ReflectionInvitationState::Pending | ReflectionInvitationState::Deferred
    ) || (invitation.state() == ReflectionInvitationState::MutedByPerson
        && invitation.importance() == ReflectionImportance::ImmediateSafetyRisk);
    if !offerable {
        return Err(ReflectionTransitionError::NotOfferable);
    }
    Ok(ReflectionInvitation::restore(
        invitation.id(),
        invitation.topic_key(),
        invitation.observation(),
        invitation.evidence_refs().to_vec(),
        invitation.why_now(),
        invitation.importance(),
        invitation.basis(),
        ReflectionInvitationState::Offered,
        invitation.created_at(),
        now,
        None,
        Some(now),
        invitation.defer_count(),
        invitation.mute_prompted() || invitation.defer_count() > 0,
    ))
}

/// Applies one person's explicit decision to an offered invitation.
///
/// # Errors
///
/// Returns [`ReflectionTransitionError`] when the invitation is not offered or
/// its deterministic deferral counter cannot advance.
pub fn decide_reflection_invitation(
    invitation: &ReflectionInvitation,
    decision: ReflectionDecision,
    now: Timestamp,
) -> Result<ReflectionInvitation, ReflectionTransitionError> {
    if invitation.state() != ReflectionInvitationState::Offered {
        return Err(ReflectionTransitionError::NotAwaitingDecision);
    }
    let (state, next_eligible_at, defer_count) = match decision {
        ReflectionDecision::Defer => (
            ReflectionInvitationState::Deferred,
            Some(Timestamp::from_millis(
                now.as_millis().saturating_add(REFLECTION_DEFER_MILLIS),
            )),
            invitation
                .defer_count()
                .checked_add(1)
                .ok_or(ReflectionTransitionError::DeferCountOverflow)?,
        ),
        ReflectionDecision::Mute => (
            ReflectionInvitationState::MutedByPerson,
            None,
            invitation.defer_count(),
        ),
        ReflectionDecision::Resolve => (
            ReflectionInvitationState::Resolved,
            None,
            invitation.defer_count(),
        ),
    };
    Ok(ReflectionInvitation::restore(
        invitation.id(),
        invitation.topic_key(),
        invitation.observation(),
        invitation.evidence_refs().to_vec(),
        invitation.why_now(),
        invitation.importance(),
        invitation.basis(),
        state,
        invitation.created_at(),
        now,
        next_eligible_at,
        invitation.last_offered_at(),
        defer_count,
        invitation.mute_prompted(),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityField {
    Name,
    ExpressionTraits,
    Viewpoints,
    ValuePriorities,
    RelationshipPosture,
    OwnGoals,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CounterpartInconsistencyReason {
    IntroductionMissing {
        identity_version: Option<u64>,
        self_bundle_version: Option<u64>,
    },
    IdentityMissing {
        self_bundle_version: u64,
        referenced_identity_version: u64,
    },
    SelfBundleMissing {
        identity_version: u64,
    },
    IdentityVersionMismatch {
        identity_version: u64,
        self_bundle_version: u64,
        referenced_identity_version: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CounterpartReadiness {
    NeedsIntroduction,
    IntroductionRecorded,
    Ready {
        identity_version: u64,
        self_bundle_version: u64,
    },
    Inconsistent {
        reason: CounterpartInconsistencyReason,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityProfileSnapshot {
    name: String,
    expression_traits: String,
    viewpoints: String,
    value_priorities: String,
    relationship_posture: String,
    own_goals: String,
}

impl IdentityProfileSnapshot {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        expression_traits: impl Into<String>,
        viewpoints: impl Into<String>,
        value_priorities: impl Into<String>,
        relationship_posture: impl Into<String>,
        own_goals: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            expression_traits: expression_traits.into(),
            viewpoints: viewpoints.into(),
            value_priorities: value_priorities.into(),
            relationship_posture: relationship_posture.into(),
            own_goals: own_goals.into(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn expression_traits(&self) -> &str {
        &self.expression_traits
    }

    #[must_use]
    pub fn viewpoints(&self) -> &str {
        &self.viewpoints
    }

    #[must_use]
    pub fn value_priorities(&self) -> &str {
        &self.value_priorities
    }

    #[must_use]
    pub fn relationship_posture(&self) -> &str {
        &self.relationship_posture
    }

    #[must_use]
    pub fn own_goals(&self) -> &str {
        &self.own_goals
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityStateSnapshot {
    version: u64,
    predecessor_version: Option<u64>,
    profile: IdentityProfileSnapshot,
    change_reason: String,
    evidence_refs: Vec<EvidenceId>,
    formed_at: Timestamp,
}

impl IdentityStateSnapshot {
    #[must_use]
    pub fn restore(
        version: u64,
        predecessor_version: Option<u64>,
        profile: IdentityProfileSnapshot,
        change_reason: impl Into<String>,
        evidence_refs: Vec<EvidenceId>,
        formed_at: Timestamp,
    ) -> Self {
        Self {
            version,
            predecessor_version,
            profile,
            change_reason: change_reason.into(),
            evidence_refs,
            formed_at,
        }
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
    pub const fn profile(&self) -> &IdentityProfileSnapshot {
        &self.profile
    }

    #[must_use]
    pub fn change_reason(&self) -> &str {
        &self.change_reason
    }

    #[must_use]
    pub fn evidence_refs(&self) -> &[EvidenceId] {
        &self.evidence_refs
    }

    #[must_use]
    pub const fn formed_at(&self) -> Timestamp {
        self.formed_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityRuntimeContext {
    constitution_version: u64,
    self_bundle_version: u64,
    state: IdentityStateSnapshot,
}

impl IdentityRuntimeContext {
    #[must_use]
    pub const fn new(
        constitution_version: u64,
        self_bundle_version: u64,
        state: IdentityStateSnapshot,
    ) -> Self {
        Self {
            constitution_version,
            self_bundle_version,
            state,
        }
    }

    #[must_use]
    pub const fn constitution_version(&self) -> u64 {
        self.constitution_version
    }

    #[must_use]
    pub const fn self_bundle_version(&self) -> u64 {
        self.self_bundle_version
    }

    #[must_use]
    pub const fn state(&self) -> &IdentityStateSnapshot {
        &self.state
    }
}

/// Maximum dynamic content admitted into one ordinary-turn counterpart self
/// context. The estimate includes a conservative allowance for JSON structure
/// in addition to every emitted UTF-8 string.
pub const MAX_COUNTERPART_SELF_CONTEXT_BYTES: usize = 64 * 1_024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfBundleSnapshot {
    version: u64,
    constitution_version: u64,
    identity_state_version: u64,
    counterpart_experience_refs: Vec<String>,
    belief_refs: Vec<ClaimId>,
    relationship_state: String,
    pending_intentions: Vec<String>,
}

impl SelfBundleSnapshot {
    /// Restores the current immutable Self Bundle projection supplied by a
    /// trusted repository adapter. Core validates it against readiness and the
    /// current identity before any formal-conversation side effect.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        version: u64,
        constitution_version: u64,
        identity_state_version: u64,
        counterpart_experience_refs: Vec<String>,
        belief_refs: Vec<ClaimId>,
        relationship_state: String,
        pending_intentions: Vec<String>,
    ) -> Self {
        Self {
            version,
            constitution_version,
            identity_state_version,
            counterpart_experience_refs,
            belief_refs,
            relationship_state,
            pending_intentions,
        }
    }

    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    #[must_use]
    pub const fn constitution_version(&self) -> u64 {
        self.constitution_version
    }

    #[must_use]
    pub const fn identity_state_version(&self) -> u64 {
        self.identity_state_version
    }

    #[must_use]
    pub fn counterpart_experience_refs(&self) -> &[String] {
        &self.counterpart_experience_refs
    }

    #[must_use]
    pub fn belief_refs(&self) -> &[ClaimId] {
        &self.belief_refs
    }

    #[must_use]
    pub fn relationship_state(&self) -> &str {
        &self.relationship_state
    }

    #[must_use]
    pub fn pending_intentions(&self) -> &[String] {
        &self.pending_intentions
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CounterpartSelfContextError {
    InvalidSelfBundleState,
    BeliefNotFound(ClaimId),
    BeliefNotActive(ClaimId),
    BeliefEvidenceNotFound(EvidenceId),
    BeliefEvidenceInvalid(EvidenceId),
    RelevantExperienceNotFound(String),
    BudgetExceeded,
}

impl fmt::Display for CounterpartSelfContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CounterpartSelfContextError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CounterpartSelfContext {
    identity: IdentityRuntimeContext,
    relationship_state: String,
    active_beliefs: Vec<Claim>,
    pending_intentions: Vec<String>,
    relevant_counterpart_experiences: Vec<String>,
    accounted_bytes: usize,
}

impl CounterpartSelfContext {
    pub(crate) fn new(
        identity: IdentityRuntimeContext,
        relationship_state: String,
        active_beliefs: Vec<Claim>,
        pending_intentions: Vec<String>,
        relevant_counterpart_experiences: Vec<String>,
    ) -> Result<Self, CounterpartSelfContextError> {
        if relationship_state.trim().is_empty()
            || has_invalid_or_duplicate_strings(&pending_intentions)
            || has_invalid_or_duplicate_strings(&relevant_counterpart_experiences)
            || active_beliefs.iter().any(|claim| {
                claim.owner() != ClaimOwner::Counterpart
                    || claim.status() != ClaimStatus::Current
                    || claim.support().is_empty()
            })
            || active_beliefs
                .iter()
                .map(Claim::id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != active_beliefs.len()
        {
            return Err(CounterpartSelfContextError::InvalidSelfBundleState);
        }

        let accounted_bytes = estimate_counterpart_self_context_bytes(
            &identity,
            &relationship_state,
            &active_beliefs,
            &pending_intentions,
            &relevant_counterpart_experiences,
        );
        if accounted_bytes > MAX_COUNTERPART_SELF_CONTEXT_BYTES {
            return Err(CounterpartSelfContextError::BudgetExceeded);
        }

        Ok(Self {
            identity,
            relationship_state,
            active_beliefs,
            pending_intentions,
            relevant_counterpart_experiences,
            accounted_bytes,
        })
    }

    #[must_use]
    pub const fn identity(&self) -> &IdentityRuntimeContext {
        &self.identity
    }

    #[must_use]
    pub const fn constitution_version(&self) -> u64 {
        self.identity.constitution_version()
    }

    #[must_use]
    pub const fn self_bundle_version(&self) -> u64 {
        self.identity.self_bundle_version()
    }

    #[must_use]
    pub const fn identity_state(&self) -> &IdentityStateSnapshot {
        self.identity.state()
    }

    #[must_use]
    pub fn relationship_state(&self) -> &str {
        &self.relationship_state
    }

    #[must_use]
    pub fn active_beliefs(&self) -> &[Claim] {
        &self.active_beliefs
    }

    #[must_use]
    pub fn pending_intentions(&self) -> &[String] {
        &self.pending_intentions
    }

    #[must_use]
    pub fn relevant_counterpart_experiences(&self) -> &[String] {
        &self.relevant_counterpart_experiences
    }

    #[must_use]
    pub const fn accounted_bytes(&self) -> usize {
        self.accounted_bytes
    }
}

fn has_invalid_or_duplicate_strings(values: &[String]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    values
        .iter()
        .any(|value| value.trim().is_empty() || !seen.insert(value.as_str()))
}

fn estimate_counterpart_self_context_bytes(
    identity: &IdentityRuntimeContext,
    relationship_state: &str,
    active_beliefs: &[Claim],
    pending_intentions: &[String],
    relevant_counterpart_experiences: &[String],
) -> usize {
    const STRUCTURAL_ALLOWANCE: usize = 1_024;
    const ITEM_ALLOWANCE: usize = 128;
    let state = identity.state();
    let profile = state.profile();
    let mut total = STRUCTURAL_ALLOWANCE
        .saturating_add(profile.name().len())
        .saturating_add(profile.expression_traits().len())
        .saturating_add(profile.viewpoints().len())
        .saturating_add(profile.value_priorities().len())
        .saturating_add(profile.relationship_posture().len())
        .saturating_add(profile.own_goals().len())
        .saturating_add(state.change_reason().len())
        .saturating_add(relationship_state.len());
    for belief in active_beliefs {
        total = total
            .saturating_add(ITEM_ALLOWANCE)
            .saturating_add(belief.statement().len());
        for citation in belief.support() {
            total = total
                .saturating_add(ITEM_ALLOWANCE)
                .saturating_add(citation.quote().len());
        }
    }
    for value in pending_intentions
        .iter()
        .chain(relevant_counterpart_experiences)
    {
        total = total
            .saturating_add(ITEM_ALLOWANCE)
            .saturating_add(value.len());
    }
    total
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityRevisionAuthorship {
    Counterpart,
    Person,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityReflectivePurposeStatus {
    Preserved,
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityPersonRepresentation {
    DistinctCounterpart,
    ImpersonatesPerson,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentityProfileChanges {
    name: Option<String>,
    expression_traits: Option<String>,
    viewpoints: Option<String>,
    value_priorities: Option<String>,
    relationship_posture: Option<String>,
    own_goals: Option<String>,
}

impl IdentityProfileChanges {
    #[must_use]
    pub fn new(
        name: Option<String>,
        expression_traits: Option<String>,
        viewpoints: Option<String>,
        value_priorities: Option<String>,
        relationship_posture: Option<String>,
        own_goals: Option<String>,
    ) -> Self {
        Self {
            name,
            expression_traits,
            viewpoints,
            value_priorities,
            relationship_posture,
            own_goals,
        }
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub fn expression_traits(&self) -> Option<&str> {
        self.expression_traits.as_deref()
    }

    #[must_use]
    pub fn viewpoints(&self) -> Option<&str> {
        self.viewpoints.as_deref()
    }

    #[must_use]
    pub fn value_priorities(&self) -> Option<&str> {
        self.value_priorities.as_deref()
    }

    #[must_use]
    pub fn relationship_posture(&self) -> Option<&str> {
        self.relationship_posture.as_deref()
    }

    #[must_use]
    pub fn own_goals(&self) -> Option<&str> {
        self.own_goals.as_deref()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.expression_traits.is_none()
            && self.viewpoints.is_none()
            && self.value_priorities.is_none()
            && self.relationship_posture.is_none()
            && self.own_goals.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityRevisionProposal {
    from_version: u64,
    constitution_version: u64,
    changes: IdentityProfileChanges,
    change_reason: String,
    evidence_refs: Vec<EvidenceCitation>,
    authorship: IdentityRevisionAuthorship,
    reflective_purpose: IdentityReflectivePurposeStatus,
    person_representation: IdentityPersonRepresentation,
}

impl IdentityRevisionProposal {
    #[must_use]
    pub fn new(
        from_version: u64,
        constitution_version: u64,
        changes: IdentityProfileChanges,
        change_reason: impl Into<String>,
        evidence_refs: Vec<EvidenceCitation>,
    ) -> Self {
        Self {
            from_version,
            constitution_version,
            changes,
            change_reason: change_reason.into(),
            evidence_refs,
            authorship: IdentityRevisionAuthorship::Counterpart,
            reflective_purpose: IdentityReflectivePurposeStatus::Preserved,
            person_representation: IdentityPersonRepresentation::DistinctCounterpart,
        }
    }

    #[must_use]
    pub const fn with_authorship(mut self, authorship: IdentityRevisionAuthorship) -> Self {
        self.authorship = authorship;
        self
    }

    #[must_use]
    pub const fn with_reflective_purpose(
        mut self,
        reflective_purpose: IdentityReflectivePurposeStatus,
    ) -> Self {
        self.reflective_purpose = reflective_purpose;
        self
    }

    #[must_use]
    pub const fn with_person_representation(
        mut self,
        person_representation: IdentityPersonRepresentation,
    ) -> Self {
        self.person_representation = person_representation;
        self
    }

    #[must_use]
    pub const fn from_version(&self) -> u64 {
        self.from_version
    }

    #[must_use]
    pub const fn constitution_version(&self) -> u64 {
        self.constitution_version
    }

    #[must_use]
    pub const fn changes(&self) -> &IdentityProfileChanges {
        &self.changes
    }

    #[must_use]
    pub fn change_reason(&self) -> &str {
        &self.change_reason
    }

    #[must_use]
    pub fn evidence_refs(&self) -> &[EvidenceCitation] {
        &self.evidence_refs
    }

    #[must_use]
    pub const fn authorship(&self) -> IdentityRevisionAuthorship {
        self.authorship
    }

    #[must_use]
    pub const fn reflective_purpose(&self) -> IdentityReflectivePurposeStatus {
        self.reflective_purpose
    }

    #[must_use]
    pub const fn person_representation(&self) -> IdentityPersonRepresentation {
        self.person_representation
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityRevisionRejectionReason {
    IdentityUnavailable,
    DuplicateRevision,
    PersonAuthoredRoleCard,
    StalePredecessor { expected: u64, proposed: u64 },
    ConstitutionVersionChanged { expected: u64, proposed: u64 },
    ReflectivePurposeAbandoned,
    ImpersonatesPerson,
    EmptyChange(IdentityField),
    NoChanges,
    UnchangedProfile,
    EmptyChangeReason,
    MissingEvidence,
    EvidenceOutsideWorkingContext(EvidenceId),
    PreIdentityUnbound(EvidenceId),
    EmptyQuote(EvidenceId),
    QuoteMismatch(EvidenceId),
    VersionOverflow,
    SelfBundleVersionOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityRevisionRejection {
    proposal_index: usize,
    reason: IdentityRevisionRejectionReason,
}

impl IdentityRevisionRejection {
    pub(crate) const fn new(
        proposal_index: usize,
        reason: IdentityRevisionRejectionReason,
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
    pub const fn reason(&self) -> &IdentityRevisionRejectionReason {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityRevisionCommit {
    expected_identity_version: u64,
    expected_self_bundle_version: u64,
    constitution_version: u64,
    state: IdentityStateSnapshot,
}

impl IdentityRevisionCommit {
    #[must_use]
    pub const fn new(
        expected_identity_version: u64,
        expected_self_bundle_version: u64,
        constitution_version: u64,
        state: IdentityStateSnapshot,
    ) -> Self {
        Self {
            expected_identity_version,
            expected_self_bundle_version,
            constitution_version,
            state,
        }
    }

    #[must_use]
    pub const fn expected_identity_version(&self) -> u64 {
        self.expected_identity_version
    }

    #[must_use]
    pub const fn expected_self_bundle_version(&self) -> u64 {
        self.expected_self_bundle_version
    }

    #[must_use]
    pub const fn constitution_version(&self) -> u64 {
        self.constitution_version
    }

    #[must_use]
    pub const fn state(&self) -> &IdentityStateSnapshot {
        &self.state
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentityRevisionReceipt {
    identity_version: u64,
    self_bundle_version: u64,
}

impl IdentityRevisionReceipt {
    #[must_use]
    pub const fn new(identity_version: u64, self_bundle_version: u64) -> Self {
        Self {
            identity_version,
            self_bundle_version,
        }
    }

    #[must_use]
    pub const fn identity_version(self) -> u64 {
        self.identity_version
    }

    #[must_use]
    pub const fn self_bundle_version(self) -> u64 {
        self.self_bundle_version
    }
}

/// A counterpart-authored request to mature one provisional pattern.
///
/// This transport value deliberately carries only references and the
/// counterpart's rationale. The memory domain resolves those references and
/// decides whether the structural maturity prerequisites are present; it does
/// not judge whether the interpretation is true.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternMaturityProposal {
    memory_id: u64,
    expected_version: u64,
    new_support_claim_ids: Vec<ClaimId>,
    counter_evidence_refs: Vec<EvidenceCitation>,
    counterexample_review_ref: Option<EvidenceCitation>,
    discussion_evidence_refs: Vec<EvidenceCitation>,
    rationale: String,
}

impl PatternMaturityProposal {
    #[must_use]
    pub fn new(memory_id: u64, expected_version: u64, rationale: impl Into<String>) -> Self {
        Self {
            memory_id,
            expected_version,
            new_support_claim_ids: Vec::new(),
            counter_evidence_refs: Vec::new(),
            counterexample_review_ref: None,
            discussion_evidence_refs: Vec::new(),
            rationale: rationale.into(),
        }
    }

    #[must_use]
    pub fn with_new_support_claim(mut self, claim_id: ClaimId) -> Self {
        self.new_support_claim_ids.push(claim_id);
        self
    }

    #[must_use]
    pub fn with_new_support_claims(mut self, claim_ids: impl IntoIterator<Item = ClaimId>) -> Self {
        self.new_support_claim_ids.extend(claim_ids);
        self
    }

    #[must_use]
    pub fn with_counter_evidence(mut self, evidence: EvidenceCitation) -> Self {
        self.counter_evidence_refs.push(evidence);
        self
    }

    #[must_use]
    pub fn with_counter_evidence_all(
        mut self,
        evidence: impl IntoIterator<Item = EvidenceCitation>,
    ) -> Self {
        self.counter_evidence_refs.extend(evidence);
        self
    }

    #[must_use]
    pub fn with_counterexample_review(mut self, evidence: EvidenceCitation) -> Self {
        self.counterexample_review_ref = Some(evidence);
        self
    }

    #[must_use]
    pub fn with_discussion_evidence(
        mut self,
        evidence: impl IntoIterator<Item = EvidenceCitation>,
    ) -> Self {
        self.discussion_evidence_refs.extend(evidence);
        self
    }

    #[must_use]
    pub const fn memory_id(&self) -> u64 {
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
    pub fn counter_evidence_refs(&self) -> &[EvidenceCitation] {
        &self.counter_evidence_refs
    }

    #[must_use]
    pub const fn counterexample_review_ref(&self) -> Option<&EvidenceCitation> {
        self.counterexample_review_ref.as_ref()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternMaturityReceipt {
    memory_id: u64,
    memory_version: u64,
}

impl PatternMaturityReceipt {
    #[must_use]
    pub const fn new(memory_id: u64, memory_version: u64) -> Self {
        Self {
            memory_id,
            memory_version,
        }
    }

    #[must_use]
    pub const fn memory_id(self) -> u64 {
        self.memory_id
    }

    #[must_use]
    pub const fn memory_version(self) -> u64 {
        self.memory_version
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternMaturityCommitOutcome {
    Accepted(PatternMaturityReceipt),
    QualificationRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternMaturityWriteRejectionReason {
    DuplicateProposal,
    QualificationRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternMaturityWriteRejection {
    proposal_index: usize,
    reason: PatternMaturityWriteRejectionReason,
}

impl PatternMaturityWriteRejection {
    #[must_use]
    pub(crate) const fn new(
        proposal_index: usize,
        reason: PatternMaturityWriteRejectionReason,
    ) -> Self {
        Self {
            proposal_index,
            reason,
        }
    }

    #[must_use]
    pub const fn proposal_index(self) -> usize {
        self.proposal_index
    }

    #[must_use]
    pub const fn reason(&self) -> &PatternMaturityWriteRejectionReason {
        &self.reason
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
    identity_revision_proposals: Vec<IdentityRevisionProposal>,
    reflection_invitation_proposals: Vec<ReflectionInvitationProposal>,
    pattern_maturity_proposals: Vec<PatternMaturityProposal>,
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
            identity_revision_proposals: Vec::new(),
            reflection_invitation_proposals: Vec::new(),
            pattern_maturity_proposals: Vec::new(),
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
    pub fn with_identity_revision(mut self, proposal: IdentityRevisionProposal) -> Self {
        self.identity_revision_proposals.push(proposal);
        self
    }

    #[must_use]
    pub fn with_reflection_invitation(mut self, proposal: ReflectionInvitationProposal) -> Self {
        self.reflection_invitation_proposals.push(proposal);
        self
    }

    #[must_use]
    pub fn with_pattern_maturity(mut self, proposal: PatternMaturityProposal) -> Self {
        self.pattern_maturity_proposals.push(proposal);
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
    pub fn identity_revision_proposals(&self) -> &[IdentityRevisionProposal] {
        &self.identity_revision_proposals
    }

    #[must_use]
    pub fn reflection_invitation_proposals(&self) -> &[ReflectionInvitationProposal] {
        &self.reflection_invitation_proposals
    }

    #[must_use]
    pub fn pattern_maturity_proposals(&self) -> &[PatternMaturityProposal] {
        &self.pattern_maturity_proposals
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
    self_context: CounterpartSelfContext,
    reflection: Option<ReflectionRuntimeContext>,
}

impl RuntimeRequest {
    pub(crate) fn new(
        prompt: ConversationEvidence,
        working_context: WorkingContext,
        pending_agreement_candidates: Vec<SharedAgreementCandidate>,
        self_context: CounterpartSelfContext,
        reflection: Option<ReflectionRuntimeContext>,
    ) -> Self {
        Self {
            prompt,
            working_context,
            pending_agreement_candidates,
            self_context,
            reflection,
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

    #[must_use]
    pub const fn self_context(&self) -> &CounterpartSelfContext {
        &self.self_context
    }

    #[must_use]
    pub const fn identity(&self) -> &IdentityRuntimeContext {
        self.self_context.identity()
    }

    #[must_use]
    pub const fn reflection(&self) -> Option<&ReflectionRuntimeContext> {
        self.reflection.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JudgmentRejectionReason {
    EmptyStatement,
    MissingSupport,
    EvidenceOutsideWorkingContext(EvidenceId),
    PreIdentityUnbound(EvidenceId),
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
    accepted_person_fact_ids: Vec<ClaimId>,
    rejected_person_fact_proposals: Vec<PersonFactProposalRejection>,
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
    accepted_identity_revision: Option<IdentityRevisionReceipt>,
    rejected_identity_revisions: Vec<IdentityRevisionRejection>,
    accepted_reflection_invitations: Vec<ReflectionInvitationReceipt>,
    rejected_reflection_invitations: Vec<ReflectionInvitationRejection>,
    offered_reflection_invitation_id: Option<ReflectionInvitationId>,
    accepted_pattern_maturities: Vec<PatternMaturityReceipt>,
    rejected_pattern_maturities: Vec<PatternMaturityWriteRejection>,
    rejected_operations: Vec<StructuredOperationRejection>,
    validated_citations: Vec<EvidenceCitation>,
}

impl TurnOutcome {
    pub(crate) fn new(
        person_observation: PersonTurnObservation,
        counterpart_evidence_id: EvidenceId,
        validated_citations: Vec<EvidenceCitation>,
    ) -> Self {
        let PersonTurnObservation {
            evidence_id: person_evidence_id,
            accepted_person_fact_ids,
            rejected_person_fact_proposals,
        } = person_observation;
        Self {
            person_evidence_id,
            counterpart_evidence_id,
            accepted_person_fact_ids,
            rejected_person_fact_proposals,
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
            accepted_identity_revision: None,
            rejected_identity_revisions: Vec::new(),
            accepted_reflection_invitations: Vec::new(),
            rejected_reflection_invitations: Vec::new(),
            offered_reflection_invitation_id: None,
            accepted_pattern_maturities: Vec::new(),
            rejected_pattern_maturities: Vec::new(),
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

    pub(crate) fn with_identity_revision(
        mut self,
        accepted: Option<IdentityRevisionReceipt>,
        rejected: Vec<IdentityRevisionRejection>,
    ) -> Self {
        self.accepted_identity_revision = accepted;
        self.rejected_identity_revisions = rejected;
        self
    }

    pub(crate) fn with_reflection_invitations(
        mut self,
        accepted: Vec<ReflectionInvitationReceipt>,
        rejected: Vec<ReflectionInvitationRejection>,
        offered: Option<ReflectionInvitationId>,
    ) -> Self {
        self.accepted_reflection_invitations = accepted;
        self.rejected_reflection_invitations = rejected;
        self.offered_reflection_invitation_id = offered;
        self
    }

    pub(crate) fn with_pattern_maturities(
        mut self,
        accepted: Vec<PatternMaturityReceipt>,
        rejected: Vec<PatternMaturityWriteRejection>,
    ) -> Self {
        self.accepted_pattern_maturities = accepted;
        self.rejected_pattern_maturities = rejected;
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
    pub fn accepted_person_fact_ids(&self) -> &[ClaimId] {
        &self.accepted_person_fact_ids
    }

    #[must_use]
    pub fn rejected_person_fact_proposals(&self) -> &[PersonFactProposalRejection] {
        &self.rejected_person_fact_proposals
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
    pub const fn accepted_identity_revision(&self) -> Option<IdentityRevisionReceipt> {
        self.accepted_identity_revision
    }

    #[must_use]
    pub fn rejected_identity_revisions(&self) -> &[IdentityRevisionRejection] {
        &self.rejected_identity_revisions
    }

    #[must_use]
    pub fn accepted_reflection_invitations(&self) -> &[ReflectionInvitationReceipt] {
        &self.accepted_reflection_invitations
    }

    #[must_use]
    pub fn rejected_reflection_invitations(&self) -> &[ReflectionInvitationRejection] {
        &self.rejected_reflection_invitations
    }

    #[must_use]
    pub const fn offered_reflection_invitation_id(&self) -> Option<ReflectionInvitationId> {
        self.offered_reflection_invitation_id
    }

    #[must_use]
    pub fn accepted_pattern_maturities(&self) -> &[PatternMaturityReceipt] {
        &self.accepted_pattern_maturities
    }

    #[must_use]
    pub fn rejected_pattern_maturities(&self) -> &[PatternMaturityWriteRejection] {
        &self.rejected_pattern_maturities
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
