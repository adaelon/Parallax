use std::{collections::BTreeSet, error::Error, fmt};

use crate::{
    AgreementWithdrawal, AgreementWithdrawalActor, AgreementWithdrawalRejection,
    AgreementWithdrawalRejectionReason, ApplicableTime, Claim, ClaimCorrectionReceipt,
    ClaimCorrectionRepository, ClaimId, ClaimOwner, ClaimStatus, Clock, ConversationEvidence,
    CounterpartRuntime, EvidenceCitation, EvidenceId, ForgetReceipt, ForgetRepository,
    ForgetRequest, JudgmentProposal, JudgmentRejection, JudgmentRejectionReason, MemoryRepository,
    PersonTurnClassification, RelationalConstraintDeparture,
    RelationalConstraintDepartureRejection, RelationalConstraintDepartureRejectionReason,
    RepositoryError, RuntimeError, RuntimeRequest, SessionId, SharedAgreementAssentRejection,
    SharedAgreementAssentRejectionReason, SharedAgreementCandidate, SharedAgreementCandidateId,
    SharedAgreementCandidateStatus, SharedAgreementDecision, SharedAgreementResolution,
    SharedAgreementRevision, SharedExperience, SharedExperienceProposal, SharedExperienceRejection,
    SharedExperienceRejectionReason, SharedExperienceRepository, Speaker,
    StructuredOperationRejection, StructuredOperationRejectionReason, TurnOutcome, WorkingContext,
    agreement_is_active_at,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    EmptyConversationTurn,
    EmptyCorrection,
    UnchangedCorrection,
    InvalidCorrectionTime,
    ClaimNotFound(ClaimId),
    ClaimNotPerson(ClaimId),
    ClaimNotCurrent(ClaimId),
    ForgetNotConfirmed,
    ForgetTargetNotFound,
    SharedAgreementCandidateNotFound(SharedAgreementCandidateId),
    SharedAgreementCandidateNotAwaitingPerson(SharedAgreementCandidateId),
    SharedAgreementNotActive(ClaimId),
    InvalidSharedAgreementRevision,
    UnchangedSharedAgreementRevision,
    MissingEvidence(EvidenceId),
    InvalidResponseCitation(JudgmentRejectionReason),
    Repository(RepositoryError),
    Runtime(RuntimeError),
}

struct JudgmentWriteOutcome {
    accepted: Vec<ClaimId>,
    rejected: Vec<JudgmentRejection>,
}

struct SharedExperienceWriteOutcome {
    pending_agreements: Vec<SharedAgreementCandidateId>,
    admitted: Vec<ClaimId>,
    rejected: Vec<SharedExperienceRejection>,
}

struct SharedAgreementAssentWriteOutcome {
    assented: Vec<SharedAgreementCandidateId>,
    rejected: Vec<SharedAgreementAssentRejection>,
}

struct ConstraintDepartureWriteOutcome {
    recorded: Vec<ClaimId>,
    rejected: Vec<RelationalConstraintDepartureRejection>,
}

struct AgreementWithdrawalWriteOutcome {
    recorded: Vec<ClaimId>,
    rejected: Vec<AgreementWithdrawalRejection>,
}

fn reject_constraint_departure(
    outcome: &mut ConstraintDepartureWriteOutcome,
    proposal_index: usize,
    reason: RelationalConstraintDepartureRejectionReason,
) {
    outcome
        .rejected
        .push(RelationalConstraintDepartureRejection::new(
            proposal_index,
            reason,
        ));
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyConversationTurn => formatter.write_str("conversation turn cannot be empty"),
            Self::EmptyCorrection => formatter.write_str("correction statement cannot be empty"),
            Self::UnchangedCorrection => {
                formatter.write_str("correction statement must change the claim")
            }
            Self::InvalidCorrectionTime => formatter.write_str("correction time is invalid"),
            Self::ClaimNotFound(id) => write!(formatter, "claim {} does not exist", id.get()),
            Self::ClaimNotPerson(id) => {
                write!(formatter, "claim {} is not owned by the person", id.get())
            }
            Self::ClaimNotCurrent(id) => write!(formatter, "claim {} is not current", id.get()),
            Self::ForgetNotConfirmed => {
                formatter.write_str("forget requires explicit person confirmation")
            }
            Self::ForgetTargetNotFound => formatter.write_str("forget target does not exist"),
            Self::SharedAgreementCandidateNotFound(id) => {
                write!(
                    formatter,
                    "shared agreement candidate {} does not exist",
                    id.get()
                )
            }
            Self::SharedAgreementCandidateNotAwaitingPerson(id) => write!(
                formatter,
                "shared agreement candidate {} is not awaiting person confirmation",
                id.get()
            ),
            Self::SharedAgreementNotActive(id) => {
                write!(formatter, "shared agreement {} is not active", id.get())
            }
            Self::InvalidSharedAgreementRevision => {
                formatter.write_str("shared agreement revision has invalid boundaries")
            }
            Self::UnchangedSharedAgreementRevision => {
                formatter.write_str("shared agreement revision must change the candidate")
            }
            Self::MissingEvidence(id) => write!(formatter, "evidence {} does not exist", id.get()),
            Self::InvalidResponseCitation(reason) => {
                write!(
                    formatter,
                    "runtime response contains an invalid citation: {reason:?}"
                )
            }
            Self::Repository(error) => write!(formatter, "repository error: {error}"),
            Self::Runtime(error) => write!(formatter, "runtime error: {error}"),
        }
    }
}

