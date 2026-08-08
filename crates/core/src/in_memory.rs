use std::collections::BTreeMap;

use crate::{
    AgreementWithdrawalActor, Claim, ClaimCorrectionReceipt, ClaimCorrectionRepository, ClaimId,
    ClaimOwner, ClaimStatus, ConversationEvidence, CounterpartReadiness, EvidenceId, ForgetReceipt,
    ForgetRepository, ForgetTarget, IdentityEvolutionRepository, IdentityRevisionCommit,
    IdentityRevisionReceipt, IdentityRuntimeContext, IdentityStateSnapshot,
    MAX_OPEN_REFLECTION_INVITATIONS, MemoryRepository, ReflectionInvitation,
    ReflectionInvitationId, ReflectionInvitationReceipt, ReflectionInvitationRepository,
    ReflectionInvitationState, RepositoryError, SharedAgreementCandidate,
    SharedAgreementCandidateId, SharedAgreementCandidateStatus, SharedAgreementDecision,
    SharedAgreementResolution, SharedExperience, SharedExperienceKind, SharedExperienceRepository,
    Speaker, Timestamp, agreement_is_active_at,
};

#[derive(Debug)]
pub struct InMemoryRepository {
    next_evidence_id: u64,
    next_claim_id: u64,
    evidence: BTreeMap<EvidenceId, ConversationEvidence>,
    claims: BTreeMap<ClaimId, Claim>,
    next_shared_agreement_candidate_id: u64,
    shared_agreement_candidates: BTreeMap<SharedAgreementCandidateId, SharedAgreementCandidate>,
    shared_experiences: BTreeMap<ClaimId, SharedExperience>,
    counterpart_readiness: CounterpartReadiness,
    identity_context: Option<IdentityRuntimeContext>,
    identity_history: Vec<IdentityStateSnapshot>,
    next_reflection_invitation_id: u64,
    reflection_invitations: BTreeMap<ReflectionInvitationId, ReflectionInvitation>,
    deletion_intents: BTreeMap<ForgetTarget, ForgetReceipt>,
    next_deletion_intent_id: u64,
}

impl Default for InMemoryRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryRepository {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_evidence_id: 1,
            next_claim_id: 1,
            evidence: BTreeMap::new(),
            claims: BTreeMap::new(),
            next_shared_agreement_candidate_id: 1,
            shared_agreement_candidates: BTreeMap::new(),
            shared_experiences: BTreeMap::new(),
            counterpart_readiness: CounterpartReadiness::NeedsIntroduction,
            identity_context: None,
            identity_history: Vec::new(),
            next_reflection_invitation_id: 1,
            reflection_invitations: BTreeMap::new(),
            deletion_intents: BTreeMap::new(),
            next_deletion_intent_id: 1,
        }
    }

    /// Seeds the in-memory adapter with one already-formed identity and Self Bundle.
    ///
    /// # Errors
    ///
    /// Rejects a zero version or a Self Bundle that does not point at the
    /// supplied current identity.
    pub fn with_identity_context(
        mut self,
        context: IdentityRuntimeContext,
    ) -> Result<Self, RepositoryError> {
        if context.constitution_version() == 0
            || context.self_bundle_version() == 0
            || context.state().version() == 0
        {
            return Err(RepositoryError::new(
                "identity context versions must be greater than zero",
            ));
        }
        self.identity_history.push(context.state().clone());
        self.counterpart_readiness = CounterpartReadiness::Ready {
            identity_version: context.state().version(),
            self_bundle_version: context.self_bundle_version(),
        };
        self.identity_context = Some(context);
        Ok(self)
    }

    /// Overrides the derived state for deterministic non-ready adapter tests.
    #[must_use]
    pub fn with_counterpart_readiness(mut self, readiness: CounterpartReadiness) -> Self {
        self.counterpart_readiness = readiness;
        self
    }

    fn collect_shared_agreement_forget_closure(
        &self,
        evidence_id: EvidenceId,
        affected_claims: &mut Vec<ClaimId>,
    ) -> Vec<SharedAgreementCandidateId> {
        let mut affected_candidates = self
            .shared_agreement_candidates
            .values()
            .filter(|candidate| {
                candidate
                    .support()
                    .iter()
                    .any(|citation| citation.evidence_id() == evidence_id)
                    || candidate
                        .claim_id()
                        .is_some_and(|claim_id| affected_claims.contains(&claim_id))
            })
            .map(SharedAgreementCandidate::id)
            .collect::<Vec<_>>();
        loop {
            let previous_candidate_len = affected_candidates.len();
            let previous_claim_len = affected_claims.len();
            for candidate in self.shared_agreement_candidates.values() {
                let depends_on_affected = candidate
                    .predecessor_candidate_id()
                    .is_some_and(|id| affected_candidates.contains(&id))
                    || candidate
                        .supersedes_agreement_ids()
                        .iter()
                        .any(|id| affected_claims.contains(id));
                if depends_on_affected && !affected_candidates.contains(&candidate.id()) {
                    affected_candidates.push(candidate.id());
                }
            }
            let newly_affected_claims = self
                .shared_agreement_candidates
                .values()
                .filter(|candidate| affected_candidates.contains(&candidate.id()))
                .filter_map(SharedAgreementCandidate::claim_id)
                .filter(|claim_id| !affected_claims.contains(claim_id))
                .collect::<Vec<_>>();
            affected_claims.extend(newly_affected_claims);
            if affected_candidates.len() == previous_candidate_len
                && affected_claims.len() == previous_claim_len
            {
                break;
            }
        }
        affected_candidates
    }
}

