use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ClaimStatus, Clock, EvidenceCitation, EvidenceId,
    PatternMaturityProposal, RepositoryError, Speaker, Timestamp, Uncertainty,
};

use crate::{
    LongTermMemoryRepository, MAX_DISPUTE_EVIDENCE, MAX_MEMORY_SOURCES, MAX_MEMORY_TEXT_BYTES,
    MemoryBasis, MemoryConfidence, MemoryDispute, MemoryDisputeId, MemoryDisputeOutcome,
    MemoryDisputeRequest, MemoryDisputeResolution, MemoryDisputeReview,
    MemoryDisputeReviewDecision, MemoryId, MemoryProposal, MemoryStatus, MemorySubject,
    MemoryTarget, MemoryVersion, ValidatedMemoryDispute, ValidatedMemoryDisputeReview,
    ValidatedMemoryProposal, ValidatedPatternMaturityProposal,
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
    SourceNotCurrent(ClaimId),
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
    PatternRequiresThreeIndependentEvents,
    PatternEventsMustSpanTime,
    PatternMissingCounterexampleReview,
    PatternSourceEvidenceNotFound(EvidenceId),
    PatternSourceEvidenceQuoteMismatch(EvidenceId),
    PatternCounterexampleReviewPredatesSupport(EvidenceId),
    PatternCounterexampleReviewNotFound(EvidenceId),
    PatternCounterexampleReviewQuoteMismatch(EvidenceId),
    PatternCounterexampleReviewNotFromCounterpart(EvidenceId),
    PatternReviewOnlyForPattern,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternMaturityRejectionReason {
    InvalidMemoryId,
    InvalidExpectedVersion,
    MemoryNotFound(MemoryId),
    StaleExpectedVersion {
        expected: u64,
        actual: u64,
    },
    MemoryNotProvisionalPattern(MemoryStatus),
    MissingNewSupport,
    TooManyNewSupport,
    DuplicateNewSupport(ClaimId),
    NewSupportNotFound(ClaimId),
    NewSupportNotCurrent(ClaimId),
    NewSupportCrossesLedger {
        claim_id: ClaimId,
        owner: ClaimOwner,
        subject: MemorySubject,
    },
    NewSupportEvidenceNotFound(EvidenceId),
    NewSupportEvidenceQuoteMismatch(EvidenceId),
    NoIndependentNewSupport,
    NewSupportPredatesPattern(EvidenceId),
    EmptyRationale,
    OversizedRationale,
    MissingCounterexampleReview,
    CounterexampleReviewNotFound(EvidenceId),
    EmptyCounterexampleReviewQuote(EvidenceId),
    CounterexampleReviewQuoteMismatch(EvidenceId),
    CounterexampleReviewNotFromCounterpart(EvidenceId),
    CounterexampleReviewPredatesPattern(EvidenceId),
    TooManyCounterEvidence,
    DuplicateCounterEvidence(EvidenceId),
    CounterEvidenceNotFound(EvidenceId),
    EmptyCounterEvidenceQuote(EvidenceId),
    CounterEvidenceQuoteMismatch(EvidenceId),
    MissingDiscussion,
    TooManyDiscussionEvidence,
    DuplicateDiscussionEvidence(EvidenceId),
    DiscussionEvidenceNotFound(EvidenceId),
    EmptyDiscussionEvidenceQuote(EvidenceId),
    DiscussionEvidenceQuoteMismatch(EvidenceId),
    DiscussionPredatesPattern(EvidenceId),
    DiscussionRequiresPerson,
    DiscussionRequiresCounterpart,
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
    InvalidPatternMaturity(PatternMaturityRejectionReason),
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

    /// Validates an explicit counterpart maturity proposal and atomically
    /// appends the supported-view successor version.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection when any independent-support, review,
    /// discussion, attribution, or version prerequisite is absent.
    pub fn mature_pattern(
        &mut self,
        proposal: &PatternMaturityProposal,
    ) -> Result<MemoryVersion, MemoryError> {
        let proposed_at = self.clock.now();
        commit_pattern_maturity(&mut self.repository, proposal, proposed_at)
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

/// Runs the single authoritative pattern-maturity qualification matrix and
/// atomically appends the supported-view successor at a trusted timestamp.
///
/// This entry point lets higher-level Core repository adapters reuse the same
/// domain service as [`MemoryMaintenance::mature_pattern`] without introducing
/// a dependency from Core back to the memory crate.
///
/// # Errors
///
/// Returns a typed qualification rejection or the underlying repository error.
pub fn commit_pattern_maturity<R: LongTermMemoryRepository>(
    repository: &mut R,
    proposal: &PatternMaturityProposal,
    proposed_at: Timestamp,
) -> Result<MemoryVersion, MemoryError> {
    let validated = validate_pattern_maturity(repository, proposal)?;
    repository
        .append_pattern_maturity(validated, proposed_at)
        .map_err(MemoryError::from)
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
        if claim.status() != ClaimStatus::Current {
            return Err(MemoryError::InvalidProposal(
                MemoryProposalRejectionReason::SourceNotCurrent(*claim_id),
            ));
        }
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
    if basis == MemoryBasis::PatternCandidate {
        validate_initial_pattern(
            repository,
            &claims,
            proposal.pattern_counterexample_review(),
        )?;
    } else if proposal.pattern_counterexample_review().is_some() {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternReviewOnlyForPattern,
        ));
    }
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
        proposal.pattern_counterexample_review().cloned(),
    ))
}

