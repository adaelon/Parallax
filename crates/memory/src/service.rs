use std::{collections::BTreeSet, error::Error, fmt};

use eam_core::{ApplicableTime, Claim, ClaimId, ClaimOwner, Clock, RepositoryError, Uncertainty};

use crate::{
    LongTermMemoryRepository, MAX_MEMORY_SOURCES, MAX_MEMORY_TEXT_BYTES, MemoryBasis,
    MemoryConfidence, MemoryId, MemoryProposal, MemoryStatus, MemorySubject, MemoryTarget,
    MemoryVersion, ValidatedMemoryProposal,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryError {
    InvalidProposal(MemoryProposalRejectionReason),
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
