use std::{collections::BTreeSet, error::Error, fmt};

use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, Clock, EvidenceCitation, EvidenceId,
    RepositoryError, Uncertainty,
};

use crate::{
    LongTermMemoryRepository, MAX_DISPUTE_EVIDENCE, MAX_MEMORY_SOURCES, MAX_MEMORY_TEXT_BYTES,
    MemoryBasis, MemoryConfidence, MemoryDispute, MemoryDisputeId, MemoryDisputeOutcome,
    MemoryDisputeRequest, MemoryDisputeResolution, MemoryDisputeReview,
    MemoryDisputeReviewDecision, MemoryId, MemoryProposal, MemoryStatus, MemorySubject,
    MemoryTarget, MemoryVersion, ValidatedMemoryDispute, ValidatedMemoryDisputeReview,
    ValidatedMemoryProposal,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryProposalField {
    Statement,
    SalienceReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryProposalRejectionReason {
    EmptyField(MemoryProposalField),
    OversizedField(MemoryProposalField),
    MissingSubject,
    MissingKind,
    MissingSources,
    TooManySources,
    DuplicateSource(ClaimId),
    MissingApplicableTime,
    InvalidApplicableTime,
    MissingConfidence,
    MissingBasis,
    SourceNotFound(ClaimId),
    CrossLedgerSubject {
        claim_id: ClaimId,
        owner: ClaimOwner,
        subject: MemorySubject,
    },
    ConfidenceExceedsSource(ClaimId),
    DirectEvidenceRequiresOneSource,
    DirectEvidenceRequiresCertainClaim(ClaimId),
    DirectEvidenceRequiresHighConfidence,
    DirectEvidenceStatementMismatch(ClaimId),
    DirectEvidenceTimeMismatch(ClaimId),
    MemoryNotFound(MemoryId),
    InvalidExpectedVersion,
    StaleExpectedVersion {
        expected: u64,
        actual: u64,
    },
    RevisionChangesSubject,
    RetractedClaimRequiresNewEvidence(MemoryId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryDisputeRejectionReason {
    EmptyReason,
    OversizedReason,
    MissingCounterEvidence,
    TooManyCounterEvidence,
    DuplicateCounterEvidence(EvidenceId),
    CounterEvidenceNotFound(EvidenceId),
    EmptyCounterEvidenceQuote(EvidenceId),
    CounterEvidenceQuoteMismatch(EvidenceId),
    MemoryNotFound(MemoryId),
    InvalidExpectedVersion,
    StaleExpectedVersion { expected: u64, actual: u64 },
    MemoryNotDisputable(MemoryStatus),
    OpenDisputeExists(MemoryDisputeId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryDisputeReviewRejectionReason {
    EmptyRationale,
    OversizedRationale,
    MissingEvidence,
    TooManyEvidence,
    DuplicateEvidence(EvidenceId),
    EvidenceNotFound(EvidenceId),
    EmptyEvidenceQuote(EvidenceId),
    EvidenceQuoteMismatch(EvidenceId),
    DisputeNotFound(MemoryDisputeId),
    DisputeAlreadyResolved(MemoryDisputeOutcome),
    MemoryNotFound(MemoryId),
    MemoryNoLongerDisputed(MemoryStatus),
    RevisionTargetsDifferentMemory,
    RevisionRequired,
    RevisionNotAllowed,
    RevisionDoesNotChangeStatement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryError {
    InvalidProposal(MemoryProposalRejectionReason),
    InvalidDispute(MemoryDisputeRejectionReason),
    InvalidReview(MemoryDisputeReviewRejectionReason),
    Repository(RepositoryError),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for MemoryError {}

impl From<RepositoryError> for MemoryError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

pub struct MemoryMaintenance<R, C> {
    repository: R,
    clock: C,
}

impl<R, C> MemoryMaintenance<R, C>
where
    R: LongTermMemoryRepository,
    C: Clock,
{
    #[must_use]
    pub const fn new(repository: R, clock: C) -> Self {
        Self { repository, clock }
    }

    /// Validates and atomically appends one explicit counterpart proposal.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::InvalidProposal`] for missing or unsupported
    /// fields, invalid source attribution/time/confidence, or a stale revision.
    pub fn propose(&mut self, proposal: &MemoryProposal) -> Result<MemoryVersion, MemoryError> {
        let validated = validate_proposal(&self.repository, proposal)?;
        let formed_at = self.clock.now();
        self.repository
            .append_memory(validated, formed_at)
            .map_err(MemoryError::from)
    }

    /// Records a sourced person objection and moves the current memory version
    /// into explicit dispute without letting the person overwrite it.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for incomplete evidence, stale state, or an
    /// already-open dispute.
    pub fn raise_dispute(
        &mut self,
        request: &MemoryDisputeRequest,
    ) -> Result<MemoryDispute, MemoryError> {
        let validated = validate_dispute(&self.repository, request)?;
        let raised_at = self.clock.now();
        self.repository
            .append_memory_dispute(validated, raised_at)
            .map_err(MemoryError::from)
    }

    /// Applies one counterpart-authored review as an atomic maintained,
    /// retracted, or revised transition.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection for incomplete review evidence, closed or
    /// stale disputes, or an invalid revision proposal.
    pub fn review_dispute(
        &mut self,
        review: &MemoryDisputeReview,
    ) -> Result<MemoryDisputeResolution, MemoryError> {
        let validated = validate_dispute_review(&self.repository, review)?;
        let reviewed_at = self.clock.now();
        self.repository
            .complete_memory_dispute(validated, reviewed_at)
            .map_err(MemoryError::from)
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
    pub fn into_parts(self) -> (R, C) {
        (self.repository, self.clock)
    }
}

fn validate_proposal<R: LongTermMemoryRepository>(
    repository: &R,
    proposal: &MemoryProposal,
) -> Result<ValidatedMemoryProposal, MemoryError> {
    validate_text(proposal.statement(), MemoryProposalField::Statement)?;
    let subject = proposal.subject().ok_or(MemoryError::InvalidProposal(
        MemoryProposalRejectionReason::MissingSubject,
    ))?;
    let kind = proposal.kind().ok_or(MemoryError::InvalidProposal(
        MemoryProposalRejectionReason::MissingKind,
    ))?;
    validate_sources(proposal.source_claim_ids())?;
    let applicable_time = proposal
        .applicable_time()
        .ok_or(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::MissingApplicableTime,
        ))?;
    if !applicable_time.is_valid() {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::InvalidApplicableTime,
        ));
    }
    let confidence = proposal.confidence().ok_or(MemoryError::InvalidProposal(
        MemoryProposalRejectionReason::MissingConfidence,
    ))?;
    validate_text(
        proposal.salience_reason(),
        MemoryProposalField::SalienceReason,
    )?;
    let basis = proposal.basis().ok_or(MemoryError::InvalidProposal(
        MemoryProposalRejectionReason::MissingBasis,
    ))?;
    validate_revision(repository, proposal.target(), subject)?;

    let mut claims = Vec::with_capacity(proposal.source_claim_ids().len());
    for claim_id in proposal.source_claim_ids() {
        let claim = repository
            .claim(*claim_id)?
            .ok_or(MemoryError::InvalidProposal(
                MemoryProposalRejectionReason::SourceNotFound(*claim_id),
            ))?;
        validate_subject(&claim, subject)?;
        if confidence > supported_confidence(&claim) {
            return Err(MemoryError::InvalidProposal(
                MemoryProposalRejectionReason::ConfidenceExceedsSource(*claim_id),
            ));
        }
        claims.push(claim);
    }
    for (memory_id, previous_sources) in
        repository.retracted_memory_sources(proposal.statement())?
    {
        if proposal
            .source_claim_ids()
            .iter()
            .all(|source| previous_sources.contains(source))
        {
            return Err(MemoryError::InvalidProposal(
                MemoryProposalRejectionReason::RetractedClaimRequiresNewEvidence(memory_id),
            ));
        }
    }
    validate_basis(
        basis,
        proposal.statement(),
        applicable_time,
        confidence,
        &claims,
    )?;
    let initial_status = match basis {
        MemoryBasis::DirectEvidence => MemoryStatus::Active,
        MemoryBasis::InterpretiveInference => MemoryStatus::Provisional,
        MemoryBasis::PatternCandidate => MemoryStatus::ProvisionalPattern,
    };

    Ok(ValidatedMemoryProposal::new(
        proposal.target(),
        proposal.statement().to_owned(),
        subject,
        kind,
        proposal.source_claim_ids().to_vec(),
        applicable_time,
        confidence,
        proposal.salience_reason().to_owned(),
        basis,
        initial_status,
    ))
}

fn validate_dispute<R: LongTermMemoryRepository>(
    repository: &R,
    request: &MemoryDisputeRequest,
) -> Result<ValidatedMemoryDispute, MemoryError> {
    if request.reason().trim().is_empty() {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::EmptyReason,
        ));
    }
    if request.reason().len() > MAX_MEMORY_TEXT_BYTES {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::OversizedReason,
        ));
    }
    validate_dispute_evidence(repository, request.counter_evidence())?;
    if request.expected_version() == 0 {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::InvalidExpectedVersion,
        ));
    }
    let memory =
        repository
            .current_memory(request.memory_id())?
            .ok_or(MemoryError::InvalidDispute(
                MemoryDisputeRejectionReason::MemoryNotFound(request.memory_id()),
            ))?;
    if memory.version() != request.expected_version() {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::StaleExpectedVersion {
                expected: request.expected_version(),
                actual: memory.version(),
            },
        ));
    }
    if !matches!(
        memory.status(),
        MemoryStatus::Active | MemoryStatus::Provisional | MemoryStatus::ProvisionalPattern
    ) {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::MemoryNotDisputable(memory.status()),
        ));
    }
    if let Some(open) = repository
        .memory_disputes(memory.id())?
        .into_iter()
        .find(|dispute| dispute.outcome() == MemoryDisputeOutcome::Open)
    {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::OpenDisputeExists(open.id()),
        ));
    }
    Ok(ValidatedMemoryDispute::new(
        memory.id(),
        memory.version(),
        request.reason().to_owned(),
        request.counter_evidence().to_vec(),
    ))
}

