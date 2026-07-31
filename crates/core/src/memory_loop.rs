use std::{collections::BTreeSet, error::Error, fmt};

use crate::{
    ApplicableTime, Claim, ClaimCorrectionReceipt, ClaimCorrectionRepository, ClaimId, ClaimOwner,
    ClaimStatus, Clock, ConversationEvidence, CounterpartRuntime, EvidenceCitation, EvidenceId,
    ForgetReceipt, ForgetRepository, ForgetRequest, JudgmentProposal, JudgmentRejection,
    JudgmentRejectionReason, MemoryRepository, PersonTurnClassification, RepositoryError,
    RuntimeError, RuntimeRequest, SessionId, SharedAgreementCandidate, SharedAgreementCandidateId,
    SharedAgreementCandidateStatus, SharedAgreementDecision, SharedAgreementResolution,
    SharedExperience, SharedExperienceProposal, SharedExperienceRejection,
    SharedExperienceRejectionReason, SharedExperienceRepository, Speaker,
    StructuredOperationRejection, StructuredOperationRejectionReason, TurnOutcome, WorkingContext,
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

        let request = RuntimeRequest::new(prompt.clone(), working_context);
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
        .with_shared_experiences(shared.pending_agreements, shared.admitted, shared.rejected)
        .with_rejected_operations(rejected_operations))
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
                let candidate = SharedAgreementCandidate::awaiting_person(
                    self.repository.next_shared_agreement_candidate_id(),
                    proposal.statement().to_owned(),
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
        let confirmed = (decision == SharedAgreementDecision::Confirm).then(|| {
            let claim = Claim::new(
                self.repository.next_claim_id(),
                ClaimOwner::Shared,
                candidate.statement().to_owned(),
                candidate.support().to_vec(),
                None,
                ApplicableTime::At(candidate.occurred_at()),
                decided_at,
            );
            SharedExperience::admitted(crate::SharedExperienceKind::Agreement, claim)
        });
        self.repository
            .commit_shared_agreement_decision(candidate_id, decision, confirmed, decided_at)
            .map_err(CoreError::from)
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