impl IdentityEvolutionRepository for InMemoryRepository {
    fn conversation_readiness(&self) -> Result<CounterpartReadiness, RepositoryError> {
        Ok(self.counterpart_readiness.clone())
    }

    fn current_identity_context(&self) -> Result<Option<IdentityRuntimeContext>, RepositoryError> {
        Ok(self.identity_context.clone())
    }

    fn commit_identity_revision(
        &mut self,
        revision: IdentityRevisionCommit,
    ) -> Result<IdentityRevisionReceipt, RepositoryError> {
        let current = self
            .identity_context
            .as_ref()
            .ok_or_else(|| RepositoryError::new("identity is not initialized"))?;
        if current.state().version() != revision.expected_identity_version()
            || current.self_bundle_version() != revision.expected_self_bundle_version()
            || current.constitution_version() != revision.constitution_version()
            || revision.state().predecessor_version() != Some(current.state().version())
            || revision.state().version() != current.state().version().saturating_add(1)
        {
            return Err(RepositoryError::new(
                "identity revision does not continue the current immutable chain",
            ));
        }
        if revision
            .state()
            .evidence_refs()
            .iter()
            .any(|id| !self.evidence.contains_key(id))
        {
            return Err(RepositoryError::new(
                "identity revision references missing evidence",
            ));
        }
        let self_bundle_version = current
            .self_bundle_version()
            .checked_add(1)
            .ok_or_else(|| RepositoryError::new("Self Bundle version space exhausted"))?;
        let state = revision.state().clone();
        let identity_version = state.version();
        self.identity_history.push(state.clone());
        self.identity_context = Some(IdentityRuntimeContext::new(
            current.constitution_version(),
            self_bundle_version,
            state,
        ));
        self.counterpart_readiness = CounterpartReadiness::Ready {
            identity_version,
            self_bundle_version,
        };
        Ok(IdentityRevisionReceipt::new(
            identity_version,
            self_bundle_version,
        ))
    }

    fn identity_history(&self) -> Result<Vec<IdentityStateSnapshot>, RepositoryError> {
        Ok(self.identity_history.clone())
    }
}

fn validate_reflection_invitation_evidence(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    invitation: &ReflectionInvitation,
) -> Result<(), RepositoryError> {
    for citation in invitation.evidence_refs() {
        let source = evidence
            .get(&citation.evidence_id())
            .ok_or_else(|| RepositoryError::new("reflection evidence does not exist"))?;
        if citation.quote().is_empty() || !source.verbatim().contains(citation.quote()) {
            return Err(RepositoryError::new(
                "reflection evidence quote does not match",
            ));
        }
    }
    Ok(())
}

fn reflection_immutable_fields_match(
    current: &ReflectionInvitation,
    updated: &ReflectionInvitation,
) -> bool {
    current.topic_key() == updated.topic_key()
        && current.observation() == updated.observation()
        && current.evidence_refs() == updated.evidence_refs()
        && current.why_now() == updated.why_now()
        && current.importance() == updated.importance()
        && current.basis() == updated.basis()
        && current.created_at() == updated.created_at()
}