fn validate_dispute_review<R: LongTermMemoryRepository>(
    repository: &R,
    review: &MemoryDisputeReview,
) -> Result<ValidatedMemoryDisputeReview, MemoryError> {
    if review.rationale().trim().is_empty() {
        return Err(MemoryError::InvalidReview(
            MemoryDisputeReviewRejectionReason::EmptyRationale,
        ));
    }
    if review.rationale().len() > MAX_MEMORY_TEXT_BYTES {
        return Err(MemoryError::InvalidReview(
            MemoryDisputeReviewRejectionReason::OversizedRationale,
        ));
    }
    validate_review_evidence(repository, review.evidence())?;
    let dispute =
        repository
            .memory_dispute(review.dispute_id())?
            .ok_or(MemoryError::InvalidReview(
                MemoryDisputeReviewRejectionReason::DisputeNotFound(review.dispute_id()),
            ))?;
    if dispute.outcome() != MemoryDisputeOutcome::Open {
        return Err(MemoryError::InvalidReview(
            MemoryDisputeReviewRejectionReason::DisputeAlreadyResolved(dispute.outcome()),
        ));
    }
    let memory =
        repository
            .current_memory(dispute.memory_id())?
            .ok_or(MemoryError::InvalidReview(
                MemoryDisputeReviewRejectionReason::MemoryNotFound(dispute.memory_id()),
            ))?;
    if memory.version() != dispute.memory_version() || memory.status() != MemoryStatus::Disputed {
        return Err(MemoryError::InvalidReview(
            MemoryDisputeReviewRejectionReason::MemoryNoLongerDisputed(memory.status()),
        ));
    }
    let (outcome, revision) = match review.decision() {
        MemoryDisputeReviewDecision::Maintain => (MemoryDisputeOutcome::Maintained, None),
        MemoryDisputeReviewDecision::Retract => (MemoryDisputeOutcome::Retracted, None),
        MemoryDisputeReviewDecision::Revise(proposal) => {
            let MemoryTarget::Revise {
                memory_id,
                expected_version,
            } = proposal.target()
            else {
                return Err(MemoryError::InvalidReview(
                    MemoryDisputeReviewRejectionReason::RevisionRequired,
                ));
            };
            if memory_id != memory.id() || expected_version != memory.version() {
                return Err(MemoryError::InvalidReview(
                    MemoryDisputeReviewRejectionReason::RevisionTargetsDifferentMemory,
                ));
            }
            if proposal.statement().trim() == memory.statement().trim() {
                return Err(MemoryError::InvalidReview(
                    MemoryDisputeReviewRejectionReason::RevisionDoesNotChangeStatement,
                ));
            }
            (
                MemoryDisputeOutcome::Revised,
                Some(validate_proposal(repository, proposal)?),
            )
        }
    };
    Ok(ValidatedMemoryDisputeReview::new(
        review.dispute_id(),
        outcome,
        review.rationale().to_owned(),
        review.evidence().to_vec(),
        revision,
    ))
}

