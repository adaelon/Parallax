use std::{
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    Claim, ClaimCorrectionReceipt, ClaimId, ConversationEvidence, EvidenceId, ForgetReceipt,
    ForgetTarget, PersonTurnClassification, RuntimeRequest, RuntimeResponse,
    SharedAgreementCandidate, SharedAgreementCandidateId, SharedAgreementDecision,
    SharedAgreementResolution, SharedExperience, Timestamp,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryError {
    message: String,
}

impl RepositoryError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RepositoryError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    Timeout,
    Unavailable,
    InvalidResponse,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    kind: RuntimeErrorKind,
    message: String,
}

impl RuntimeError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: RuntimeErrorKind::Other,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            kind: RuntimeErrorKind::Timeout,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: RuntimeErrorKind::Unavailable,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self {
            kind: RuntimeErrorKind::InvalidResponse,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> RuntimeErrorKind {
        self.kind
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RuntimeError {}

pub trait MemoryRepository {
    fn next_evidence_id(&mut self) -> EvidenceId;
    fn next_claim_id(&mut self) -> ClaimId;

    /// Appends immutable conversation evidence.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the evidence cannot be appended exactly once.
    fn append_evidence(&mut self, evidence: ConversationEvidence) -> Result<(), RepositoryError>;

    /// Appends an immutable ledger claim.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the claim cannot be appended exactly once.
    fn append_claim(&mut self, claim: Claim) -> Result<(), RepositoryError>;

    /// Resolves one evidence identifier.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the backing store cannot be queried.
    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError>;

    /// Returns all evidence in append order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the backing store cannot be queried.
    fn all_evidence(&self) -> Result<Vec<ConversationEvidence>, RepositoryError>;

    /// Returns all claims in append order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the backing store cannot be queried.
    fn all_claims(&self) -> Result<Vec<Claim>, RepositoryError>;
}

pub trait ClaimCorrectionRepository: MemoryRepository {
    /// Resolves one claim together with its current temporal state.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the ledger cannot be queried.
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError>;

    /// Atomically appends correction evidence and a successor person claim,
    /// marks its predecessor superseded, and propagates affected derived state.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without leaving a partial correction.
    fn commit_person_fact_correction(
        &mut self,
        evidence: ConversationEvidence,
        replacement: Claim,
    ) -> Result<ClaimCorrectionReceipt, RepositoryError>;
}

pub trait ForgetRepository: MemoryRepository {
    /// Atomically persists a deletion intent and removes the complete active
    /// authority/derived closure for its target. A repeated committed target
    /// returns the original receipt; `None` means it never existed.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without exposing a partially forgotten target.
    fn commit_forget(
        &mut self,
        target: ForgetTarget,
        requested_at: Timestamp,
    ) -> Result<Option<ForgetReceipt>, RepositoryError>;
}

pub trait SharedExperienceRepository: MemoryRepository {
    fn next_shared_agreement_candidate_id(&mut self) -> SharedAgreementCandidateId;

    /// Persists a candidate outside the shared ledger until the person resolves
    /// its confirmation ceremony.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without admitting a shared claim.
    fn stage_shared_agreement_candidate(
        &mut self,
        candidate: SharedAgreementCandidate,
    ) -> Result<(), RepositoryError>;

    /// Resolves one candidate together with its immutable evidence.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the candidate store cannot be queried.
    fn shared_agreement_candidate(
        &self,
        id: SharedAgreementCandidateId,
    ) -> Result<Option<SharedAgreementCandidate>, RepositoryError>;

    /// Atomically records a person's confirmation or deferral. Confirmation
    /// appends the supplied shared claim and experience; deferral appends none.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without partially resolving the candidate.
    fn commit_shared_agreement_decision(
        &mut self,
        id: SharedAgreementCandidateId,
        decision: SharedAgreementDecision,
        confirmed: Option<SharedExperience>,
        decided_at: Timestamp,
    ) -> Result<SharedAgreementResolution, RepositoryError>;

    /// Atomically appends a non-veto shared experience and its shared claim.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without a partial ledger entry.
    fn commit_shared_experience(
        &mut self,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError>;

    /// Returns every candidate in identifier order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the candidate store cannot be queried.
    fn all_shared_agreement_candidates(
        &self,
    ) -> Result<Vec<SharedAgreementCandidate>, RepositoryError>;

    /// Returns every admitted shared experience in claim order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the shared ledger cannot be queried.
    fn all_shared_experiences(&self) -> Result<Vec<SharedExperience>, RepositoryError>;

    /// Dismisses only the ceremonial notice; the shared claim remains immutable.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without modifying shared history.
    fn dismiss_shared_experience_ceremony(
        &mut self,
        claim_id: ClaimId,
    ) -> Result<bool, RepositoryError>;
}

/// Runtime implementations receive only typed values selected by the trusted
/// core. The repository is intentionally absent from both method signatures.
pub trait CounterpartRuntime {
    /// Classifies a person turn without receiving repository access.
    ///
    /// # Errors
    ///
    /// Returns a runtime error when no structured classification can be produced.
    fn classify_person_turn(
        &mut self,
        evidence: &ConversationEvidence,
    ) -> Result<PersonTurnClassification, RuntimeError>;

    /// Produces free text and optional structured operations from a frozen request.
    ///
    /// # Errors
    ///
    /// Returns a runtime error when the runtime cannot produce a response.
    fn respond(&mut self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError>;
}

impl<T> CounterpartRuntime for Box<T>
where
    T: CounterpartRuntime + ?Sized,
{
    fn classify_person_turn(
        &mut self,
        evidence: &ConversationEvidence,
    ) -> Result<PersonTurnClassification, RuntimeError> {
        self.as_mut().classify_person_turn(evidence)
    }

    fn respond(&mut self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError> {
        self.as_mut().respond(request)
    }
}

pub trait Clock {
    fn now(&mut self) -> Timestamp;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&mut self) -> Timestamp {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let millis = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        Timestamp::from_millis(millis)
    }
}
