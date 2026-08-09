use std::{
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    Claim, ClaimCorrectionReceipt, ClaimId, ConversationEvidence, CounterpartReadiness,
    EvidenceCitation, EvidenceId, ForgetReceipt, ForgetTarget, IdentityRevisionCommit,
    IdentityRevisionReceipt, IdentityRuntimeContext, IdentityStateSnapshot,
    PatternMaturityCommitOutcome, PatternMaturityProposal, PersonFactProposalBatch,
    ReflectionInvitation, ReflectionInvitationId, ReflectionInvitationReceipt,
    ReflectionInvitationState, RuntimeRequest, RuntimeResponse, SelfBundleSnapshot,
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

    /// Passes one runtime-authored maturity proposal to the repository's
    /// trusted long-term-memory domain adapter.
    ///
    /// Repositories without that adapter reject closed by default. A concrete
    /// adapter must reuse the memory domain's qualification service rather
    /// than reproduce its eligibility matrix in Core.
    ///
    /// # Errors
    ///
    /// Returns an adapter error only when trusted persistence fails; domain
    /// qualification failures are represented by the returned outcome.
    fn commit_pattern_maturity(
        &mut self,
        _proposal: &PatternMaturityProposal,
        _proposed_at: Timestamp,
    ) -> Result<PatternMaturityCommitOutcome, RepositoryError> {
        Ok(PatternMaturityCommitOutcome::QualificationRejected)
    }
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

pub trait IdentityEvolutionRepository: MemoryRepository {
    /// Re-derives the formal-conversation gate from persisted introduction,
    /// identity, and Self Bundle facts.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted readiness facts cannot be read.
    fn conversation_readiness(&self) -> Result<CounterpartReadiness, RepositoryError>;

    /// Loads the current immutable identity together with the Self Bundle and
    /// constitution versions that make it portable across model runtimes.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when identity and Self Bundle pointers are
    /// incomplete, inconsistent, or cannot be decoded.
    fn current_identity_context(&self) -> Result<Option<IdentityRuntimeContext>, RepositoryError>;

    /// Loads the current immutable Self Bundle projection without resolving
    /// belief references or selecting turn-relevant experiences.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the current bundle is missing children,
    /// structurally invalid, or cannot be decoded.
    fn current_self_bundle_snapshot(&self) -> Result<Option<SelfBundleSnapshot>, RepositoryError>;

    /// Resolves one Self Bundle belief reference to its authoritative claim.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the ledger cannot be queried. A missing
    /// claim is represented by `None` so Core can fail the context closed.
    fn counterpart_belief(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError>;

    /// Atomically appends one validated identity state and advances the Self
    /// Bundle to that exact version.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without either write when the current identity,
    /// Self Bundle, constitution, evidence, or immutable chain changed.
    fn commit_identity_revision(
        &mut self,
        revision: IdentityRevisionCommit,
    ) -> Result<IdentityRevisionReceipt, RepositoryError>;

    /// Returns the immutable identity chain in ascending version order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when any persisted version cannot be decoded.
    fn identity_history(&self) -> Result<Vec<IdentityStateSnapshot>, RepositoryError>;
}

pub trait ReflectionInvitationRepository: MemoryRepository {
    fn next_reflection_invitation_id(&mut self) -> ReflectionInvitationId;

    /// Atomically appends one validated pending invitation without replacing
    /// another open invitation for the same topic.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without writing when evidence, identity,
    /// uniqueness, state, or the G08 open-invitation budget is invalid.
    fn commit_reflection_invitation(
        &mut self,
        invitation: ReflectionInvitation,
    ) -> Result<ReflectionInvitationReceipt, RepositoryError>;

    /// Compare-and-swaps one invitation state while preserving immutable fields.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without writing when current state or immutable
    /// fields differ from the supplied transition.
    fn transition_reflection_invitation(
        &mut self,
        expected_state: ReflectionInvitationState,
        invitation: ReflectionInvitation,
    ) -> Result<ReflectionInvitationReceipt, RepositoryError>;

    /// Resolves one current invitation snapshot.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when storage cannot be queried.
    fn reflection_invitation(
        &self,
        id: ReflectionInvitationId,
    ) -> Result<Option<ReflectionInvitation>, RepositoryError>;

    /// Returns every invitation in identifier order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted state cannot be decoded.
    fn all_reflection_invitations(&self) -> Result<Vec<ReflectionInvitation>, RepositoryError>;
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

    /// Atomically records a person's structured revision evidence, retires the
    /// previous signable version, and appends the new immutable candidate.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without partially changing either version.
    fn commit_shared_agreement_revision(
        &mut self,
        previous_id: SharedAgreementCandidateId,
        person_evidence: ConversationEvidence,
        revised: SharedAgreementCandidate,
        revised_at: Timestamp,
    ) -> Result<(), RepositoryError>;

    /// Atomically attaches the counterpart's exact assent evidence to one
    /// immutable version and makes that version eligible for person signing.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without partially changing candidate state.
    fn commit_counterpart_agreement_assent(
        &mut self,
        id: SharedAgreementCandidateId,
        version: u64,
        citation: EvidenceCitation,
        assented_at: Timestamp,
    ) -> Result<SharedAgreementCandidate, RepositoryError>;

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

    /// Atomically appends a reasoned departure from one active agreement as a
    /// shared claim plus its typed relationship-history record.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without a partial breach record.
    fn commit_relational_constraint_departure(
        &mut self,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError>;

    /// Atomically appends a prospective agreement withdrawal and its shared
    /// history claim. Person withdrawals also append the supplied confirmation
    /// evidence in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without partially ending the agreement.
    fn commit_agreement_withdrawal(
        &mut self,
        person_confirmation: Option<ConversationEvidence>,
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
    /// Proposes zero or more atomic person facts without receiving repository access.
    ///
    /// # Errors
    ///
    /// Returns a runtime error when no bounded structured proposal batch can be produced.
    fn propose_person_facts(
        &mut self,
        evidence: &ConversationEvidence,
    ) -> Result<PersonFactProposalBatch, RuntimeError>;

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
    fn propose_person_facts(
        &mut self,
        evidence: &ConversationEvidence,
    ) -> Result<PersonFactProposalBatch, RuntimeError> {
        self.as_mut().propose_person_facts(evidence)
    }

    fn respond(&mut self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError> {
        self.as_mut().respond(request)
    }
}

pub trait Clock {
    fn now(&mut self) -> Timestamp;
}

impl<C> Clock for &mut C
where
    C: Clock + ?Sized,
{
    fn now(&mut self) -> Timestamp {
        (**self).now()
    }
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