fn validate_dispute_evidence<R: LongTermMemoryRepository>(
    repository: &R,
    evidence: &[EvidenceCitation],
) -> Result<(), MemoryError> {
    if evidence.is_empty() {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::MissingCounterEvidence,
        ));
    }
    if evidence.len() > MAX_DISPUTE_EVIDENCE {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::TooManyCounterEvidence,
        ));
    }
    let mut unique = BTreeSet::new();
    for citation in evidence {
        if !unique.insert(citation.evidence_id()) {
            return Err(MemoryError::InvalidDispute(
                MemoryDisputeRejectionReason::DuplicateCounterEvidence(citation.evidence_id()),
            ));
        }
        let stored =
            repository
                .evidence(citation.evidence_id())?
                .ok_or(MemoryError::InvalidDispute(
                    MemoryDisputeRejectionReason::CounterEvidenceNotFound(citation.evidence_id()),
                ))?;
        if citation.quote().trim().is_empty() {
            return Err(MemoryError::InvalidDispute(
                MemoryDisputeRejectionReason::EmptyCounterEvidenceQuote(citation.evidence_id()),
            ));
        }
        if !stored.verbatim().contains(citation.quote()) {
            return Err(MemoryError::InvalidDispute(
                MemoryDisputeRejectionReason::CounterEvidenceQuoteMismatch(citation.evidence_id()),
            ));
        }
    }
    Ok(())
}