impl<R, T, C> MemoryCore<R, T, C>
where
    R: ForgetRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    /// Applies one explicitly confirmed forget command as an atomic deletion
    /// intent plus target closure.
    ///
    /// # Errors
    ///
    /// Rejects missing person confirmation, unknown targets, or an adapter
    /// failure without allocating a deletion intent in Core.
    pub fn forget(&mut self, request: ForgetRequest) -> Result<ForgetReceipt, CoreError> {
        if !request.confirmed_by_person() {
            return Err(CoreError::ForgetNotConfirmed);
        }
        self.repository
            .commit_forget(request.target(), self.clock.now())?
            .ok_or(CoreError::ForgetTargetNotFound)
    }
}

impl<R, T, C> MemoryCore<R, T, C>
where
    R: ClaimCorrectionRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    /// Appends a sourced person correction and supersedes exactly one current
    /// person claim without erasing historical state.
    ///
    /// # Errors
    ///
    /// Rejects empty text, invalid time, unknown/non-person/non-current claims,
    /// or an adapter failure. Rejections persist neither evidence nor a claim.
    pub fn correct_person_fact(
        &mut self,
        session_id: SessionId,
        superseded_claim_id: ClaimId,
        corrected_statement: impl Into<String>,
        applicable_time: ApplicableTime,
    ) -> Result<ClaimCorrectionReceipt, CoreError> {
        let corrected_statement = corrected_statement.into();
        if corrected_statement.trim().is_empty() {
            return Err(CoreError::EmptyCorrection);
        }
        if !applicable_time.is_valid() {
            return Err(CoreError::InvalidCorrectionTime);
        }
        let previous = self
            .repository
            .claim(superseded_claim_id)?
            .ok_or(CoreError::ClaimNotFound(superseded_claim_id))?;
        if previous.owner() != ClaimOwner::Person {
            return Err(CoreError::ClaimNotPerson(superseded_claim_id));
        }
        if previous.status() != ClaimStatus::Current {
            return Err(CoreError::ClaimNotCurrent(superseded_claim_id));
        }
        if corrected_statement.trim() == previous.statement().trim() {
            return Err(CoreError::UnchangedCorrection);
        }

        let recorded_at = self.clock.now();
        let evidence = ConversationEvidence::new(
            self.repository.next_evidence_id(),
            session_id,
            Speaker::Person,
            corrected_statement.clone(),
            recorded_at,
        );
        let replacement = Claim::correction(
            self.repository.next_claim_id(),
            ClaimOwner::Person,
            corrected_statement,
            vec![EvidenceCitation::new(evidence.id(), evidence.verbatim())],
            None,
            applicable_time,
            recorded_at,
            superseded_claim_id,
        );
        self.repository
            .commit_person_fact_correction(evidence, replacement)
            .map_err(CoreError::from)
    }
}

impl Error for CoreError {}

impl From<RepositoryError> for CoreError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

impl From<RuntimeError> for CoreError {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}

pub struct MemoryCore<R, T, C> {
    repository: R,
    runtime: T,
    clock: C,
}

impl<R, T, C> MemoryCore<R, T, C>
where
    R: MemoryRepository,
    T: CounterpartRuntime,
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

    /// Retains a person's turn verbatim and records a person fact only for a
    /// structured direct-self-report classification.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError`] when the turn is empty, the runtime cannot classify
    /// it, or the repository cannot append the evidence or resulting claim.
    pub fn record_person_turn(
        &mut self,
        session_id: SessionId,
        verbatim: impl Into<String>,
    ) -> Result<(EvidenceId, PersonTurnClassification), CoreError> {
        let evidence = self.append_conversation_evidence(session_id, Speaker::Person, verbatim)?;
        let classification = self.runtime.classify_person_turn(&evidence)?;

        if classification == PersonTurnClassification::DirectSelfReport {
            let citation = EvidenceCitation::new(evidence.id(), evidence.verbatim());
            let claim = Claim::new(
                self.repository.next_claim_id(),
                ClaimOwner::Person,
                evidence.verbatim().to_owned(),
                vec![citation],
                None,
                ApplicableTime::At(evidence.recorded_at()),
                evidence.recorded_at(),
            );
            self.repository.append_claim(claim)?;
        }

        Ok((evidence.id(), classification))
    }

    /// Copies selected evidence into an immutable, ordered working-context value.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::MissingEvidence`] when a selected identifier does not
    /// exist, or a repository error when the adapter cannot resolve it.
    pub fn freeze_working_context(
        &mut self,
        evidence_ids: &[EvidenceId],
    ) -> Result<WorkingContext, CoreError> {
        let mut seen = BTreeSet::new();
        let mut evidence = Vec::with_capacity(evidence_ids.len());

        for id in evidence_ids {
            if !seen.insert(*id) {
                continue;
            }
            let item = self
                .repository
                .evidence(*id)?
                .ok_or(CoreError::MissingEvidence(*id))?;
            evidence.push(item);
        }

        Ok(WorkingContext::new(evidence, self.clock.now()))
    }

    /// Resolves and verifies an exact quote against retained verbatim evidence.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError`] when the evidence does not exist, the repository
    /// cannot read it, or the quote is empty or does not match verbatim text.
    pub fn resolve_citation(&self, citation: &EvidenceCitation) -> Result<String, CoreError> {
        let evidence = self
            .repository
            .evidence(citation.evidence_id())?
            .ok_or(CoreError::MissingEvidence(citation.evidence_id()))?;
        if citation.quote().is_empty() {
            return Err(CoreError::InvalidResponseCitation(
                JudgmentRejectionReason::EmptyQuote(citation.evidence_id()),
            ));
        }
        if !evidence.verbatim().contains(citation.quote()) {
            return Err(CoreError::InvalidResponseCitation(
                JudgmentRejectionReason::QuoteMismatch(citation.evidence_id()),
            ));
        }
        Ok(citation.quote().to_owned())
    }

    #[must_use]
    pub const fn repository(&self) -> &R {
        &self.repository
    }

    #[must_use]
    pub fn repository_mut(&mut self) -> &mut R {
        &mut self.repository
    }

    #[must_use]
    pub const fn runtime(&self) -> &T {
        &self.runtime
    }

    #[must_use]
    pub fn into_parts(self) -> (R, T, C) {
        (self.repository, self.runtime, self.clock)
    }

    fn append_conversation_evidence(
        &mut self,
        session_id: SessionId,
        speaker: Speaker,
        verbatim: impl Into<String>,
    ) -> Result<ConversationEvidence, CoreError> {
        let verbatim = verbatim.into();
        if verbatim.is_empty() {
            return Err(CoreError::EmptyConversationTurn);
        }
        let evidence = ConversationEvidence::new(
            self.repository.next_evidence_id(),
            session_id,
            speaker,
            verbatim,
            self.clock.now(),
        );
        self.repository.append_evidence(evidence.clone())?;
        Ok(evidence)
    }
}

