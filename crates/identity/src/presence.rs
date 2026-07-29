use std::{error::Error, fmt};

use eam_core::{Clock, RepositoryError, RuntimeError};

use crate::{
    IdentityRepository, PresenceState, SelfBundleRepository, SelfBundleState, SelfBundleVersion,
    WakeCommit, WakeExit, WakeTrigger, WakeWork,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WakeInterruptionReason {
    Work(RuntimeError),
    ConstitutionVersionChanged { expected: u64, proposed: u64 },
    IdentityStateVersionMismatch { expected: u64, proposed: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WakeInterruption {
    phase: PresenceState,
    reason: WakeInterruptionReason,
}

impl WakeInterruption {
    const fn new(phase: PresenceState, reason: WakeInterruptionReason) -> Self {
        Self { phase, reason }
    }

    #[must_use]
    pub const fn phase(&self) -> PresenceState {
        self.phase
    }

    #[must_use]
    pub const fn reason(&self) -> &WakeInterruptionReason {
        &self.reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WakeOutcome {
    bundle: SelfBundleVersion,
    trace: Vec<PresenceState>,
    interruption: Option<WakeInterruption>,
}

impl WakeOutcome {
    #[must_use]
    pub const fn bundle(&self) -> &SelfBundleVersion {
        &self.bundle
    }

    #[must_use]
    pub fn trace(&self) -> &[PresenceState] {
        &self.trace
    }

    #[must_use]
    pub const fn interruption(&self) -> Option<&WakeInterruption> {
        self.interruption.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresenceError {
    IdentityNotFormed,
    SelfBundleAlreadyInitialized,
    SelfBundleNotInitialized,
    IdentityStateVersionMismatch { expected: u64, proposed: u64 },
    VersionOverflow,
    Repository(RepositoryError),
}

impl fmt::Display for PresenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for PresenceError {}

impl From<RepositoryError> for PresenceError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

pub struct PresenceCoordinator<R, W, C> {
    repository: R,
    work: W,
    clock: C,
}

impl<R, W, C> PresenceCoordinator<R, W, C>
where
    R: IdentityRepository + SelfBundleRepository,
    W: WakeWork,
    C: Clock,
{
    #[must_use]
    pub const fn new(repository: R, work: W, clock: C) -> Self {
        Self {
            repository,
            work,
            clock,
        }
    }

    /// Creates Self Bundle version 1 around the current immutable identity.
    ///
    /// # Errors
    ///
    /// Rejects initialization before identity formation, a second bundle, an
    /// identity-version mismatch, or a repository failure.
    pub fn initialize_self_bundle(
        &mut self,
        state: SelfBundleState,
    ) -> Result<SelfBundleVersion, PresenceError> {
        if self.repository.current_self_bundle()?.is_some() {
            return Err(PresenceError::SelfBundleAlreadyInitialized);
        }
        let identity = self
            .repository
            .current_identity_state()?
            .ok_or(PresenceError::IdentityNotFormed)?;
        if state.identity_state_version() != identity.version() {
            return Err(PresenceError::IdentityStateVersionMismatch {
                expected: identity.version(),
                proposed: state.identity_state_version(),
            });
        }

        let bundle = SelfBundleVersion::restore(1, None, state, None, self.clock.now());
        self.repository.append_self_bundle(bundle.clone())?;
        Ok(bundle)
    }

    /// Runs one bounded wake cycle and appends a complete Self Bundle before sleeping.
    ///
    /// Work-step failures and unsafe proposed state changes are represented in
    /// the successful [`WakeOutcome`]: the last valid state is committed, then
    /// the cycle returns to sleeping. Storage load or commit failures return an
    /// error and never claim the final sleeping transition.
    ///
    /// # Errors
    ///
    /// Returns an error when no bundle or identity exists, version space is
    /// exhausted, or the repository cannot load or atomically append state.
    pub fn wake(&mut self, trigger: WakeTrigger) -> Result<WakeOutcome, PresenceError> {
        let mut trace = vec![PresenceState::Sleeping, PresenceState::LoadSelf];
        let current = self
            .repository
            .current_self_bundle()?
            .ok_or(PresenceError::SelfBundleNotInitialized)?;
        let identity = self
            .repository
            .current_identity_state()?
            .ok_or(PresenceError::IdentityNotFormed)?;
        let expected_constitution = current.state().constitution_version();
        let expected_identity = identity.version();
        let mut state = current.state().clone();
        let mut interruption = None;

        for phase in [
            PresenceState::Observe,
            PresenceState::Think,
            PresenceState::Respond,
        ] {
            trace.push(phase);
            let proposed = match phase {
                PresenceState::Observe => self.work.observe(trigger, &state),
                PresenceState::Think => self.work.think(trigger, &state),
                PresenceState::Respond => self.work.respond(trigger, &state),
                PresenceState::Sleeping
                | PresenceState::LoadSelf
                | PresenceState::WriteAgentMemory => unreachable!("wake work phase is fixed"),
            };

            let candidate = match proposed {
                Ok(candidate) => candidate,
                Err(error) => {
                    interruption = Some(WakeInterruption::new(
                        phase,
                        WakeInterruptionReason::Work(error),
                    ));
                    break;
                }
            };

            if candidate.constitution_version() != expected_constitution {
                interruption = Some(WakeInterruption::new(
                    phase,
                    WakeInterruptionReason::ConstitutionVersionChanged {
                        expected: expected_constitution,
                        proposed: candidate.constitution_version(),
                    },
                ));
                break;
            }
            if candidate.identity_state_version() != expected_identity {
                interruption = Some(WakeInterruption::new(
                    phase,
                    WakeInterruptionReason::IdentityStateVersionMismatch {
                        expected: expected_identity,
                        proposed: candidate.identity_state_version(),
                    },
                ));
                break;
            }
            state = candidate;
        }

        let exit = interruption
            .as_ref()
            .map_or(WakeExit::Completed, |failure| {
                WakeExit::InterruptedAt(failure.phase())
            });
        trace.push(PresenceState::WriteAgentMemory);
        let next_version = current
            .version()
            .checked_add(1)
            .ok_or(PresenceError::VersionOverflow)?;
        let next = SelfBundleVersion::restore(
            next_version,
            Some(current.version()),
            state,
            Some(WakeCommit::new(trigger, exit)),
            self.clock.now(),
        );
        self.repository.append_self_bundle(next.clone())?;
        trace.push(PresenceState::Sleeping);

        Ok(WakeOutcome {
            bundle: next,
            trace,
            interruption,
        })
    }

    /// Loads the latest complete Self Bundle version.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when persisted state cannot be decoded.
    pub fn current_self_bundle(&self) -> Result<Option<SelfBundleVersion>, PresenceError> {
        self.repository
            .current_self_bundle()
            .map_err(PresenceError::from)
    }

    #[must_use]
    pub const fn repository(&self) -> &R {
        &self.repository
    }

    #[must_use]
    pub const fn work(&self) -> &W {
        &self.work
    }

    #[must_use]
    pub fn into_parts(self) -> (R, W, C) {
        (self.repository, self.work, self.clock)
    }
}
