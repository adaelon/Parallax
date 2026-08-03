use eam_core::Timestamp;

use crate::{
    ActivitySnapshot, CaptureCheckpoint, CaptureError, CaptureGapReason, CaptureMode,
    CaptureRecovery, CaptureSpanKind, ShutdownReason,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureStateMachine {
    mode: CaptureMode,
    resume_after_unlock: CaptureMode,
    current: Option<CaptureSpanKind>,
}

impl Default for CaptureStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureStateMachine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: CaptureMode::Collecting,
            resume_after_unlock: CaptureMode::Collecting,
            current: None,
        }
    }

    #[must_use]
    pub fn restore(recovery: &CaptureRecovery) -> Self {
        Self {
            mode: recovery.mode(),
            resume_after_unlock: CaptureMode::Collecting,
            current: recovery.open_kind().cloned(),
        }
    }

    #[must_use]
    pub const fn mode(&self) -> CaptureMode {
        self.mode
    }

    /// Records one bounded foreground observation while collection is active.
    ///
    /// # Errors
    ///
    /// Rejects observations after the state machine has stopped.
    pub fn observe(
        &mut self,
        snapshot: ActivitySnapshot,
        observed_at: Timestamp,
    ) -> Result<Option<CaptureCheckpoint>, CaptureError> {
        match self.mode {
            CaptureMode::Collecting => Ok(Some(
                self.checkpoint(CaptureSpanKind::Activity(snapshot), observed_at),
            )),
            CaptureMode::Paused | CaptureMode::Locked => Ok(None),
            CaptureMode::Stopped => Err(self.invalid("observe")),
        }
    }

    /// Records an explicit source gap while collection is active.
    ///
    /// # Errors
    ///
    /// Rejects signals after the state machine has stopped.
    pub fn source_unavailable(
        &mut self,
        observed_at: Timestamp,
    ) -> Result<Option<CaptureCheckpoint>, CaptureError> {
        match self.mode {
            CaptureMode::Collecting => Ok(Some(self.checkpoint(
                CaptureSpanKind::Gap(CaptureGapReason::SourceUnavailable),
                observed_at,
            ))),
            CaptureMode::Paused | CaptureMode::Locked => Ok(None),
            CaptureMode::Stopped => Err(self.invalid("source_unavailable")),
        }
    }

    /// Enters or maintains the person-controlled paused state.
    ///
    /// # Errors
    ///
    /// Rejects pause after the state machine has stopped.
    pub fn pause(&mut self, at: Timestamp) -> Result<CaptureCheckpoint, CaptureError> {
        match self.mode {
            CaptureMode::Collecting | CaptureMode::Paused => {
                self.mode = CaptureMode::Paused;
                Ok(self.checkpoint(CaptureSpanKind::Gap(CaptureGapReason::Paused), at))
            }
            CaptureMode::Locked => {
                self.resume_after_unlock = CaptureMode::Paused;
                Ok(self.checkpoint(CaptureSpanKind::Gap(CaptureGapReason::SessionLocked), at))
            }
            CaptureMode::Stopped => Err(self.invalid("pause")),
        }
    }

    /// Resumes collection with an immediate foreground observation.
    ///
    /// # Errors
    ///
    /// Rejects resume unless capture is paused or session-locked.
    pub fn resume(
        &mut self,
        snapshot: ActivitySnapshot,
        at: Timestamp,
    ) -> Result<CaptureCheckpoint, CaptureError> {
        match self.mode {
            CaptureMode::Paused => {
                self.mode = CaptureMode::Collecting;
                Ok(self.checkpoint(CaptureSpanKind::Activity(snapshot), at))
            }
            CaptureMode::Locked => {
                self.resume_after_unlock = CaptureMode::Collecting;
                Ok(self.checkpoint(CaptureSpanKind::Gap(CaptureGapReason::SessionLocked), at))
            }
            CaptureMode::Collecting | CaptureMode::Stopped => Err(self.invalid("resume")),
        }
    }

    /// Replaces the current span with an explicit Windows lock gap.
    ///
    /// # Errors
    ///
    /// Rejects lock signals after the state machine has stopped.
    pub fn session_locked(&mut self, at: Timestamp) -> Result<CaptureCheckpoint, CaptureError> {
        match self.mode {
            CaptureMode::Collecting | CaptureMode::Paused => {
                self.resume_after_unlock = self.mode;
                self.mode = CaptureMode::Locked;
                Ok(self.checkpoint(CaptureSpanKind::Gap(CaptureGapReason::SessionLocked), at))
            }
            CaptureMode::Locked => {
                Ok(self.checkpoint(CaptureSpanKind::Gap(CaptureGapReason::SessionLocked), at))
            }
            CaptureMode::Stopped => Err(self.invalid("session_locked")),
        }
    }

    /// Leaves the lock gap and restores collection or a prior person pause.
    ///
    /// # Errors
    ///
    /// Rejects unlock unless the state machine is currently locked.
    pub fn session_unlocked(
        &mut self,
        snapshot: ActivitySnapshot,
        at: Timestamp,
    ) -> Result<CaptureCheckpoint, CaptureError> {
        if self.mode != CaptureMode::Locked {
            return Err(self.invalid("session_unlocked"));
        }
        self.mode = self.resume_after_unlock;
        match self.mode {
            CaptureMode::Collecting => Ok(self.checkpoint(CaptureSpanKind::Activity(snapshot), at)),
            CaptureMode::Paused => {
                Ok(self.checkpoint(CaptureSpanKind::Gap(CaptureGapReason::Paused), at))
            }
            CaptureMode::Locked | CaptureMode::Stopped => Err(self.invalid("session_unlocked")),
        }
    }

    /// Stops collection with an explicit exit or update gap.
    ///
    /// # Errors
    ///
    /// Rejects repeated stop signals.
    pub fn stop(
        &mut self,
        reason: ShutdownReason,
        at: Timestamp,
    ) -> Result<CaptureCheckpoint, CaptureError> {
        if self.mode == CaptureMode::Stopped {
            return Err(self.invalid("stop"));
        }
        self.mode = CaptureMode::Stopped;
        let reason = match reason {
            ShutdownReason::ExplicitExit => CaptureGapReason::ExplicitExit,
            ShutdownReason::Update => CaptureGapReason::Update,
        };
        Ok(self.checkpoint(CaptureSpanKind::Gap(reason), at))
    }

    fn checkpoint(&mut self, kind: CaptureSpanKind, at: Timestamp) -> CaptureCheckpoint {
        let begins_new_span = self.current.as_ref() != Some(&kind);
        self.current = Some(kind.clone());
        CaptureCheckpoint::new(at, kind, begins_new_span)
    }

    const fn invalid(&self, action: &'static str) -> CaptureError {
        CaptureError::InvalidTransition {
            from: self.mode,
            action,
        }
    }
}