impl<R, T, C> MemoryCore<R, T, C>
where
    R: SharedExperienceRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    /// Runs a person/counterpart turn against a previously frozen context.
    ///
    /// Runtime free text is retained only as conversation evidence. Structured
    /// judgment and shared-experience proposals are independently source-
    /// validated before any ledger admission.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError`] for an empty turn, adapter/runtime failure, missing
    /// prompt evidence, or a response-level citation that cannot be verified.
    pub fn run_counterpart_turn(
        &mut self,
        session_id: SessionId,
        person_verbatim: impl Into<String>,
        working_context: WorkingContext,
    ) -> Result<TurnOutcome, CoreError> {
        let (person_evidence_id, classification) =
            self.record_person_turn(session_id.clone(), person_verbatim)?;
        let prompt = self
            .repository
            .evidence(person_evidence_id)?
            .ok_or(CoreError::MissingEvidence(person_evidence_id))?;

        let pending_agreement_candidates = self
            .repository
            .all_shared_agreement_candidates()?
            .into_iter()
            .filter(|candidate| {
                candidate.status() == SharedAgreementCandidateStatus::AwaitingCounterpart
            })
            .collect();
        let request = RuntimeRequest::new(
            prompt.clone(),
            working_context,
            pending_agreement_candidates,
        );
        let validation_context = request.working_context().clone();
        let response = self.runtime.respond(request)?;

        for citation in response.citations() {
            if let Err(reason) = validate_citation(citation, &validation_context, &prompt) {
                return Err(CoreError::InvalidResponseCitation(reason));
            }
        }

        let counterpart_evidence = self.append_conversation_evidence(
            session_id,
            Speaker::Counterpart,
            response.text().to_owned(),
        )?;
        let judgments = self.persist_judgment_proposals(
            &response,
            &validation_context,
            &prompt,
            &counterpart_evidence,
        )?;
        let agreement_assents =
            self.persist_shared_agreement_assents(&response, &counterpart_evidence)?;
        let departures = self.persist_relational_constraint_departures(
            &response,
            &validation_context,
            &counterpart_evidence,
        )?;
        let withdrawals = self.persist_agreement_withdrawals(
            &response,
            &validation_context,
            &counterpart_evidence,
        )?;
        let shared = self.persist_shared_experience_proposals(
            &response,
            &validation_context,
            &prompt,
            &counterpart_evidence,
        )?;
        let rejected_operations = response
            .unsupported_operations()
            .iter()
            .map(|operation| {
                StructuredOperationRejection::new(
                    operation.operation_index(),
                    StructuredOperationRejectionReason::NotWhitelisted(operation.name().to_owned()),
                )
            })
            .collect();
        Ok(TurnOutcome::new(
            person_evidence_id,
            counterpart_evidence.id(),
            classification,
            response.citations().to_vec(),
        )
        .with_judgments(judgments.accepted, judgments.rejected)
        .with_agreement_assents(agreement_assents.assented, agreement_assents.rejected)
        .with_constraint_departures(departures.recorded, departures.rejected)
        .with_agreement_withdrawals(withdrawals.recorded, withdrawals.rejected)
        .with_shared_experiences(shared.pending_agreements, shared.admitted, shared.rejected)
        .with_rejected_operations(rejected_operations))
    }

    fn persist_relational_constraint_departures(
        &mut self,
        response: &crate::RuntimeResponse,
        validation_context: &WorkingContext,
        counterpart_evidence: &ConversationEvidence,
    ) -> Result<ConstraintDepartureWriteOutcome, CoreError> {
        let mut outcome = ConstraintDepartureWriteOutcome {
            recorded: Vec::new(),
            rejected: Vec::new(),
        };
        let agreements = self.repository.all_shared_experiences()?;
        let mut seen = BTreeSet::new();
        for (proposal_index, proposed) in response
            .relational_constraint_departures()
            .iter()
            .enumerate()
        {
            let agreement_claim_id = proposed.agreement_claim_id();
            if !seen.insert(agreement_claim_id) {
                reject_constraint_departure(
                    &mut outcome,
                    proposal_index,
                    RelationalConstraintDepartureRejectionReason::DuplicateDeparture(
                        agreement_claim_id,
                    ),
                );
                continue;
            }
            let Some(constraint) = validation_context
                .active_relational_constraints()
                .iter()
                .find(|constraint| constraint.agreement_claim_id() == agreement_claim_id)
            else {
                reject_constraint_departure(
                    &mut outcome,
                    proposal_index,
                    RelationalConstraintDepartureRejectionReason::ConstraintNotActive(
                        agreement_claim_id,
                    ),
                );
                continue;
            };
            let reason = proposed.reason().trim();
            if reason.is_empty() {
                reject_constraint_departure(
                    &mut outcome,
                    proposal_index,
                    RelationalConstraintDepartureRejectionReason::EmptyReason,
                );
                continue;
            }
            if !response.text().contains(reason) {
                reject_constraint_departure(
                    &mut outcome,
                    proposal_index,
                    RelationalConstraintDepartureRejectionReason::ReasonNotInResponse,
                );
                continue;
            }
            let Some(agreement) = agreements.iter().find(|experience| {
                experience.kind() == crate::SharedExperienceKind::Agreement
                    && experience.claim().id() == agreement_claim_id
            }) else {
                reject_constraint_departure(
                    &mut outcome,
                    proposal_index,
                    RelationalConstraintDepartureRejectionReason::AgreementNotFound(
                        agreement_claim_id,
                    ),
                );
                continue;
            };
            let departure = RelationalConstraintDeparture::new(agreement_claim_id, reason);
            let support = agreement
                .claim()
                .support()
                .iter()
                .cloned()
                .chain(std::iter::once(EvidenceCitation::new(
                    counterpart_evidence.id(),
                    reason,
                )))
                .collect();
            let claim_id = self.repository.next_claim_id();
            let claim = Claim::new(
                claim_id,
                ClaimOwner::Shared,
                format!(
                    "偏离共同约定“{}”：{}",
                    constraint.statement(),
                    departure.reason()
                ),
                support,
                None,
                ApplicableTime::At(counterpart_evidence.recorded_at()),
                counterpart_evidence.recorded_at(),
            );
            self.repository.commit_relational_constraint_departure(
                SharedExperience::agreement_breach(claim, departure),
            )?;
            outcome.recorded.push(claim_id);
        }
        Ok(outcome)
    }

    fn persist_agreement_withdrawals(
        &mut self,
        response: &crate::RuntimeResponse,
        validation_context: &WorkingContext,
        counterpart_evidence: &ConversationEvidence,
    ) -> Result<AgreementWithdrawalWriteOutcome, CoreError> {
        let mut outcome = AgreementWithdrawalWriteOutcome {
            recorded: Vec::new(),
            rejected: Vec::new(),
        };
        let candidates = self.repository.all_shared_agreement_candidates()?;
        let mut experiences = self.repository.all_shared_experiences()?;
        let mut seen = BTreeSet::new();
        for (proposal_index, proposed) in response.agreement_withdrawals().iter().enumerate() {
            let agreement_claim_id = proposed.agreement_claim_id();
            let reject = |outcome: &mut AgreementWithdrawalWriteOutcome, reason| {
                outcome
                    .rejected
                    .push(AgreementWithdrawalRejection::new(proposal_index, reason));
            };
            if !seen.insert(agreement_claim_id) {
                reject(
                    &mut outcome,
                    AgreementWithdrawalRejectionReason::DuplicateWithdrawal(agreement_claim_id),
                );
                continue;
            }
            if !validation_context
                .active_relational_constraints()
                .iter()
                .any(|constraint| constraint.agreement_claim_id() == agreement_claim_id)
                || !agreement_is_active_at(
                    agreement_claim_id,
                    &candidates,
                    &experiences,
                    counterpart_evidence.recorded_at(),
                )
            {
                reject(
                    &mut outcome,
                    AgreementWithdrawalRejectionReason::ConstraintNotActive(agreement_claim_id),
                );
                continue;
            }
            let Some(agreement) = experiences.iter().find(|experience| {
                experience.kind() == crate::SharedExperienceKind::Agreement
                    && experience.claim().id() == agreement_claim_id
            }) else {
                reject(
                    &mut outcome,
                    AgreementWithdrawalRejectionReason::AgreementNotFound(agreement_claim_id),
                );
                continue;
            };
            let reason = proposed.reason().trim();
            if reason.is_empty() {
                reject(
                    &mut outcome,
                    AgreementWithdrawalRejectionReason::EmptyReason,
                );
                continue;
            }
            if !response.text().contains(reason) {
                reject(
                    &mut outcome,
                    AgreementWithdrawalRejectionReason::ReasonNotInResponse,
                );
                continue;
            }
            let (claim_id, experience) = self.record_counterpart_agreement_withdrawal(
                agreement,
                agreement_claim_id,
                reason,
                counterpart_evidence,
            )?;
            experiences.push(experience);
            outcome.recorded.push(claim_id);
        }
        Ok(outcome)
    }

    fn record_counterpart_agreement_withdrawal(
        &mut self,
        agreement: &SharedExperience,
        agreement_claim_id: ClaimId,
        reason: &str,
        counterpart_evidence: &ConversationEvidence,
    ) -> Result<(ClaimId, SharedExperience), CoreError> {
        let support = agreement
            .claim()
            .support()
            .iter()
            .cloned()
            .chain(std::iter::once(EvidenceCitation::new(
                counterpart_evidence.id(),
                reason,
            )))
            .collect::<Vec<_>>();
        let claim_id = self.repository.next_claim_id();
        let claim = Claim::new(
            claim_id,
            ClaimOwner::Shared,
            format!(
                "第二自我退出共同约定“{}”：{}",
                agreement.claim().statement(),
                reason
            ),
            support.clone(),
            None,
            ApplicableTime::At(counterpart_evidence.recorded_at()),
            counterpart_evidence.recorded_at(),
        );
        let withdrawal = AgreementWithdrawal::recorded(
            claim_id,
            agreement_claim_id,
            AgreementWithdrawalActor::Counterpart,
            counterpart_evidence.recorded_at(),
            Some(reason.to_owned()),
            support,
        );
        let experience = SharedExperience::admitted_agreement_withdrawal(claim, withdrawal);
        self.repository
            .commit_agreement_withdrawal(None, experience.clone())?;
        Ok((claim_id, experience))
    }

    fn persist_shared_agreement_assents(
        &mut self,
        response: &crate::RuntimeResponse,
        counterpart_evidence: &ConversationEvidence,
    ) -> Result<SharedAgreementAssentWriteOutcome, CoreError> {
        let mut outcome = SharedAgreementAssentWriteOutcome {
            assented: Vec::new(),
            rejected: Vec::new(),
        };
        for (proposal_index, assent) in response.shared_agreement_assents().iter().enumerate() {
            let Some(candidate) = self
                .repository
                .shared_agreement_candidate(assent.candidate_id())?
            else {
                outcome.rejected.push(SharedAgreementAssentRejection::new(
                    proposal_index,
                    SharedAgreementAssentRejectionReason::CandidateNotFound(assent.candidate_id()),
                ));
                continue;
            };
            let rejection =
                if candidate.status() != SharedAgreementCandidateStatus::AwaitingCounterpart {
                    Some(
                        SharedAgreementAssentRejectionReason::CandidateNotAwaitingCounterpart(
                            assent.candidate_id(),
                        ),
                    )
                } else if candidate.version() != assent.version() {
                    Some(SharedAgreementAssentRejectionReason::VersionMismatch {
                        candidate_id: assent.candidate_id(),
                        expected: candidate.version(),
                        actual: assent.version(),
                    })
                } else if assent.counterpart_quote().is_empty() {
                    Some(SharedAgreementAssentRejectionReason::EmptyCounterpartQuote)
                } else if !response.text().contains(assent.counterpart_quote()) {
                    Some(SharedAgreementAssentRejectionReason::CounterpartQuoteMismatch)
                } else {
                    None
                };
            if let Some(reason) = rejection {
                outcome
                    .rejected
                    .push(SharedAgreementAssentRejection::new(proposal_index, reason));
                continue;
            }
            self.repository.commit_counterpart_agreement_assent(
                assent.candidate_id(),
                assent.version(),
                EvidenceCitation::new(counterpart_evidence.id(), assent.counterpart_quote()),
                counterpart_evidence.recorded_at(),
            )?;
            outcome.assented.push(assent.candidate_id());
        }
        Ok(outcome)
    }

    fn persist_judgment_proposals(
        &mut self,
        response: &crate::RuntimeResponse,
        validation_context: &WorkingContext,
        prompt: &ConversationEvidence,
        counterpart_evidence: &ConversationEvidence,
    ) -> Result<JudgmentWriteOutcome, CoreError> {
        let mut outcome = JudgmentWriteOutcome {
            accepted: Vec::new(),
            rejected: Vec::new(),
        };
        for (proposal_index, proposal) in response.judgment_proposals().iter().enumerate() {
            match validate_judgment(proposal, validation_context, prompt) {
                Ok(()) => {
                    let claim_id = self.repository.next_claim_id();
                    let claim = Claim::new(
                        claim_id,
                        ClaimOwner::Counterpart,
                        proposal.statement().to_owned(),
                        proposal.support().to_vec(),
                        Some(proposal.uncertainty()),
                        proposal.applicable_time(),
                        counterpart_evidence.recorded_at(),
                    );
                    self.repository.append_claim(claim)?;
                    outcome.accepted.push(claim_id);
                }
                Err(reason) => outcome
                    .rejected
                    .push(JudgmentRejection::new(proposal_index, reason)),
            }
        }
        Ok(outcome)
    }

    fn persist_shared_experience_proposals(
        &mut self,
        response: &crate::RuntimeResponse,
        validation_context: &WorkingContext,
        prompt: &ConversationEvidence,
        counterpart_evidence: &ConversationEvidence,
    ) -> Result<SharedExperienceWriteOutcome, CoreError> {
        let mut outcome = SharedExperienceWriteOutcome {
            pending_agreements: Vec::new(),
            admitted: Vec::new(),
            rejected: Vec::new(),
        };
        for (proposal_index, proposal) in response.shared_experience_proposals().iter().enumerate()
        {
            let validation = validate_shared_experience_proposal(
                proposal,
                validation_context,
                prompt,
                response.text(),
            );
            if let Err(reason) = validation {
                outcome
                    .rejected
                    .push(SharedExperienceRejection::new(proposal_index, reason));
                continue;
            }
            let support = proposal
                .person_support()
                .iter()
                .cloned()
                .chain(std::iter::once(EvidenceCitation::new(
                    counterpart_evidence.id(),
                    proposal.counterpart_quote(),
                )))
                .collect::<Vec<_>>();
            if proposal.kind().requires_person_confirmation() {
                let revision = SharedAgreementRevision::new(
                    proposal.statement(),
                    proposal
                        .agreement_scope()
                        .expect("validated agreement scope"),
                    proposal
                        .agreement_effective_from()
                        .expect("validated agreement effective time"),
                    proposal.agreement_effective_until(),
                    proposal.agreement_end_condition().map(str::to_owned),
                )
                .with_superseded_agreements(proposal.supersedes_agreement_ids().to_vec());
                if let Err(reason) = validate_agreement_supersession(
                    &revision,
                    &self.repository.all_shared_agreement_candidates()?,
                    &self.repository.all_shared_experiences()?,
                ) {
                    outcome
                        .rejected
                        .push(SharedExperienceRejection::new(proposal_index, reason));
                    continue;
                }
                let candidate = SharedAgreementCandidate::awaiting_person(
                    self.repository.next_shared_agreement_candidate_id(),
                    revision,
                    support,
                    proposal.occurred_at(),
                    counterpart_evidence.recorded_at(),
                );
                let candidate_id = candidate.id();
                self.repository
                    .stage_shared_agreement_candidate(candidate)?;
                outcome.pending_agreements.push(candidate_id);
                continue;
            }
            let claim_id = self.repository.next_claim_id();
            let claim = Claim::new(
                claim_id,
                ClaimOwner::Shared,
                proposal.statement().to_owned(),
                support,
                None,
                ApplicableTime::At(proposal.occurred_at()),
                counterpart_evidence.recorded_at(),
            );
            self.repository
                .commit_shared_experience(SharedExperience::admitted(proposal.kind(), claim))?;
            outcome.admitted.push(claim_id);
        }
        Ok(outcome)
    }

    /// Creates a new immutable candidate version from a person's structured
    /// ceremony revision. The previous version becomes non-signable, while the
    /// new version waits for explicit counterpart assent.
    ///
    /// # Errors
    ///
    /// Rejects unknown or non-signable candidates, unchanged/invalid terms,
    /// and adapter failures without partially recording the revision.
    pub fn revise_shared_agreement(
        &mut self,
        candidate_id: SharedAgreementCandidateId,
        session_id: SessionId,
        revision: SharedAgreementRevision,
    ) -> Result<SharedAgreementCandidateId, CoreError> {
        let previous = self
            .repository
            .shared_agreement_candidate(candidate_id)?
            .ok_or(CoreError::SharedAgreementCandidateNotFound(candidate_id))?;
        if previous.status() != SharedAgreementCandidateStatus::AwaitingPerson {
            return Err(CoreError::SharedAgreementCandidateNotAwaitingPerson(
                candidate_id,
            ));
        }
        if !revision.is_valid() {
            return Err(CoreError::InvalidSharedAgreementRevision);
        }
        if validate_agreement_supersession(
            &revision,
            &self.repository.all_shared_agreement_candidates()?,
            &self.repository.all_shared_experiences()?,
        )
        .is_err()
        {
            return Err(CoreError::InvalidSharedAgreementRevision);
        }
        if previous.statement() == revision.statement()
            && previous.scope() == Some(revision.scope())
            && previous.effective_from() == Some(revision.effective_from())
            && previous.effective_until() == revision.effective_until()
            && previous.end_condition() == revision.end_condition()
            && previous.supersedes_agreement_ids() == revision.supersedes_agreement_ids()
        {
            return Err(CoreError::UnchangedSharedAgreementRevision);
        }

        let revised_at = self.clock.now();
        let canonical = revision.canonical_text();
        let person_evidence = ConversationEvidence::new(
            self.repository.next_evidence_id(),
            session_id,
            Speaker::Person,
            canonical.clone(),
            revised_at,
        );
        let revised = SharedAgreementCandidate::awaiting_counterpart(
            self.repository.next_shared_agreement_candidate_id(),
            previous
                .version()
                .checked_add(1)
                .ok_or(CoreError::InvalidSharedAgreementRevision)?,
            candidate_id,
            revision,
            vec![EvidenceCitation::new(person_evidence.id(), canonical)],
            revised_at,
            revised_at,
        );
        let revised_id = revised.id();
        self.repository.commit_shared_agreement_revision(
            candidate_id,
            person_evidence,
            revised,
            revised_at,
        )?;
        Ok(revised_id)
    }

    /// Resolves the person-facing admission ceremony for one immutable shared
    /// agreement candidate.
    ///
    /// # Errors
    ///
    /// Rejects unknown or already-resolved candidates and adapter failures.
    pub fn resolve_shared_agreement(
        &mut self,
        candidate_id: SharedAgreementCandidateId,
        decision: SharedAgreementDecision,
    ) -> Result<SharedAgreementResolution, CoreError> {
        let candidate = self
            .repository
            .shared_agreement_candidate(candidate_id)?
            .ok_or(CoreError::SharedAgreementCandidateNotFound(candidate_id))?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingPerson {
            return Err(CoreError::SharedAgreementCandidateNotAwaitingPerson(
                candidate_id,
            ));
        }
        let decided_at = self.clock.now();
        let confirmed =
            if decision == SharedAgreementDecision::Confirm {
                let revision = SharedAgreementRevision::new(
                    candidate.statement(),
                    candidate
                        .scope()
                        .ok_or(CoreError::InvalidSharedAgreementRevision)?,
                    candidate
                        .effective_from()
                        .ok_or(CoreError::InvalidSharedAgreementRevision)?,
                    candidate.effective_until(),
                    candidate.end_condition().map(str::to_owned),
                )
                .with_superseded_agreements(candidate.supersedes_agreement_ids().to_vec());
                if validate_agreement_supersession(
                    &revision,
                    &self.repository.all_shared_agreement_candidates()?,
                    &self.repository.all_shared_experiences()?,
                )
                .is_err()
                {
                    return Err(CoreError::InvalidSharedAgreementRevision);
                }
                let effective_from = candidate
                    .effective_from()
                    .ok_or(CoreError::InvalidSharedAgreementRevision)?;
                let applicable_time = candidate.effective_until().map_or(
                    ApplicableTime::Since(effective_from),
                    |end| ApplicableTime::Between {
                        start: effective_from,
                        end,
                    },
                );
                let claim = Claim::new(
                    self.repository.next_claim_id(),
                    ClaimOwner::Shared,
                    candidate.statement().to_owned(),
                    candidate.support().to_vec(),
                    None,
                    applicable_time,
                    decided_at,
                );
                Some(SharedExperience::admitted(
                    crate::SharedExperienceKind::Agreement,
                    claim,
                ))
            } else {
                None
            };
        self.repository
            .commit_shared_agreement_decision(candidate_id, decision, confirmed, decided_at)
            .map_err(CoreError::from)
    }

    /// Applies the person-side anti-misclick gate and, only after explicit
    /// confirmation, records an immediate prospective withdrawal.
    ///
    /// # Errors
    ///
    /// Rejects a target that is not an active shared agreement and adapter
    /// failures without leaving confirmation evidence or partial history.
    pub fn withdraw_shared_agreement_as_person(
        &mut self,
        session_id: SessionId,
        agreement_claim_id: ClaimId,
        confirmed: bool,
        reason: Option<String>,
    ) -> Result<Option<ClaimId>, CoreError> {
        if !confirmed {
            return Ok(None);
        }
        let effective_at = self.clock.now();
        let candidates = self.repository.all_shared_agreement_candidates()?;
        let experiences = self.repository.all_shared_experiences()?;
        if !agreement_is_active_at(agreement_claim_id, &candidates, &experiences, effective_at) {
            return Err(CoreError::SharedAgreementNotActive(agreement_claim_id));
        }
        let agreement = experiences
            .iter()
            .find(|experience| {
                experience.kind() == crate::SharedExperienceKind::Agreement
                    && experience.claim().id() == agreement_claim_id
            })
            .ok_or(CoreError::SharedAgreementNotActive(agreement_claim_id))?;
        let reason = reason.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        });
        let canonical = reason.as_ref().map_or_else(
            || format!("确认退出共同约定 Claim {}。", agreement_claim_id.get()),
            |value| {
                format!(
                    "确认退出共同约定 Claim {}。\n理由：{}",
                    agreement_claim_id.get(),
                    value
                )
            },
        );
        let person_confirmation = ConversationEvidence::new(
            self.repository.next_evidence_id(),
            session_id,
            Speaker::Person,
            canonical.clone(),
            effective_at,
        );
        let support = agreement
            .claim()
            .support()
            .iter()
            .cloned()
            .chain(std::iter::once(EvidenceCitation::new(
                person_confirmation.id(),
                canonical,
            )))
            .collect::<Vec<_>>();
        let claim_id = self.repository.next_claim_id();
        let claim = Claim::new(
            claim_id,
            ClaimOwner::Shared,
            reason.as_ref().map_or_else(
                || format!("本人退出共同约定“{}”", agreement.claim().statement()),
                |value| {
                    format!(
                        "本人退出共同约定“{}”：{}",
                        agreement.claim().statement(),
                        value
                    )
                },
            ),
            support.clone(),
            None,
            ApplicableTime::At(effective_at),
            effective_at,
        );
        let withdrawal = AgreementWithdrawal::recorded(
            claim_id,
            agreement_claim_id,
            AgreementWithdrawalActor::Person,
            effective_at,
            reason,
            support,
        );
        self.repository.commit_agreement_withdrawal(
            Some(person_confirmation),
            SharedExperience::admitted_agreement_withdrawal(claim, withdrawal),
        )?;
        Ok(Some(claim_id))
    }

    /// Dismisses a ceremony without modifying the admitted shared claim.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when ceremony state cannot be updated.
    pub fn dismiss_shared_experience_ceremony(
        &mut self,
        claim_id: ClaimId,
    ) -> Result<bool, CoreError> {
        self.repository
            .dismiss_shared_experience_ceremony(claim_id)
            .map_err(CoreError::from)
    }
}

