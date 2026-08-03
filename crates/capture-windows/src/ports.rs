use eam_core::{RepositoryError, Timestamp};
use eam_desktop_host::{HostGapReason, HostSessionId};

use crate::{CaptureCheckpoint, CaptureRecovery, CaptureSpan};

pub trait ActivityTimelineRepository {
    /// Recovers the last open capture span without inventing activity.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted state is invalid or recovery
    /// cannot be committed atomically.
    fn recover_capture_timeline(
        &mut self,
        host_session_id: HostSessionId,
        started_at: Timestamp,
        recovered_host_gap: Option<HostGapReason>,
    ) -> Result<CaptureRecovery, RepositoryError>;

    /// Checkpoints one observed activity or explicit gap transition.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the current open span is invalid or the
    /// checkpoint cannot be committed atomically.
    fn record_capture_checkpoint(
        &mut self,
        host_session_id: HostSessionId,
        checkpoint: &CaptureCheckpoint,
    ) -> Result<CaptureSpan, RepositoryError>;

    /// Loads the complete activity and capture-gap timeline.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted state cannot be decoded.
    fn all_capture_spans(&self) -> Result<Vec<CaptureSpan>, RepositoryError>;
}
