use std::{error::Error, fmt};

use crate::{ExitReason, HostSessionId, LaunchMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostState {
    Starting,
    Recovering,
    BackgroundRunning,
    ForegroundRunning,
    ExitingExplicit,
    ExitingUpdate,
    FailedClosed,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitPlan {
    session_id: HostSessionId,
    reason: ExitReason,
}

impl ExitPlan {
    #[must_use]
    pub const fn session_id(self) -> HostSessionId {
        self.session_id
    }

    #[must_use]
    pub const fn reason(self) -> ExitReason {
        self.reason
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostLifecycleError {
    InvalidTransition {
        from: HostState,
        action: &'static str,
    },
}

impl fmt::Display for HostLifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for HostLifecycleError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostLifecycle {
    state: HostState,
    session_id: Option<HostSessionId>,
}

impl Default for HostLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl HostLifecycle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: HostState::Starting,
            session_id: None,
        }
    }

    #[must_use]
    pub const fn state(&self) -> HostState {
        self.state
    }

    #[must_use]
    pub const fn session_id(&self) -> Option<HostSessionId> {
        self.session_id
    }

    /// Enters recovery before the encrypted host session is created.
    ///
    /// # Errors
    ///
    /// Rejects repeated or out-of-order startup.
    pub fn begin_recovery(&mut self) -> Result<(), HostLifecycleError> {
        self.require(HostState::Starting, "begin_recovery")?;
        self.state = HostState::Recovering;
        Ok(())
    }

    /// Marks recovery complete and enters the requested visible/background mode.
    ///
    /// # Errors
    ///
    /// Rejects completion outside recovery.
    pub fn complete_recovery(
        &mut self,
        session_id: HostSessionId,
        launch_mode: LaunchMode,
    ) -> Result<(), HostLifecycleError> {
        self.require(HostState::Recovering, "complete_recovery")?;
        self.session_id = Some(session_id);
        self.state = match launch_mode {
            LaunchMode::Background => HostState::BackgroundRunning,
            LaunchMode::Foreground | LaunchMode::UpdateRelaunch => HostState::ForegroundRunning,
        };
        Ok(())
    }

    /// Handles a close request by hiding the window without stopping Core.
    ///
    /// # Errors
    ///
    /// Rejects close requests outside a running state.
    pub fn hide_window(&mut self) -> Result<(), HostLifecycleError> {
        match self.state {
            HostState::ForegroundRunning | HostState::BackgroundRunning => {
                self.state = HostState::BackgroundRunning;
                Ok(())
            }
            state => Err(HostLifecycleError::InvalidTransition {
                from: state,
                action: "hide_window",
            }),
        }
    }

    /// Shows and focuses the existing window without creating another Core.
    ///
    /// # Errors
    ///
    /// Rejects activation outside a running state.
    pub fn show_window(&mut self) -> Result<(), HostLifecycleError> {
        match self.state {
            HostState::ForegroundRunning | HostState::BackgroundRunning => {
                self.state = HostState::ForegroundRunning;
                Ok(())
            }
            state => Err(HostLifecycleError::InvalidTransition {
                from: state,
                action: "show_window",
            }),
        }
    }

    /// Produces the one secure shutdown plan for explicit exit or update.
    ///
    /// # Errors
    ///
    /// Rejects repeated exit or exit outside a running state.
    pub fn begin_exit(&mut self, reason: ExitReason) -> Result<ExitPlan, HostLifecycleError> {
        if !matches!(
            self.state,
            HostState::ForegroundRunning | HostState::BackgroundRunning
        ) {
            return Err(HostLifecycleError::InvalidTransition {
                from: self.state,
                action: "begin_exit",
            });
        }
        let Some(session_id) = self.session_id else {
            return Err(HostLifecycleError::InvalidTransition {
                from: self.state,
                action: "begin_exit_without_session",
            });
        };
        self.state = match reason {
            ExitReason::Explicit => HostState::ExitingExplicit,
            ExitReason::Update => HostState::ExitingUpdate,
        };
        Ok(ExitPlan { session_id, reason })
    }

    /// Marks all secure shutdown steps complete.
    ///
    /// # Errors
    ///
    /// Rejects completion when no exit is active.
    pub fn mark_stopped(&mut self) -> Result<(), HostLifecycleError> {
        if !matches!(
            self.state,
            HostState::ExitingExplicit | HostState::ExitingUpdate
        ) {
            return Err(HostLifecycleError::InvalidTransition {
                from: self.state,
                action: "mark_stopped",
            });
        }
        self.state = HostState::Stopped;
        self.session_id = None;
        Ok(())
    }

    /// Marks an update failure after Core has been closed and cannot reopen.
    ///
    /// # Errors
    ///
    /// Rejects this terminal state outside update exit.
    pub fn mark_failed_closed(&mut self) -> Result<(), HostLifecycleError> {
        self.require(HostState::ExitingUpdate, "mark_failed_closed")?;
        self.state = HostState::FailedClosed;
        self.session_id = None;
        Ok(())
    }

    fn require(&self, expected: HostState, action: &'static str) -> Result<(), HostLifecycleError> {
        if self.state == expected {
            Ok(())
        } else {
            Err(HostLifecycleError::InvalidTransition {
                from: self.state,
                action,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(mode: LaunchMode) -> HostLifecycle {
        let mut lifecycle = HostLifecycle::new();
        lifecycle.begin_recovery().unwrap();
        lifecycle
            .complete_recovery(HostSessionId::from_raw(7), mode)
            .unwrap();
        lifecycle
    }

    #[test]
    fn close_only_hides_and_activation_reuses_the_session() {
        let mut lifecycle = running(LaunchMode::Foreground);
        lifecycle.hide_window().unwrap();
        assert_eq!(lifecycle.state(), HostState::BackgroundRunning);
        assert_eq!(lifecycle.session_id(), Some(HostSessionId::from_raw(7)));

        lifecycle.show_window().unwrap();
        assert_eq!(lifecycle.state(), HostState::ForegroundRunning);
        assert_eq!(lifecycle.session_id(), Some(HostSessionId::from_raw(7)));
    }

    #[test]
    fn background_launch_never_requires_a_visible_window() {
        let lifecycle = running(LaunchMode::Background);
        assert_eq!(lifecycle.state(), HostState::BackgroundRunning);
    }

    #[test]
    fn exit_is_single_use_and_reasoned() {
        let mut lifecycle = running(LaunchMode::Foreground);
        let plan = lifecycle.begin_exit(ExitReason::Update).unwrap();
        assert_eq!(plan.session_id(), HostSessionId::from_raw(7));
        assert_eq!(plan.reason(), ExitReason::Update);
        assert_eq!(lifecycle.state(), HostState::ExitingUpdate);
        assert!(lifecycle.begin_exit(ExitReason::Explicit).is_err());

        lifecycle.mark_stopped().unwrap();
        assert_eq!(lifecycle.state(), HostState::Stopped);
        assert_eq!(lifecycle.session_id(), None);
    }

    #[test]
    fn invalid_transitions_fail_closed() {
        let mut lifecycle = HostLifecycle::new();
        assert!(lifecycle.show_window().is_err());
        assert!(lifecycle.begin_exit(ExitReason::Explicit).is_err());
        lifecycle.begin_recovery().unwrap();
        assert!(lifecycle.begin_recovery().is_err());
        assert!(lifecycle.mark_stopped().is_err());
    }
}