fn validate_initial_pattern<R: LongTermMemoryRepository>(
    repository: &R,
    claims: &[Claim],
    review: Option<&EvidenceCitation>,
) -> Result<(), MemoryError> {
    let mut events = BTreeMap::new();
    for claim in claims {
        for citation in claim.support() {
            let evidence = repository.evidence(citation.evidence_id())?.ok_or(
                MemoryError::InvalidProposal(
                    MemoryProposalRejectionReason::PatternSourceEvidenceNotFound(
                        citation.evidence_id(),
                    ),
                ),
            )?;
            if citation.quote().trim().is_empty() || !evidence.verbatim().contains(citation.quote())
            {
                return Err(MemoryError::InvalidProposal(
                    MemoryProposalRejectionReason::PatternSourceEvidenceQuoteMismatch(
                        citation.evidence_id(),
                    ),
                ));
            }
            events
                .entry(citation.evidence_id())
                .or_insert(evidence.recorded_at());
        }
    }
    if events.len() < 3 {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternRequiresThreeIndependentEvents,
        ));
    }
    let distinct_times = events
        .values()
        .map(|timestamp| timestamp.as_millis())
        .collect::<BTreeSet<_>>();
    if distinct_times.len() < 3 {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternEventsMustSpanTime,
        ));
    }
    let review = review.ok_or(MemoryError::InvalidProposal(
        MemoryProposalRejectionReason::PatternMissingCounterexampleReview,
    ))?;
    let evidence =
        repository
            .evidence(review.evidence_id())?
            .ok_or(MemoryError::InvalidProposal(
                MemoryProposalRejectionReason::PatternCounterexampleReviewNotFound(
                    review.evidence_id(),
                ),
            ))?;
    if review.quote().trim().is_empty() || !evidence.verbatim().contains(review.quote()) {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternCounterexampleReviewQuoteMismatch(
                review.evidence_id(),
            ),
        ));
    }
    if evidence.speaker() != Speaker::Counterpart {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternCounterexampleReviewNotFromCounterpart(
                review.evidence_id(),
            ),
        ));
    }
    let latest_support = events
        .values()
        .map(|timestamp| timestamp.as_millis())
        .max()
        .expect("three pattern events were established above");
    if evidence.recorded_at().as_millis() < latest_support {
        return Err(MemoryError::InvalidProposal(
            MemoryProposalRejectionReason::PatternCounterexampleReviewPredatesSupport(
                review.evidence_id(),
            ),
        ));
    }
    Ok(())
}

fn validate_pattern_maturity<R: LongTermMemoryRepository>(
    repository: &R,
    proposal: &PatternMaturityProposal,
) -> Result<ValidatedPatternMaturityProposal, MemoryError> {
    let (memory_id, memory) = validate_maturity_target(repository, proposal)?;
    validate_maturity_field_bounds(proposal)?;
    let all_sources = validate_maturity_new_support(repository, &memory, proposal)?;
    validate_maturity_counter_evidence(repository, proposal)?;
    let review = validate_maturity_review(repository, &memory, proposal)?;
    validate_maturity_discussion(repository, &memory, proposal)?;
    Ok(ValidatedPatternMaturityProposal::new(
        memory_id,
        proposal.expected_version(),
        proposal.new_support_claim_ids().to_vec(),
        all_sources,
        proposal.counter_evidence_refs().to_vec(),
        review,
        proposal.discussion_evidence_refs().to_vec(),
        proposal.rationale().to_owned(),
    ))
}