fn validate_review_evidence<R: LongTermMemoryRepository>(
    repository: &R,
    evidence: &[EvidenceCitation],
) -> Result<(), MemoryError> {
    if evidence.is_empty() {
        return Err(MemoryError::InvalidReview(
            MemoryDisputeReviewRejectionReason::MissingEvidence,
        ));
    }
    if evidence.len() > MAX_DISPUTE_EVIDENCE {
        return Err(MemoryError::InvalidReview(
            MemoryDisputeReviewRejectionReason::TooManyEvidence,
        ));
    }
    let mut unique = BTreeSet::new();
    for citation in evidence {
        if !unique.insert(citation.evidence_id()) {
            return Err(MemoryError::InvalidReview(
                MemoryDisputeReviewRejectionReason::DuplicateEvidence(citation.evidence_id()),
            ));
        }
        let stored =
            repository
                .evidence(citation.evidence_id())?
                .ok_or(MemoryError::InvalidReview(
                    MemoryDisputeReviewRejectionReason::EvidenceNotFound(citation.evidence_id()),
                ))?;
        if citation.quote().trim().is_empty() {
            return Err(MemoryError::InvalidReview(
                MemoryDisputeReviewRejectionReason::EmptyEvidenceQuote(citation.evidence_id()),
            ));
        }
        if !stored.verbatim().contains(citation.quote()) {
            return Err(MemoryError::InvalidReview(
                MemoryDisputeReviewRejectionReason::EvidenceQuoteMismatch(citation.evidence_id()),
            ));
        }
    }
    Ok(())
}