impl ReflectionInvitationRepository for InMemoryRepository {
    fn next_reflection_invitation_id(&mut self) -> ReflectionInvitationId {
        let id = ReflectionInvitationId::from_raw(self.next_reflection_invitation_id);
        self.next_reflection_invitation_id = self
            .next_reflection_invitation_id
            .checked_add(1)
            .expect("reflection invitation identifier space exhausted");
        id
    }

    fn commit_reflection_invitation(
        &mut self,
        invitation: ReflectionInvitation,
    ) -> Result<ReflectionInvitationReceipt, RepositoryError> {
        if invitation.id().get() == 0
            || invitation.state() != ReflectionInvitationState::Pending
            || self.reflection_invitations.contains_key(&invitation.id())
        {
            return Err(RepositoryError::new("invalid new reflection invitation"));
        }
        validate_reflection_invitation_evidence(&self.evidence, &invitation)?;
        if self
            .reflection_invitations
            .values()
            .any(|stored| stored.is_open() && stored.topic_key() == invitation.topic_key())
        {
            return Err(RepositoryError::new(
                "reflection topic already has an open invitation",
            ));
        }
        if self
            .reflection_invitations
            .values()
            .filter(|stored| stored.is_open())
            .count()
            >= MAX_OPEN_REFLECTION_INVITATIONS
        {
            return Err(RepositoryError::new(
                "open reflection invitation budget exceeded",
            ));
        }
        let receipt = ReflectionInvitationReceipt::new(invitation.id(), invitation.state());
        self.reflection_invitations
            .insert(invitation.id(), invitation);
        Ok(receipt)
    }

    fn transition_reflection_invitation(
        &mut self,
        expected_state: ReflectionInvitationState,
        invitation: ReflectionInvitation,
    ) -> Result<ReflectionInvitationReceipt, RepositoryError> {
        let current = self
            .reflection_invitations
            .get(&invitation.id())
            .ok_or_else(|| RepositoryError::new("reflection invitation does not exist"))?;
        if current.state() != expected_state
            || !reflection_immutable_fields_match(current, &invitation)
        {
            return Err(RepositoryError::new(
                "reflection invitation compare-and-swap failed",
            ));
        }
        let receipt = ReflectionInvitationReceipt::new(invitation.id(), invitation.state());
        self.reflection_invitations
            .insert(invitation.id(), invitation);
        Ok(receipt)
    }

    fn reflection_invitation(
        &self,
        id: ReflectionInvitationId,
    ) -> Result<Option<ReflectionInvitation>, RepositoryError> {
        Ok(self.reflection_invitations.get(&id).cloned())
    }

    fn all_reflection_invitations(&self) -> Result<Vec<ReflectionInvitation>, RepositoryError> {
        Ok(self.reflection_invitations.values().cloned().collect())
    }
}

impl SharedExperienceRepository for InMemoryRepository {
    fn next_shared_agreement_candidate_id(&mut self) -> SharedAgreementCandidateId {
        let id = SharedAgreementCandidateId::from_raw(self.next_shared_agreement_candidate_id);
        self.next_shared_agreement_candidate_id = self
            .next_shared_agreement_candidate_id
            .checked_add(1)
            .expect("shared agreement candidate identifier space exhausted");
        id
    }

    fn stage_shared_agreement_candidate(
        &mut self,
        candidate: SharedAgreementCandidate,
    ) -> Result<(), RepositoryError> {
        validate_candidate_storage(&self.evidence, &candidate)?;
        validate_candidate_supersession_targets(
            &candidate,
            &self.shared_agreement_candidates,
            &self.shared_experiences,
        )?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingPerson
            || candidate.counterpart_assented_at().is_none()
            || candidate.decided_at().is_some()
            || candidate.claim_id().is_some()
        {
            return Err(RepositoryError::new(
                "new shared agreement candidate must await person confirmation",
            ));
        }
        if self
            .shared_agreement_candidates
            .insert(candidate.id(), candidate)
            .is_some()
        {
            return Err(RepositoryError::new(
                "duplicate shared agreement candidate id",
            ));
        }
        Ok(())
    }

