use eam_core::{RepositoryError, RuntimeError, SessionId, Timestamp};

use crate::{
    IdentityStateVersion, InitialIdentityProposal, InitialIdentityRequest, InitialSelfIntroduction,
    IntroductionAnswer, SelfBundleState, SelfBundleVersion, WakeTrigger,
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

    /// Loads the complete immutable identity chain in ascending version order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when any persisted version or evidence link is invalid.
    fn all_identity_states(&self) -> Result<Vec<IdentityStateVersion>, RepositoryError>;
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

pub trait SelfBundleRepository {
    /// Appends one complete immutable Self Bundle version atomically.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when version continuity, referenced state, or
    /// the all-or-nothing persistence contract is violated.
    fn append_self_bundle(&mut self, bundle: SelfBundleVersion) -> Result<(), RepositoryError>;

    /// Loads the latest complete Self Bundle version, if one exists.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted bundle state is incomplete or
    /// cannot be decoded.
    fn current_self_bundle(&self) -> Result<Option<SelfBundleVersion>, RepositoryError>;
}

/// Executes bounded wake-cycle work without receiving repository access.
///
/// This is orchestration work, not the S06 model runtime gateway. Each phase
/// receives a complete immutable state and must return a complete candidate;
/// the trusted coordinator validates identity and constitutional boundaries.
pub trait WakeWork {
    /// Observes the wake trigger and returns the complete candidate state.
    ///
    /// # Errors
    ///
    /// Returns a work error when observation cannot complete.
    fn observe(
        &mut self,
        trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError>;

    /// Performs bounded reasoning and returns the complete candidate state.
    ///
    /// # Errors
    ///
    /// Returns a work error when thinking cannot complete.
    fn think(
        &mut self,
        trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError>;

    /// Produces the bounded response-stage state.
    ///
    /// # Errors
    ///
    /// Returns a work error when response work cannot complete.
    fn respond(
        &mut self,
        trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError>;
}
