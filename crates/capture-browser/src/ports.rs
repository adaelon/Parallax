use eam_core::RepositoryError;
use eam_desktop_host::HostSessionId;

use crate::{BrowserCaptureReceipt, BrowserSubmission, BrowserVisit};

pub trait BrowserCaptureRepository {
    /// Atomically records one idempotent browser submission under the current host session.
    ///
    /// Optional page text remains untrusted evidence and must be archived before
    /// its visit row references it.
    ///
    /// # Errors
    ///
    /// Returns an adapter error for a stale host session, conflicting retry,
    /// corrupt persisted state, or a transaction failure.
    fn record_browser_submission(
        &mut self,
        host_session_id: HostSessionId,
        submission: &BrowserSubmission,
    ) -> Result<BrowserCaptureReceipt, RepositoryError>;

    /// Loads the encrypted browser timeline in deterministic insertion order.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted state is invalid.
    fn all_browser_visits(&self) -> Result<Vec<BrowserVisit>, RepositoryError>;
}
