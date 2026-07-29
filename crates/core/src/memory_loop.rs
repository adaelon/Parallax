use std::{collections::BTreeSet, error::Error, fmt};

use crate::{
    ApplicableTime, Claim, ClaimOwner, Clock, ConversationEvidence, CounterpartRuntime,
    EvidenceCitation, EvidenceId, JudgmentProposal, JudgmentRejection, JudgmentRejectionReason,
    MemoryRepository, PersonTurnClassification, RepositoryError, RuntimeError, RuntimeRequest,
    SessionId, Speaker, TurnOutcome, WorkingContext,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    EmptyConversationTurn,
    MissingEvidence(EvidenceId),
    InvalidResponseCitation(JudgmentRejectionReason),
    Repository(RepositoryError),
    Runtime(RuntimeError),
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyConversationTurn => formatter.write_str("conversation turn cannot be empty"),
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

    /// Runs a person/counterpart turn against a previously frozen context.
    ///
    /// Runtime free text is retained only as conversation evidence. Structured
    /// judgment proposals are independently source-validated before ledger entry.
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

        let mut accepted_judgment_ids = Vec::new();
        let mut rejected_judgments = Vec::new();
        for (proposal_index, proposal) in response.judgment_proposals().iter().enumerate() {
            match validate_judgment(proposal, &validation_context, &prompt) {
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
                    accepted_judgment_ids.push(claim_id);
                }
                Err(reason) => {
                    rejected_judgments.push(JudgmentRejection::new(proposal_index, reason));
                }
            }
        }

        Ok(TurnOutcome::new(
            person_evidence_id,
            counterpart_evidence.id(),
            classification,
            accepted_judgment_ids,
            rejected_judgments,
            response.citations().to_vec(),
        ))
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
