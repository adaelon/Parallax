use std::{error::Error, fmt};

use eam_core::Timestamp;
use eam_desktop_host::{HostGapReason, HostSessionId};

pub const MAX_APPLICATION_BYTES: usize = 1_024;
pub const MAX_WINDOW_TITLE_BYTES: usize = 16 * 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaptureSpanId(u64);

impl CaptureSpanId {
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleState {
    Active,
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivitySnapshot {
    application: String,
    window_title: String,
    idle_state: IdleState,
}

impl ActivitySnapshot {
    /// Creates one bounded metadata-only foreground observation.
    ///
    /// # Errors
    ///
    /// Rejects an empty application identity or metadata beyond the fixed
    /// capture limits.
    pub fn new(
        application: impl Into<String>,
        window_title: impl Into<String>,
        idle_state: IdleState,
    ) -> Result<Self, CaptureError> {
        let application = application.into();
        let window_title = window_title.into();
        if application.trim().is_empty() {
            return Err(CaptureError::InvalidMetadata("application is empty"));
        }
        if application.len() > MAX_APPLICATION_BYTES {
            return Err(CaptureError::InvalidMetadata("application is too large"));
        }
        if window_title.len() > MAX_WINDOW_TITLE_BYTES {
            return Err(CaptureError::InvalidMetadata("window title is too large"));
        }
        Ok(Self {
            application,
            window_title,
            idle_state,
        })
    }

    #[must_use]
    pub fn application(&self) -> &str {
        &self.application
    }

    #[must_use]
    pub fn window_title(&self) -> &str {
        &self.window_title
    }

    #[must_use]
    pub const fn idle_state(&self) -> IdleState {
        self.idle_state
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureGapReason {
    Paused,
    SessionLocked,
    ExplicitExit,
    Update,
    Crash,
    SourceUnavailable,
}

impl From<HostGapReason> for CaptureGapReason {
    fn from(value: HostGapReason) -> Self {
        match value {
            HostGapReason::Crash => Self::Crash,
            HostGapReason::ExplicitExit => Self::ExplicitExit,
            HostGapReason::Update => Self::Update,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureSpanKind {
    Activity(ActivitySnapshot),
    Gap(CaptureGapReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureCheckpoint {
    observed_at: Timestamp,
    kind: CaptureSpanKind,
    begins_new_span: bool,
}

impl CaptureCheckpoint {
    #[must_use]
    pub const fn new(observed_at: Timestamp, kind: CaptureSpanKind, begins_new_span: bool) -> Self {
        Self {
            observed_at,
            kind,
            begins_new_span,
        }
    }

    #[must_use]
    pub const fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    #[must_use]
    pub const fn kind(&self) -> &CaptureSpanKind {
        &self.kind
    }

    #[must_use]
    pub const fn begins_new_span(&self) -> bool {
        self.begins_new_span
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    Collecting,
    Paused,
    Locked,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    ExplicitExit,
    Update,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureRecovery {
    mode: CaptureMode,
    open_kind: Option<CaptureSpanKind>,
}

impl CaptureRecovery {
    #[must_use]
    pub const fn new(mode: CaptureMode, open_kind: Option<CaptureSpanKind>) -> Self {
        Self { mode, open_kind }
    }

    #[must_use]
    pub const fn mode(&self) -> CaptureMode {
        self.mode
    }

    #[must_use]
    pub const fn open_kind(&self) -> Option<&CaptureSpanKind> {
        self.open_kind.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureSpan {
    id: CaptureSpanId,
    started_in_host_session: HostSessionId,
    kind: CaptureSpanKind,
    started_at: Timestamp,
    observed_until: Timestamp,
    ended_at: Option<Timestamp>,
}

impl CaptureSpan {
    #[must_use]
    pub fn restore(
        id: CaptureSpanId,
        started_in_host_session: HostSessionId,
        kind: CaptureSpanKind,
        started_at: Timestamp,
        observed_until: Timestamp,
        ended_at: Option<Timestamp>,
    ) -> Self {
        Self {
            id,
            started_in_host_session,
            kind,
            started_at,
            observed_until,
            ended_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> CaptureSpanId {
        self.id
    }

    #[must_use]
    pub const fn started_in_host_session(&self) -> HostSessionId {
        self.started_in_host_session
    }

    #[must_use]
    pub const fn kind(&self) -> &CaptureSpanKind {
        &self.kind
    }

    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.started_at
    }

    #[must_use]
    pub const fn observed_until(&self) -> Timestamp {
        self.observed_until
    }

    #[must_use]
    pub const fn ended_at(&self) -> Option<Timestamp> {
        self.ended_at
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    InvalidMetadata(&'static str),
    InvalidTransition {
        from: CaptureMode,
        action: &'static str,
    },
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for CaptureError {}
