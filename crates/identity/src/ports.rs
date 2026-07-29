use eam_core::{RepositoryError, RuntimeError, SessionId, Timestamp};

use crate::{
    IdentityStateVersion, InitialIdentityProposal, InitialIdentityRequest, InitialSelfIntroduction,
    IntroductionAnswer,
};

pub trait IdentityRepository {
    /// Atomically appends the complete introduction as person evidence and facts.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the introduction already exists or the
    /// backing store cannot commit every category and its fact together.
    fn record_initial_self_introduction(
        &mut self,
        session_id: &SessionId,
        answers: &[IntroductionAnswer],
        recorded_at: Timestamp,
    ) -> Result<InitialSelfIntroduction, RepositoryError>;

    /// Loads the one recorded initial introduction, if present.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted introduction state is invalid or
    /// cannot be read.
    fn initial_self_introduction(&self)
    -> Result<Option<InitialSelfIntroduction>, RepositoryError>;

    /// Appends one immutable identity version.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the version violates persistence
    /// constraints or cannot be committed with its evidence references.
    fn append_identity_state(
        &mut self,
        identity: IdentityStateVersion,
    ) -> Result<(), RepositoryError>;

    /// Loads the latest immutable identity version, if one exists.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when identity state or its evidence references
    /// cannot be decoded.
    fn current_identity_state(&self) -> Result<Option<IdentityStateVersion>, RepositoryError>;
}

pub trait IdentityRuntime {
    /// Produces a structured counterpart-authored proposal from introduction evidence.
    ///
    /// # Errors
    ///
    /// Returns a runtime error when no structured proposal can be produced.
    fn form_initial_identity(
        &mut self,
        request: InitialIdentityRequest,
    ) -> Result<InitialIdentityProposal, RuntimeError>;
}