fn validate_judgment(
    proposal: &JudgmentProposal,
    working_context: &WorkingContext,
    prompt: &ConversationEvidence,
) -> Result<(), JudgmentRejectionReason> {
    if proposal.statement().trim().is_empty() {
        return Err(JudgmentRejectionReason::EmptyStatement);
    }
    if proposal.support().is_empty() {
        return Err(JudgmentRejectionReason::MissingSupport);
    }
    if !proposal.applicable_time().is_valid() {
        return Err(JudgmentRejectionReason::InvalidApplicableTime);
    }
    for citation in proposal.support() {
        validate_citation(citation, working_context, prompt)?;
    }
    Ok(())
}

fn validate_shared_experience_proposal(
    proposal: &SharedExperienceProposal,
    working_context: &WorkingContext,
    prompt: &ConversationEvidence,
    counterpart_text: &str,
) -> Result<(), SharedExperienceRejectionReason> {
    if proposal.statement().trim().is_empty() {
        return Err(SharedExperienceRejectionReason::EmptyStatement);
    }
    if proposal.kind() == crate::SharedExperienceKind::AgreementBreach {
        return Err(SharedExperienceRejectionReason::AgreementBreachRequiresConstraintDeparture);
    }
    if proposal.kind() == crate::SharedExperienceKind::Agreement {
        let Some(scope) = proposal.agreement_scope() else {
            return Err(SharedExperienceRejectionReason::MissingAgreementScope);
        };
        if scope.trim().is_empty() {
            return Err(SharedExperienceRejectionReason::MissingAgreementScope);
        }
        let Some(effective_from) = proposal.agreement_effective_from() else {
            return Err(SharedExperienceRejectionReason::MissingAgreementEffectiveFrom);
        };
        if proposal
            .agreement_effective_until()
            .is_some_and(|until| until.as_millis() < effective_from.as_millis())
            || proposal
                .agreement_end_condition()
                .is_some_and(|condition| condition.trim().is_empty())
        {
            return Err(SharedExperienceRejectionReason::InvalidAgreementValidity);
        }
        if proposal
            .supersedes_agreement_ids()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != proposal.supersedes_agreement_ids().len()
        {
            return Err(SharedExperienceRejectionReason::InvalidAgreementValidity);
        }
    } else if proposal.agreement_scope().is_some()
        || proposal.agreement_effective_from().is_some()
        || proposal.agreement_effective_until().is_some()
        || proposal.agreement_end_condition().is_some()
        || !proposal.supersedes_agreement_ids().is_empty()
    {
        return Err(SharedExperienceRejectionReason::UnexpectedAgreementTerms);
    }
    if proposal.person_support().is_empty() {
        return Err(SharedExperienceRejectionReason::MissingPersonSupport);
    }
    for citation in proposal.person_support() {
        let evidence = if prompt.id() == citation.evidence_id() {
            prompt
        } else {
            working_context
                .evidence()
                .iter()
                .find(|evidence| evidence.id() == citation.evidence_id())
                .ok_or(
                    SharedExperienceRejectionReason::EvidenceOutsideWorkingContext(
                        citation.evidence_id(),
                    ),
                )?
        };
        if evidence.speaker() != Speaker::Person {
            return Err(SharedExperienceRejectionReason::EvidenceNotFromPerson(
                citation.evidence_id(),
            ));
        }
        if citation.quote().is_empty() {
            return Err(SharedExperienceRejectionReason::EmptyPersonQuote(
                citation.evidence_id(),
            ));
        }
        if !evidence.verbatim().contains(citation.quote()) {
            return Err(SharedExperienceRejectionReason::PersonQuoteMismatch(
                citation.evidence_id(),
            ));
        }
    }
    if proposal.counterpart_quote().is_empty() {
        return Err(SharedExperienceRejectionReason::EmptyCounterpartQuote);
    }
    if !counterpart_text.contains(proposal.counterpart_quote()) {
        return Err(SharedExperienceRejectionReason::CounterpartQuoteMismatch);
    }
    Ok(())
}

