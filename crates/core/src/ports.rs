use std::{error::Error, fmt};

use crate::{
    Claim, ClaimId, ConversationEvidence, EvidenceId, PersonTurnClassification, RuntimeRequest,
    RuntimeResponse, Timestamp,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    message: String,
}

impl RuntimeError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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

pub trait Clock {
    fn now(&mut self) -> Timestamp;
}