fn validate_maturity_target<R: LongTermMemoryRepository>(
    repository: &R,
    proposal: &PatternMaturityProposal,
) -> Result<(MemoryId, MemoryVersion), MemoryError> {
    let memory_id = MemoryId::new(proposal.memory_id()).ok_or(
        MemoryError::InvalidPatternMaturity(PatternMaturityRejectionReason::InvalidMemoryId),
    )?;
    if proposal.expected_version() == 0 {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::InvalidExpectedVersion,
        ));
    }
    let memory =
        repository
            .current_memory(memory_id)?
            .ok_or(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::MemoryNotFound(memory_id),
            ))?;
    if memory.version() != proposal.expected_version() {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::StaleExpectedVersion {
                expected: proposal.expected_version(),
                actual: memory.version(),
            },
        ));
    }
    if memory.status() != MemoryStatus::ProvisionalPattern
        || memory.basis() != MemoryBasis::PatternCandidate
    {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::MemoryNotProvisionalPattern(memory.status()),
        ));
    }
    Ok((memory_id, memory))
}

fn validate_maturity_field_bounds(proposal: &PatternMaturityProposal) -> Result<(), MemoryError> {
    if proposal.rationale().trim().is_empty() {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::EmptyRationale,
        ));
    }
    if proposal.rationale().len() > MAX_MEMORY_TEXT_BYTES {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::OversizedRationale,
        ));
    }
    if proposal.new_support_claim_ids().is_empty() {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::MissingNewSupport,
        ));
    }
    if proposal.new_support_claim_ids().len() > MAX_MEMORY_SOURCES {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::TooManyNewSupport,
        ));
    }
    Ok(())
}

fn validate_maturity_new_support<R: LongTermMemoryRepository>(
    repository: &R,
    memory: &MemoryVersion,
    proposal: &PatternMaturityProposal,
) -> Result<Vec<ClaimId>, MemoryError> {
    let expected_owner = match memory.subject() {
        MemorySubject::Person => ClaimOwner::Person,
        MemorySubject::Counterpart => ClaimOwner::Counterpart,
        MemorySubject::Shared => ClaimOwner::Shared,
    };
    let mut base_events = BTreeSet::new();
    for claim_id in memory.source_claim_ids() {
        if let Some(claim) = repository.claim(*claim_id)? {
            for citation in claim.support() {
                base_events.insert(citation.evidence_id());
            }
        }
    }
    let mut unique_claims = BTreeSet::new();
    let mut independent_new_events = BTreeSet::new();
    for claim_id in proposal.new_support_claim_ids() {
        if !unique_claims.insert(*claim_id) {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::DuplicateNewSupport(*claim_id),
            ));
        }
        let claim = repository
            .claim(*claim_id)?
            .ok_or(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::NewSupportNotFound(*claim_id),
            ))?;
        if claim.status() != ClaimStatus::Current {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::NewSupportNotCurrent(*claim_id),
            ));
        }
        if claim.owner() != expected_owner {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::NewSupportCrossesLedger {
                    claim_id: *claim_id,
                    owner: claim.owner(),
                    subject: memory.subject(),
                },
            ));
        }
        for citation in claim.support() {
            let evidence = repository.evidence(citation.evidence_id())?.ok_or(
                MemoryError::InvalidPatternMaturity(
                    PatternMaturityRejectionReason::NewSupportEvidenceNotFound(
                        citation.evidence_id(),
                    ),
                ),
            )?;
            if citation.quote().trim().is_empty() || !evidence.verbatim().contains(citation.quote())
            {
                return Err(MemoryError::InvalidPatternMaturity(
                    PatternMaturityRejectionReason::NewSupportEvidenceQuoteMismatch(
                        citation.evidence_id(),
                    ),
                ));
            }
            if !base_events.contains(&citation.evidence_id()) {
                if evidence.recorded_at().as_millis() <= memory.formed_at().as_millis() {
                    return Err(MemoryError::InvalidPatternMaturity(
                        PatternMaturityRejectionReason::NewSupportPredatesPattern(
                            citation.evidence_id(),
                        ),
                    ));
                }
                independent_new_events.insert(citation.evidence_id());
            }
        }
    }
    if independent_new_events.is_empty() {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::NoIndependentNewSupport,
        ));
    }
    let mut all_sources = memory.source_claim_ids().to_vec();
    for claim_id in proposal.new_support_claim_ids() {
        if !all_sources.contains(claim_id) {
            all_sources.push(*claim_id);
        }
    }
    Ok(all_sources)
}