fn validate_agreement_supersession(
    proposed: &SharedAgreementRevision,
    candidates: &[SharedAgreementCandidate],
    experiences: &[SharedExperience],
) -> Result<(), SharedExperienceRejectionReason> {
    let effective_from = proposed.effective_from();
    let is_active_at_start = |candidate: &SharedAgreementCandidate| {
        candidate.claim_id().is_some_and(|claim_id| {
            agreement_is_active_at(claim_id, candidates, experiences, effective_from)
        })
    };

    for target in proposed.supersedes_agreement_ids() {
        if !candidates
            .iter()
            .any(|candidate| candidate.claim_id() == Some(*target) && is_active_at_start(candidate))
        {
            return Err(SharedExperienceRejectionReason::SupersededAgreementNotActive(*target));
        }
    }

    let mut undeclared = candidates
        .iter()
        .filter(|candidate| is_active_at_start(candidate))
        .filter(|candidate| crate::domain::shared_agreements_conflict(proposed, candidate))
        .filter_map(SharedAgreementCandidate::claim_id)
        .filter(|claim_id| !proposed.supersedes_agreement_ids().contains(claim_id))
        .collect::<Vec<_>>();
    undeclared.sort_unstable();
    undeclared.dedup();
    if undeclared.is_empty() {
        Ok(())
    } else {
        Err(
            SharedExperienceRejectionReason::ConflictingAgreementsRequireExplicitSupersession(
                undeclared,
            ),
        )
    }
}

fn validate_citation(
    citation: &EvidenceCitation,
    working_context: &WorkingContext,
    prompt: &ConversationEvidence,
) -> Result<(), JudgmentRejectionReason> {
    let evidence = if prompt.id() == citation.evidence_id() {
        prompt
    } else {
        working_context
            .evidence()
            .iter()
            .find(|evidence| evidence.id() == citation.evidence_id())
            .ok_or(JudgmentRejectionReason::EvidenceOutsideWorkingContext(
                citation.evidence_id(),
            ))?
    };

    if citation.quote().is_empty() {
        return Err(JudgmentRejectionReason::EmptyQuote(citation.evidence_id()));
    }
    if !evidence.verbatim().contains(citation.quote()) {
        return Err(JudgmentRejectionReason::QuoteMismatch(
            citation.evidence_id(),
        ));
    }
    Ok(())
}