    fn commit_shared_agreement_revision(
        &mut self,
        previous_id: SharedAgreementCandidateId,
        person_evidence: ConversationEvidence,
        revised: SharedAgreementCandidate,
        revised_at: Timestamp,
    ) -> Result<(), RepositoryError> {
        let previous = self
            .shared_agreement_candidates
            .get(&previous_id)
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if previous.status() != SharedAgreementCandidateStatus::AwaitingPerson {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting person confirmation",
            ));
        }
        if revised.status() != SharedAgreementCandidateStatus::AwaitingCounterpart
            || revised.predecessor_candidate_id() != Some(previous_id)
            || revised.version() != previous.version().saturating_add(1)
            || revised.counterpart_assented_at().is_some()
            || revised.decided_at().is_some()
            || revised.claim_id().is_some()
            || person_evidence.speaker() != Speaker::Person
            || self.evidence.contains_key(&person_evidence.id())
            || self.shared_agreement_candidates.contains_key(&revised.id())
        {
            return Err(RepositoryError::new("invalid shared agreement revision"));
        }
        let mut evidence = self.evidence.clone();
        evidence.insert(person_evidence.id(), person_evidence.clone());
        validate_candidate_storage(&evidence, &revised)?;
        validate_candidate_supersession_targets(
            &revised,
            &self.shared_agreement_candidates,
            &self.shared_experiences,
        )?;

        self.evidence.insert(person_evidence.id(), person_evidence);
        self.shared_agreement_candidates
            .get_mut(&previous_id)
            .expect("validated candidate remains present")
            .resolve(SharedAgreementCandidateStatus::Deferred, revised_at, None);
        self.shared_agreement_candidates
            .insert(revised.id(), revised);
        Ok(())
    }

    fn commit_counterpart_agreement_assent(
        &mut self,
        id: SharedAgreementCandidateId,
        version: u64,
        citation: crate::EvidenceCitation,
        assented_at: Timestamp,
    ) -> Result<SharedAgreementCandidate, RepositoryError> {
        let source = self
            .evidence
            .get(&citation.evidence_id())
            .ok_or_else(|| RepositoryError::new("counterpart assent evidence does not exist"))?;
        if source.speaker() != Speaker::Counterpart
            || !source.can_support_counterpart_knowledge()
            || citation.quote().is_empty()
            || !source.verbatim().contains(citation.quote())
        {
            return Err(RepositoryError::new(
                "counterpart assent is not an exact counterpart quote",
            ));
        }
        let candidate = self
            .shared_agreement_candidates
            .get_mut(&id)
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingCounterpart
            || candidate.version() != version
        {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting this counterpart assent",
            ));
        }
        candidate.accept_counterpart_assent(citation, assented_at);
        Ok(candidate.clone())
    }

    fn shared_agreement_candidate(
        &self,
        id: SharedAgreementCandidateId,
    ) -> Result<Option<SharedAgreementCandidate>, RepositoryError> {
        Ok(self.shared_agreement_candidates.get(&id).cloned())
    }

    fn commit_shared_agreement_decision(
        &mut self,
        id: SharedAgreementCandidateId,
        decision: SharedAgreementDecision,
        confirmed: Option<SharedExperience>,
        decided_at: Timestamp,
    ) -> Result<SharedAgreementResolution, RepositoryError> {
        let candidate = self
            .shared_agreement_candidates
            .get(&id)
            .cloned()
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingPerson {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting person confirmation",
            ));
        }

        let (status, claim_id) = match decision {
            SharedAgreementDecision::Confirm => {
                let experience = confirmed.ok_or_else(|| {
                    RepositoryError::new("confirmed agreement requires a shared claim")
                })?;
                if experience.kind() != SharedExperienceKind::Agreement
                    || experience.claim().statement() != candidate.statement()
                    || experience.claim().support() != candidate.support()
                    || candidate.effective_from().is_none()
                    || experience.claim().applicable_time() != agreement_applicable_time(&candidate)
                {
                    return Err(RepositoryError::new(
                        "confirmed agreement does not match its immutable candidate",
                    ));
                }
                validate_candidate_supersession_targets(
                    &candidate,
                    &self.shared_agreement_candidates,
                    &self.shared_experiences,
                )?;
                validate_shared_experience_storage(&self.evidence, &experience)?;
                let claim_id = experience.claim().id();
                if self.claims.contains_key(&claim_id)
                    || self.shared_experiences.contains_key(&claim_id)
                {
                    return Err(RepositoryError::new("duplicate shared claim id"));
                }
                self.claims.insert(claim_id, experience.claim().clone());
                self.shared_experiences.insert(claim_id, experience);
                (SharedAgreementCandidateStatus::Confirmed, Some(claim_id))
            }
            SharedAgreementDecision::Defer => {
                if confirmed.is_some() {
                    return Err(RepositoryError::new(
                        "deferred agreement cannot append a shared claim",
                    ));
                }
                (SharedAgreementCandidateStatus::Deferred, None)
            }
        };

        self.shared_agreement_candidates
            .get_mut(&id)
            .expect("validated candidate remains present")
            .resolve(status, decided_at, claim_id);
        Ok(SharedAgreementResolution::new(id, status, claim_id))
    }

    fn commit_shared_experience(
        &mut self,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError> {
        if matches!(
            experience.kind(),
            SharedExperienceKind::Agreement
                | SharedExperienceKind::AgreementBreach
                | SharedExperienceKind::AgreementWithdrawal
        ) {
            return Err(RepositoryError::new(
                "agreements, breaches, and withdrawals require their typed commit path",
            ));
        }
        validate_shared_experience_storage(&self.evidence, &experience)?;
        let claim_id = experience.claim().id();
        if self.claims.contains_key(&claim_id) || self.shared_experiences.contains_key(&claim_id) {
            return Err(RepositoryError::new("duplicate shared claim id"));
        }
        self.claims.insert(claim_id, experience.claim().clone());
        self.shared_experiences.insert(claim_id, experience);
        Ok(())
    }

    fn commit_relational_constraint_departure(
        &mut self,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError> {
        let departure = experience.constraint_departure().ok_or_else(|| {
            RepositoryError::new("agreement breach requires a constraint departure")
        })?;
        if experience.kind() != SharedExperienceKind::AgreementBreach
            || departure.reason().trim().is_empty()
        {
            return Err(RepositoryError::new(
                "invalid relational constraint departure",
            ));
        }
        let agreement = self
            .shared_experiences
            .get(&departure.agreement_claim_id())
            .filter(|agreement| agreement.kind() == SharedExperienceKind::Agreement)
            .ok_or_else(|| RepositoryError::new("departed agreement does not exist"))?;
        if !agreement
            .claim()
            .support()
            .iter()
            .all(|citation| experience.claim().support().contains(citation))
            || !experience.claim().support().iter().any(|citation| {
                self.evidence
                    .get(&citation.evidence_id())
                    .is_some_and(|source| {
                        source.speaker() == Speaker::Counterpart
                            && source.can_support_counterpart_knowledge()
                            && citation.quote() == departure.reason()
                    })
            })
        {
            return Err(RepositoryError::new(
                "agreement breach must preserve agreement support and exact reason evidence",
            ));
        }
        validate_shared_experience_storage(&self.evidence, &experience)?;
        let claim_id = experience.claim().id();
        if self.claims.contains_key(&claim_id) || self.shared_experiences.contains_key(&claim_id) {
            return Err(RepositoryError::new("duplicate shared claim id"));
        }
        self.claims.insert(claim_id, experience.claim().clone());
        self.shared_experiences.insert(claim_id, experience);
        Ok(())
    }

    fn commit_agreement_withdrawal(
        &mut self,
        person_confirmation: Option<ConversationEvidence>,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError> {
        let withdrawal = experience
            .agreement_withdrawal()
            .cloned()
            .ok_or_else(|| RepositoryError::new("agreement withdrawal metadata is missing"))?;
        if experience.kind() != SharedExperienceKind::AgreementWithdrawal
            || withdrawal.id() != experience.claim().id()
            || withdrawal.evidence_refs() != experience.claim().support()
            || withdrawal
                .reason()
                .is_some_and(|reason| reason.trim().is_empty())
            || (withdrawal.actor() == AgreementWithdrawalActor::Counterpart
                && withdrawal.reason().is_none())
        {
            return Err(RepositoryError::new("invalid agreement withdrawal"));
        }
        let candidates = self
            .shared_agreement_candidates
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let experiences = self
            .shared_experiences
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if !agreement_is_active_at(
            withdrawal.agreement_claim_id(),
            &candidates,
            &experiences,
            withdrawal.effective_at(),
        ) {
            return Err(RepositoryError::new(
                "withdrawn shared agreement is not active",
            ));
        }
        let agreement = self
            .shared_experiences
            .get(&withdrawal.agreement_claim_id())
            .filter(|agreement| agreement.kind() == SharedExperienceKind::Agreement)
            .ok_or_else(|| RepositoryError::new("withdrawn agreement does not exist"))?;
        if !agreement
            .claim()
            .support()
            .iter()
            .all(|citation| experience.claim().support().contains(citation))
        {
            return Err(RepositoryError::new(
                "agreement withdrawal must preserve original agreement support",
            ));
        }
        let mut evidence = self.evidence.clone();
        match (withdrawal.actor(), person_confirmation.as_ref()) {
            (AgreementWithdrawalActor::Person, Some(confirmation))
                if confirmation.speaker() == Speaker::Person
                    && confirmation.recorded_at() == withdrawal.effective_at()
                    && !evidence.contains_key(&confirmation.id()) =>
            {
                evidence.insert(confirmation.id(), confirmation.clone());
            }
            (AgreementWithdrawalActor::Counterpart, None) => {}
            _ => return Err(RepositoryError::new("withdrawal actor evidence is invalid")),
        }
        let actor_evidence_is_exact = experience.claim().support().iter().any(|citation| {
            evidence.get(&citation.evidence_id()).is_some_and(|source| {
                source.recorded_at() == withdrawal.effective_at()
                    && match withdrawal.actor() {
                        AgreementWithdrawalActor::Person => {
                            source.speaker() == Speaker::Person
                                && source.verbatim().contains(citation.quote())
                                && citation.quote().contains("确认退出共同约定 Claim")
                        }
                        AgreementWithdrawalActor::Counterpart => {
                            source.speaker() == Speaker::Counterpart
                                && source.can_support_counterpart_knowledge()
                                && withdrawal.reason() == Some(citation.quote())
                        }
                    }
            })
        });
        if !actor_evidence_is_exact {
            return Err(RepositoryError::new(
                "agreement withdrawal requires exact actor evidence",
            ));
        }
        validate_shared_experience_storage(&evidence, &experience)?;
        let claim_id = experience.claim().id();
        if self.claims.contains_key(&claim_id) || self.shared_experiences.contains_key(&claim_id) {
            return Err(RepositoryError::new("duplicate shared claim id"));
        }
        if let Some(confirmation) = person_confirmation {
            self.evidence.insert(confirmation.id(), confirmation);
        }
        self.claims.insert(claim_id, experience.claim().clone());
        self.shared_experiences.insert(claim_id, experience);
        Ok(())
    }

    fn all_shared_agreement_candidates(
        &self,
    ) -> Result<Vec<SharedAgreementCandidate>, RepositoryError> {
        Ok(self.shared_agreement_candidates.values().cloned().collect())
    }

    fn all_shared_experiences(&self) -> Result<Vec<SharedExperience>, RepositoryError> {
        Ok(self.shared_experiences.values().cloned().collect())
    }

    fn dismiss_shared_experience_ceremony(
        &mut self,
        claim_id: ClaimId,
    ) -> Result<bool, RepositoryError> {
        let Some(experience) = self.shared_experiences.get_mut(&claim_id) else {
            return Ok(false);
        };
        experience.dismiss_ceremony();
        Ok(true)
    }
}

fn validate_candidate_storage(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    candidate: &SharedAgreementCandidate,
) -> Result<(), RepositoryError> {
    if candidate.version() == 0
        || candidate.statement().trim().is_empty()
        || candidate
            .scope()
            .is_none_or(|scope| scope.trim().is_empty())
        || candidate.effective_from().is_none()
        || candidate.effective_until().is_some_and(|until| {
            until.as_millis()
                < candidate
                    .effective_from()
                    .expect("checked above")
                    .as_millis()
        })
        || candidate
            .end_condition()
            .is_some_and(|condition| condition.trim().is_empty())
        || candidate
            .supersedes_agreement_ids()
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != candidate.supersedes_agreement_ids().len()
    {
        return Err(RepositoryError::new("invalid shared agreement candidate"));
    }
    match candidate.status() {
        SharedAgreementCandidateStatus::AwaitingCounterpart => {
            validate_candidate_support(evidence, candidate.support(), true, false)
        }
        SharedAgreementCandidateStatus::AwaitingPerson
        | SharedAgreementCandidateStatus::Deferred
        | SharedAgreementCandidateStatus::Confirmed => {
            validate_candidate_support(evidence, candidate.support(), true, true)
        }
    }
}

fn validate_candidate_supersession_targets(
    candidate: &SharedAgreementCandidate,
    candidates: &BTreeMap<SharedAgreementCandidateId, SharedAgreementCandidate>,
    experiences: &BTreeMap<ClaimId, SharedExperience>,
) -> Result<(), RepositoryError> {
    let effective_from = candidate
        .effective_from()
        .ok_or_else(|| RepositoryError::new("agreement effective time is missing"))?;
    let candidates = candidates.values().cloned().collect::<Vec<_>>();
    let experiences = experiences.values().cloned().collect::<Vec<_>>();
    for target in candidate.supersedes_agreement_ids() {
        if !agreement_is_active_at(*target, &candidates, &experiences, effective_from) {
            return Err(RepositoryError::new(
                "superseded shared agreement is no longer active",
            ));
        }
    }
    Ok(())
}

fn agreement_applicable_time(candidate: &SharedAgreementCandidate) -> crate::ApplicableTime {
    let start = candidate
        .effective_from()
        .expect("signable candidates have an effective time");
    candidate
        .effective_until()
        .map_or(crate::ApplicableTime::Since(start), |end| {
            crate::ApplicableTime::Between { start, end }
        })
}

fn validate_candidate_support(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    support: &[crate::EvidenceCitation],
    require_person: bool,
    require_counterpart: bool,
) -> Result<(), RepositoryError> {
    let (has_person, has_counterpart) = validate_exact_support(evidence, support)?;
    if (require_person && !has_person) || (require_counterpart && !has_counterpart) {
        return Err(RepositoryError::new(
            "candidate signature evidence does not match its signing state",
        ));
    }
    Ok(())
}

fn validate_shared_experience_storage(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    experience: &SharedExperience,
) -> Result<(), RepositoryError> {
    let claim = experience.claim();
    if claim.owner() != ClaimOwner::Shared
        || claim.status() != ClaimStatus::Current
        || claim.statement().trim().is_empty()
        || claim.support().len() < 2
    {
        return Err(RepositoryError::new("invalid shared experience claim"));
    }
    validate_shared_support(evidence, claim.support())
}

fn validate_shared_support(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    support: &[crate::EvidenceCitation],
) -> Result<(), RepositoryError> {
    let (has_person, has_counterpart) = validate_exact_support(evidence, support)?;
    if !has_person || !has_counterpart {
        return Err(RepositoryError::new(
            "shared history requires evidence from both participants",
        ));
    }
    Ok(())
}

fn validate_exact_support(
    evidence: &BTreeMap<EvidenceId, ConversationEvidence>,
    support: &[crate::EvidenceCitation],
) -> Result<(bool, bool), RepositoryError> {
    let mut has_person = false;
    let mut has_counterpart = false;
    for citation in support {
        let source = evidence
            .get(&citation.evidence_id())
            .ok_or_else(|| RepositoryError::new("shared support evidence does not exist"))?;
        if citation.quote().is_empty() || !source.verbatim().contains(citation.quote()) {
            return Err(RepositoryError::new(
                "shared support is not an exact evidence quote",
            ));
        }
        match source.speaker() {
            Speaker::Person => has_person = true,
            Speaker::Counterpart if source.can_support_counterpart_knowledge() => {
                has_counterpart = true;
            }
            Speaker::Counterpart => {
                return Err(RepositoryError::new(
                    "shared support counterpart evidence is not identity-bound",
                ));
            }
        }
    }
    Ok((has_person, has_counterpart))
}

impl ForgetRepository for InMemoryRepository {
    fn commit_forget(
        &mut self,
        target: ForgetTarget,
        _requested_at: Timestamp,
    ) -> Result<Option<ForgetReceipt>, RepositoryError> {
        if let Some(receipt) = self.deletion_intents.get(&target) {
            return Ok(Some(*receipt));
        }
        let ForgetTarget::ConversationEvidence(evidence_id) = target else {
            return Ok(None);
        };
        if !self.evidence.contains_key(&evidence_id) {
            return Ok(None);
        }

        let mut affected_claims = self
            .claims
            .values()
            .filter(|claim| {
                claim
                    .support()
                    .iter()
                    .any(|citation| citation.evidence_id() == evidence_id)
            })
            .map(Claim::id)
            .collect::<Vec<_>>();
        let affected_candidates =
            self.collect_shared_agreement_forget_closure(evidence_id, &mut affected_claims);
        loop {
            let previous_len = affected_claims.len();
            for claim in self.claims.values() {
                let linked = claim
                    .supersedes()
                    .is_some_and(|id| affected_claims.contains(&id))
                    || claim
                        .superseded_by()
                        .is_some_and(|id| affected_claims.contains(&id));
                if linked && !affected_claims.contains(&claim.id()) {
                    affected_claims.push(claim.id());
                }
            }
            if affected_claims.len() == previous_len {
                break;
            }
        }
        for claim_id in &affected_claims {
            self.claims.remove(claim_id);
        }
        for candidate_id in &affected_candidates {
            self.shared_agreement_candidates.remove(candidate_id);
        }
        let affected_experiences = self
            .shared_experiences
            .keys()
            .filter(|claim_id| affected_claims.contains(claim_id))
            .copied()
            .collect::<Vec<_>>();
        for claim_id in &affected_experiences {
            self.shared_experiences.remove(claim_id);
        }
        let affected_reflections = self
            .reflection_invitations
            .values()
            .filter(|invitation| {
                invitation
                    .evidence_refs()
                    .iter()
                    .any(|citation| citation.evidence_id() == evidence_id)
            })
            .map(ReflectionInvitation::id)
            .collect::<Vec<_>>();
        for invitation_id in &affected_reflections {
            self.reflection_invitations.remove(invitation_id);
        }
        self.evidence.remove(&evidence_id);

        let receipt = ForgetReceipt::new(
            self.next_deletion_intent_id,
            target,
            1 + affected_claims.len(),
            affected_candidates.len() + affected_experiences.len() + affected_reflections.len(),
            0,
        );
        self.next_deletion_intent_id += 1;
        self.deletion_intents.insert(target, receipt);
        Ok(Some(receipt))
    }
}

impl MemoryRepository for InMemoryRepository {
    fn next_evidence_id(&mut self) -> EvidenceId {
        let id = EvidenceId::from_raw(self.next_evidence_id);
        self.next_evidence_id += 1;
        id
    }

    fn next_claim_id(&mut self) -> ClaimId {
        let id = ClaimId::from_raw(self.next_claim_id);
        self.next_claim_id += 1;
        id
    }

    fn append_evidence(&mut self, evidence: ConversationEvidence) -> Result<(), RepositoryError> {
        if self.evidence.insert(evidence.id(), evidence).is_some() {
            return Err(RepositoryError::new("duplicate evidence id"));
        }
        Ok(())
    }

    fn append_claim(&mut self, claim: Claim) -> Result<(), RepositoryError> {
        if self.claims.insert(claim.id(), claim).is_some() {
            return Err(RepositoryError::new("duplicate claim id"));
        }
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.get(&id).cloned())
    }

    fn all_evidence(&self) -> Result<Vec<ConversationEvidence>, RepositoryError> {
        Ok(self.evidence.values().cloned().collect())
    }

    fn all_claims(&self) -> Result<Vec<Claim>, RepositoryError> {
        Ok(self.claims.values().cloned().collect())
    }
}

impl ClaimCorrectionRepository for InMemoryRepository {
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        Ok(self.claims.get(&id).cloned())
    }

    fn commit_person_fact_correction(
        &mut self,
        evidence: ConversationEvidence,
        replacement: Claim,
    ) -> Result<ClaimCorrectionReceipt, RepositoryError> {
        let superseded_id = replacement
            .supersedes()
            .ok_or_else(|| RepositoryError::new("correction claim has no predecessor"))?;
        let previous = self
            .claims
            .get(&superseded_id)
            .ok_or_else(|| RepositoryError::new("claim does not exist"))?;
        if previous.owner() != ClaimOwner::Person || replacement.owner() != ClaimOwner::Person {
            return Err(RepositoryError::new("only person claims can be corrected"));
        }
        if previous.status() != ClaimStatus::Current {
            return Err(RepositoryError::new("claim is not current"));
        }
        if self.evidence.contains_key(&evidence.id()) || self.claims.contains_key(&replacement.id())
        {
            return Err(RepositoryError::new("duplicate correction identifier"));
        }

        self.evidence.insert(evidence.id(), evidence.clone());
        self.claims
            .get_mut(&superseded_id)
            .expect("validated predecessor remains present")
            .mark_superseded_by(replacement.id());
        self.claims.insert(replacement.id(), replacement.clone());
        Ok(ClaimCorrectionReceipt::new(
            evidence.id(),
            superseded_id,
            replacement.id(),
            0,
            0,
            0,
            0,
        ))
    }
}
