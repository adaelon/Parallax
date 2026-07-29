use eam_core::{RepositoryError, Timestamp};

use crate::{ExitReason, HostRuntimeGap, HostSession, HostSessionId, HostSessionStart, LaunchMode};

pub trait HostLifecycleRepository {
    /// Starts one host session and atomically recovers at most one preceding gap.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted lifecycle state is invalid or
    /// the session and recovered gap cannot be committed together.
    fn begin_host_session(
        &mut self,
        started_at: Timestamp,
        launch_mode: LaunchMode,
    ) -> Result<HostSessionStart, RepositoryError>;

    /// Advances the last confirmed live time for the current open session.
    ///
    /// # Errors
    ///
    /// Rejects a stale, unknown, or already closed session.
    fn heartbeat_host_session(
        &mut self,
        session_id: HostSessionId,
        observed_at: Timestamp,
    ) -> Result<HostSession, RepositoryError>;

    /// Closes the current session before secure Core shutdown.
    ///
    /// # Errors
    ///
    /// Rejects a stale, unknown, or already closed session.
    fn finish_host_session(
        &mut self,
        session_id: HostSessionId,
        ended_at: Timestamp,
        reason: ExitReason,
    ) -> Result<HostSession, RepositoryError>;

    /// Loads all host sessions in append order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted state cannot be decoded.
    fn all_host_sessions(&self) -> Result<Vec<HostSession>, RepositoryError>;

    /// Loads all recovered runtime gaps in append order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted state cannot be decoded.
    fn all_host_runtime_gaps(&self) -> Result<Vec<HostRuntimeGap>, RepositoryError>;
}