fn validate_maturity_counter_evidence<R: LongTermMemoryRepository>(
    repository: &R,
    proposal: &PatternMaturityProposal,
) -> Result<(), MemoryError> {
    if proposal.counter_evidence_refs().len() > MAX_DISPUTE_EVIDENCE {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::TooManyCounterEvidence,
        ));
    }
    let mut counter_ids = BTreeSet::new();
    for citation in proposal.counter_evidence_refs() {
        if !counter_ids.insert(citation.evidence_id()) {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::DuplicateCounterEvidence(citation.evidence_id()),
            ));
        }
        let evidence = repository.evidence(citation.evidence_id())?.ok_or(
            MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::CounterEvidenceNotFound(citation.evidence_id()),
            ),
        )?;
        if citation.quote().trim().is_empty() {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::EmptyCounterEvidenceQuote(citation.evidence_id()),
            ));
        }
        if !evidence.verbatim().contains(citation.quote()) {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::CounterEvidenceQuoteMismatch(
                    citation.evidence_id(),
                ),
            ));
        }
    }
    Ok(())
}

fn validate_maturity_review<R: LongTermMemoryRepository>(
    repository: &R,
    memory: &MemoryVersion,
    proposal: &PatternMaturityProposal,
) -> Result<EvidenceCitation, MemoryError> {
    let review =
        proposal
            .counterexample_review_ref()
            .ok_or(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::MissingCounterexampleReview,
            ))?;
    let review_evidence =
        repository
            .evidence(review.evidence_id())?
            .ok_or(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::CounterexampleReviewNotFound(review.evidence_id()),
            ))?;
    if review.quote().trim().is_empty() {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::EmptyCounterexampleReviewQuote(review.evidence_id()),
        ));
    }
    if !review_evidence.verbatim().contains(review.quote()) {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::CounterexampleReviewQuoteMismatch(review.evidence_id()),
        ));
    }
    if review_evidence.speaker() != Speaker::Counterpart {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::CounterexampleReviewNotFromCounterpart(
                review.evidence_id(),
            ),
        ));
    }
    if review_evidence.recorded_at().as_millis() <= memory.formed_at().as_millis() {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::CounterexampleReviewPredatesPattern(
                review.evidence_id(),
            ),
        ));
    }
    Ok(review.clone())
}

fn validate_maturity_discussion<R: LongTermMemoryRepository>(
    repository: &R,
    memory: &MemoryVersion,
    proposal: &PatternMaturityProposal,
) -> Result<(), MemoryError> {
    if proposal.discussion_evidence_refs().is_empty() {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::MissingDiscussion,
        ));
    }
    if proposal.discussion_evidence_refs().len() > MAX_DISPUTE_EVIDENCE {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::TooManyDiscussionEvidence,
        ));
    }
    let mut discussion_ids = BTreeSet::new();
    let mut has_person = false;
    let mut has_counterpart = false;
    for citation in proposal.discussion_evidence_refs() {
        if !discussion_ids.insert(citation.evidence_id()) {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::DuplicateDiscussionEvidence(citation.evidence_id()),
            ));
        }
        let evidence = repository.evidence(citation.evidence_id())?.ok_or(
            MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::DiscussionEvidenceNotFound(citation.evidence_id()),
            ),
        )?;
        if citation.quote().trim().is_empty() {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::EmptyDiscussionEvidenceQuote(
                    citation.evidence_id(),
                ),
            ));
        }
        if !evidence.verbatim().contains(citation.quote()) {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::DiscussionEvidenceQuoteMismatch(
                    citation.evidence_id(),
                ),
            ));
        }
        if evidence.recorded_at().as_millis() <= memory.formed_at().as_millis() {
            return Err(MemoryError::InvalidPatternMaturity(
                PatternMaturityRejectionReason::DiscussionPredatesPattern(citation.evidence_id()),
            ));
        }
        match evidence.speaker() {
            Speaker::Person => has_person = true,
            Speaker::Counterpart => has_counterpart = true,
        }
    }
    if !has_person {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::DiscussionRequiresPerson,
        ));
    }
    if !has_counterpart {
        return Err(MemoryError::InvalidPatternMaturity(
            PatternMaturityRejectionReason::DiscussionRequiresCounterpart,
        ));
    }
    Ok(())
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
        MemoryStatus::Active
            | MemoryStatus::Provisional
            | MemoryStatus::ProvisionalPattern
            | MemoryStatus::SupportedCounterpartView
            | MemoryStatus::Weakened
    ) {
        return Err(MemoryError::InvalidDispute(
            MemoryDisputeRejectionReason::MemoryNotDisputable(memory.status()),
        ));
    }
    if let Some(open) = repository
        .memory_disputes(memory.id())?
        .into_iter()
        .find(|dispute| {
            dispute.memory_version() == memory.version()
                && dispute.outcome() == MemoryDisputeOutcome::Open
        })
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
        MemoryDisputeReviewDecision::Weaken => (MemoryDisputeOutcome::Weakened, None),
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
