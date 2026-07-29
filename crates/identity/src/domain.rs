use eam_core::{ClaimId, EvidenceId, SessionId, Timestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SelfIntroductionCategory {
    BasicIdentityAndAddress,
    CurrentLife,
    ImportantPeople,
    LongTermGoals,
    CurrentConcerns,
    DesiredReflection,
}

impl SelfIntroductionCategory {
    pub const ALL: [Self; 6] = [
        Self::BasicIdentityAndAddress,
        Self::CurrentLife,
        Self::ImportantPeople,
        Self::LongTermGoals,
        Self::CurrentConcerns,
        Self::DesiredReflection,
    ];

    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::BasicIdentityAndAddress => 0,
            Self::CurrentLife => 1,
            Self::ImportantPeople => 2,
            Self::LongTermGoals => 3,
            Self::CurrentConcerns => 4,
            Self::DesiredReflection => 5,
        }
    }

    #[must_use]
    pub const fn from_code(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::BasicIdentityAndAddress),
            1 => Some(Self::CurrentLife),
            2 => Some(Self::ImportantPeople),
            3 => Some(Self::LongTermGoals),
            4 => Some(Self::CurrentConcerns),
            5 => Some(Self::DesiredReflection),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntroductionAnswer {
    category: SelfIntroductionCategory,
    statement: String,
}

impl IntroductionAnswer {
    #[must_use]
    pub fn new(category: SelfIntroductionCategory, statement: impl Into<String>) -> Self {
        Self {
            category,
            statement: statement.into(),
        }
    }

    #[must_use]
    pub const fn category(&self) -> SelfIntroductionCategory {
        self.category
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntroductionItem {
    category: SelfIntroductionCategory,
    evidence_id: EvidenceId,
    claim_id: ClaimId,
    statement: String,
    recorded_at: Timestamp,
}

impl IntroductionItem {
    #[must_use]
    pub fn restore(
        category: SelfIntroductionCategory,
        evidence_id: EvidenceId,
        claim_id: ClaimId,
        statement: impl Into<String>,
        recorded_at: Timestamp,
    ) -> Self {
        Self {
            category,
            evidence_id,
            claim_id,
            statement: statement.into(),
            recorded_at,
        }
    }

    #[must_use]
    pub const fn category(&self) -> SelfIntroductionCategory {
        self.category
    }

    #[must_use]
    pub const fn evidence_id(&self) -> EvidenceId {
        self.evidence_id
    }

    #[must_use]
    pub const fn claim_id(&self) -> ClaimId {
        self.claim_id
    }

    #[must_use]
    pub fn statement(&self) -> &str {
        &self.statement
    }

    #[must_use]
    pub const fn recorded_at(&self) -> Timestamp {
        self.recorded_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialSelfIntroduction {
    session_id: SessionId,
    items: Vec<IntroductionItem>,
}

impl InitialSelfIntroduction {
    #[must_use]
    pub fn restore(session_id: SessionId, items: Vec<IntroductionItem>) -> Self {
        Self { session_id, items }
    }

    #[must_use]
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    #[must_use]
    pub fn items(&self) -> &[IntroductionItem] {
        &self.items
    }

    #[must_use]
    pub fn contains_evidence(&self, evidence_id: EvidenceId) -> bool {
        self.items
            .iter()
            .any(|item| item.evidence_id() == evidence_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialIdentityRequest {
    introduction: InitialSelfIntroduction,
}

impl InitialIdentityRequest {
    pub(crate) const fn new(introduction: InitialSelfIntroduction) -> Self {
        Self { introduction }
    }

    #[must_use]
    pub const fn introduction(&self) -> &InitialSelfIntroduction {
        &self.introduction
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityAuthorship {
    Counterpart,
    Person,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflectivePurposeStatus {
    Preserved,
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersonRepresentation {
    DistinctCounterpart,
    ImpersonatesPerson,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityProfile {
    name: String,
    expression_traits: String,
    viewpoints: String,
    value_priorities: String,
    relationship_posture: String,
    own_goals: String,
}

impl IdentityProfile {
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
pub struct InitialIdentityProposal {
    profile: IdentityProfile,
    change_reason: String,
    evidence_refs: Vec<EvidenceId>,
    authorship: IdentityAuthorship,
    reflective_purpose: ReflectivePurposeStatus,
    person_representation: PersonRepresentation,
}

impl InitialIdentityProposal {
    #[must_use]
    pub fn new(
        profile: IdentityProfile,
        change_reason: impl Into<String>,
        evidence_refs: Vec<EvidenceId>,
    ) -> Self {
        Self {
            profile,
            change_reason: change_reason.into(),
            evidence_refs,
            authorship: IdentityAuthorship::Counterpart,
            reflective_purpose: ReflectivePurposeStatus::Preserved,
            person_representation: PersonRepresentation::DistinctCounterpart,
        }
    }

    #[must_use]
    pub const fn with_authorship(mut self, authorship: IdentityAuthorship) -> Self {
        self.authorship = authorship;
        self
    }

    #[must_use]
    pub const fn with_reflective_purpose(
        mut self,
        reflective_purpose: ReflectivePurposeStatus,
    ) -> Self {
        self.reflective_purpose = reflective_purpose;
        self
    }

    #[must_use]
    pub const fn with_person_representation(
        mut self,
        person_representation: PersonRepresentation,
    ) -> Self {
        self.person_representation = person_representation;
        self
    }

    #[must_use]
    pub const fn profile(&self) -> &IdentityProfile {
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
    pub const fn authorship(&self) -> IdentityAuthorship {
        self.authorship
    }

    #[must_use]
    pub const fn reflective_purpose(&self) -> ReflectivePurposeStatus {
        self.reflective_purpose
    }

    #[must_use]
    pub const fn person_representation(&self) -> PersonRepresentation {
        self.person_representation
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityStateVersion {
    version: u64,
    predecessor_version: Option<u64>,
    profile: IdentityProfile,
    change_reason: String,
    evidence_refs: Vec<EvidenceId>,
    formed_at: Timestamp,
}

impl IdentityStateVersion {
    #[must_use]
    pub fn restore(
        version: u64,
        predecessor_version: Option<u64>,
        profile: IdentityProfile,
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

    pub(crate) fn initial(proposal: InitialIdentityProposal, formed_at: Timestamp) -> Self {
        Self::restore(
            1,
            None,
            proposal.profile,
            proposal.change_reason,
            proposal.evidence_refs,
            formed_at,
        )
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
    pub const fn profile(&self) -> &IdentityProfile {
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