fn validate_text(value: &str, field: MemoryProposalField) -> Result<(), MemoryError> {
    if value.trim().is_empty() {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::EmptyField(field),
        ));
    }
    if value.len() > MAX_MEMORY_TEXT_BYTES {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::OversizedField(field),
        ));
    }
    Ok(())
}

fn validate_sources(source_claim_ids: &[ClaimId]) -> Result<(), MemoryError> {
    if source_claim_ids.is_empty() {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::MissingSources,
        ));
    }
    if source_claim_ids.len() > MAX_MEMORY_SOURCES {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::TooManySources,
        ));
    }
    let mut unique = BTreeSet::new();
    for claim_id in source_claim_ids {
        if !unique.insert(*claim_id) {
            return Err(MemoryError::InvalidProposal(
                MemoryProposalRejectionReason::DuplicateSource(*claim_id),
            ));
        }
    }
    Ok(())
}

fn validate_revision<R: LongTermMemoryRepository>(
    repository: &R,
    target: MemoryTarget,
    subject: MemorySubject,
) -> Result<(), MemoryError> {
    let MemoryTarget::Revise {
        memory_id,
        expected_version,
    } = target
    else {
        return Ok(());
    };
    if expected_version == 0 {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::InvalidExpectedVersion,
        ));
    }
    let current = repository
        .current_memory(memory_id)?
        .ok_or(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::MemoryNotFound(memory_id),
        ))?;
    if current.version() != expected_version {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::StaleExpectedVersion {
                expected: expected_version,
                actual: current.version(),
            },
        ));
    }
    if current.subject() != subject {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::RevisionChangesSubject,
        ));
    }
    Ok(())
}

fn validate_subject(claim: &Claim, subject: MemorySubject) -> Result<(), MemoryError> {
    let expected = match claim.owner() {
        ClaimOwner::Person => MemorySubject::Person,
        ClaimOwner::Counterpart => MemorySubject::Counterpart,
        ClaimOwner::Shared => MemorySubject::Shared,
    };
    if subject == expected {
        Ok(())
    } else {
        Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::CrossLedgerSubject {
                claim_id: claim.id(),
                owner: claim.owner(),
                subject,
            },
        ))
    }
}

const fn supported_confidence(claim: &Claim) -> MemoryConfidence {
    match claim.uncertainty() {
        None | Some(Uncertainty::Low) => MemoryConfidence::High,
        Some(Uncertainty::Medium) => MemoryConfidence::Medium,
        Some(Uncertainty::High) => MemoryConfidence::Low,
    }
}

fn validate_basis(
    basis: MemoryBasis,
    statement: &str,
    applicable_time: ApplicableTime,
    confidence: MemoryConfidence,
    claims: &[Claim],
) -> Result<(), MemoryError> {
    if basis != MemoryBasis::DirectEvidence {
        return Ok(());
    }
    let [claim] = claims else {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::DirectEvidenceRequiresOneSource,
        ));
    };
    if claim.uncertainty().is_some() {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::DirectEvidenceRequiresCertainClaim(claim.id()),
        ));
    }
    if confidence != MemoryConfidence::High {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::DirectEvidenceRequiresHighConfidence,
        ));
    }
    if statement.trim() != claim.statement().trim() {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::DirectEvidenceStatementMismatch(claim.id()),
        ));
    }
    if applicable_time != claim.applicable_time() {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::DirectEvidenceTimeMismatch(claim.id()),
        ));
    }
    Ok(())
}
