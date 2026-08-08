use std::{collections::BTreeSet, error::Error, fmt};

use eam_core::{Clock, EvidenceId, RepositoryError, RuntimeError, SessionId};

use crate::{
    CounterpartInconsistencyReason, CounterpartReadiness, CounterpartRepository,
    IdentityAuthorship, IdentityRepository, IdentityRuntime, IdentityStateVersion,
    InitialIdentityProposal, InitialIdentityRequest, InitialSelfIntroduction, IntroductionAnswer,
    PersonRepresentation, ReflectivePurposeStatus, SelfBundleVersion, SelfIntroductionCategory,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityField {
    Name,
    ExpressionTraits,
    Viewpoints,
    ValuePriorities,
    RelationshipPosture,
    OwnGoals,
    ChangeReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityProposalRejectionReason {
    PersonAuthoredRoleCard,
    ReflectivePurposeAbandoned,
    ImpersonatesPerson,
    EmptyField(IdentityField),
    MissingEvidence,
    EvidenceOutsideIntroduction(EvidenceId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    MissingCategories(Vec<SelfIntroductionCategory>),
    DuplicateCategory(SelfIntroductionCategory),
    EmptyAnswer(SelfIntroductionCategory),
    IntroductionAlreadyRecorded,
    IntroductionNotRecorded,
    IdentityAlreadyFormed,
    CounterpartAlreadyCreated,
    InconsistentCounterpartState(CounterpartInconsistencyReason),
    InvalidProposal(IdentityProposalRejectionReason),
    Repository(RepositoryError),
    Runtime(RuntimeError),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for IdentityError {}

impl From<RepositoryError> for IdentityError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

impl From<RuntimeError> for IdentityError {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}

pub struct IdentityFormation<R, T, C> {
    repository: R,
    runtime: T,
    clock: C,
}

impl<R, T, C> IdentityFormation<R, T, C>
where
    R: IdentityRepository,
    T: IdentityRuntime,
    C: Clock,
{
    #[must_use]
    pub const fn new(repository: R, runtime: T, clock: C) -> Self {
        Self {
            repository,
            runtime,
            clock,
        }
    }

    /// Validates all six categories before atomically recording person evidence and facts.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] for missing, duplicate, or empty categories,
    /// an existing introduction, or a repository failure.
    pub fn record_initial_self_introduction(
        &mut self,
        session_id: &SessionId,
        answers: &[IntroductionAnswer],
    ) -> Result<InitialSelfIntroduction, IdentityError> {
        validate_answers(answers)?;
        if self.repository.initial_self_introduction()?.is_some() {
            return Err(IdentityError::IntroductionAlreadyRecorded);
        }
        let recorded_at = self.clock.now();
        self.repository
            .record_initial_self_introduction(session_id, answers, recorded_at)
            .map_err(IdentityError::from)
    }

    /// Requests, validates, and appends the counterpart's first identity version.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when introduction evidence is absent, an
    /// identity already exists, the runtime fails, or the proposal violates
    /// authorship, constitutional, identity-separation, field, or source rules.
    pub fn form_initial_identity(&mut self) -> Result<IdentityStateVersion, IdentityError> {
        if self.repository.current_identity_state()?.is_some() {
            return Err(IdentityError::IdentityAlreadyFormed);
        }
        let introduction = self
            .repository
            .initial_self_introduction()?
            .ok_or(IdentityError::IntroductionNotRecorded)?;
        let request = InitialIdentityRequest::new(introduction.clone());
        let proposal = self.runtime.form_initial_identity(request)?;
        validate_proposal(&proposal, &introduction)?;
        let identity = IdentityStateVersion::initial(proposal, self.clock.now());
        self.repository.append_identity_state(identity.clone())?;
        Ok(identity)
    }

    /// Loads the current immutable identity version.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when the repository cannot decode the state.
    pub fn current_identity(&self) -> Result<Option<IdentityStateVersion>, IdentityError> {
        self.repository
            .current_identity_state()
            .map_err(IdentityError::from)
    }

    /// Loads the complete immutable identity history.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when the repository cannot decode the chain.
    pub fn identity_history(&self) -> Result<Vec<IdentityStateVersion>, IdentityError> {
        self.repository
            .all_identity_states()
            .map_err(IdentityError::from)
    }

    #[must_use]
    pub const fn repository(&self) -> &R {
        &self.repository
    }

    #[must_use]
    pub const fn runtime(&self) -> &T {
        &self.runtime
    }

    #[must_use]
    pub fn into_parts(self) -> (R, T, C) {
        (self.repository, self.runtime, self.clock)
    }
}

impl<R, T, C> IdentityFormation<R, T, C>
where
    R: CounterpartRepository,
    T: IdentityRuntime,
    C: Clock,
{
    /// Derives the current counterpart creation state from trusted persistence.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when persisted state cannot be read or decoded.
    pub fn counterpart_readiness(&self) -> Result<CounterpartReadiness, IdentityError> {
        self.repository
            .counterpart_readiness()
            .map_err(IdentityError::from)
    }

    /// Forms and atomically commits identity v1 with Self Bundle v1.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when the introduction is absent, creation
    /// already completed, persisted state is inconsistent, the runtime or
    /// proposal fails, or the repository cannot commit the complete pair.
    pub fn form_initial_counterpart(&mut self) -> Result<CounterpartReadiness, IdentityError> {
        match self.counterpart_readiness()? {
            CounterpartReadiness::NeedsIntroduction => {
                return Err(IdentityError::IntroductionNotRecorded);
            }
            CounterpartReadiness::IntroductionRecorded => {}
            CounterpartReadiness::Ready { .. } => {
                return Err(IdentityError::CounterpartAlreadyCreated);
            }
            CounterpartReadiness::Inconsistent { reason } => {
                return Err(IdentityError::InconsistentCounterpartState(reason));
            }
        }

        let introduction = self
            .repository
            .initial_self_introduction()?
            .ok_or(IdentityError::IntroductionNotRecorded)?;
        let request = InitialIdentityRequest::new(introduction.clone());
        let proposal = self.runtime.form_initial_identity(request)?;
        validate_proposal(&proposal, &introduction)?;

        let created_at = self.clock.now();
        let identity = IdentityStateVersion::initial(proposal, created_at);
        let bundle = SelfBundleVersion::initial(&identity, created_at);
        self.repository
            .commit_initial_counterpart(identity, bundle)?;

        Ok(CounterpartReadiness::Ready {
            identity_version: 1,
            self_bundle_version: 1,
        })
    }
}

fn validate_answers(answers: &[IntroductionAnswer]) -> Result<(), IdentityError> {
    let mut seen = BTreeSet::new();
    for answer in answers {
        if answer.statement().trim().is_empty() {
            return Err(IdentityError::EmptyAnswer(answer.category()));
        }
        if !seen.insert(answer.category()) {
            return Err(IdentityError::DuplicateCategory(answer.category()));
        }
    }

    let missing = SelfIntroductionCategory::ALL
        .into_iter()
        .filter(|category| !seen.contains(category))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(IdentityError::MissingCategories(missing))
    }
}

fn validate_proposal(
    proposal: &InitialIdentityProposal,
    introduction: &InitialSelfIntroduction,
) -> Result<(), IdentityError> {
    if proposal.authorship() != IdentityAuthorship::Counterpart {
        return Err(IdentityError::InvalidProposal(
            IdentityProposalRejectionReason::PersonAuthoredRoleCard,
        ));
    }
    if proposal.reflective_purpose() != ReflectivePurposeStatus::Preserved {
        return Err(IdentityError::InvalidProposal(
            IdentityProposalRejectionReason::ReflectivePurposeAbandoned,
        ));
    }
    if proposal.person_representation() != PersonRepresentation::DistinctCounterpart {
        return Err(IdentityError::InvalidProposal(
            IdentityProposalRejectionReason::ImpersonatesPerson,
        ));
    }

    let profile = proposal.profile();
    for (field, value) in [
        (IdentityField::Name, profile.name()),
        (IdentityField::ExpressionTraits, profile.expression_traits()),
        (IdentityField::Viewpoints, profile.viewpoints()),
        (IdentityField::ValuePriorities, profile.value_priorities()),
        (
            IdentityField::RelationshipPosture,
            profile.relationship_posture(),
        ),
        (IdentityField::OwnGoals, profile.own_goals()),
        (IdentityField::ChangeReason, proposal.change_reason()),
    ] {
        if value.trim().is_empty() {
            return Err(IdentityError::InvalidProposal(
                IdentityProposalRejectionReason::EmptyField(field),
            ));
        }
    }

    if proposal.evidence_refs().is_empty() {
        return Err(IdentityError::InvalidProposal(
            IdentityProposalRejectionReason::MissingEvidence,
        ));
    }
    if let Some(evidence_id) = proposal
        .evidence_refs()
        .iter()
        .find(|evidence_id| !introduction.contains_evidence(**evidence_id))
    {
        return Err(IdentityError::InvalidProposal(
            IdentityProposalRejectionReason::EvidenceOutsideIntroduction(*evidence_id),
        ));
    }
    Ok(())
}
