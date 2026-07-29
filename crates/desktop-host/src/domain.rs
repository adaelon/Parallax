use eam_core::Timestamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostSessionId(u64);

impl HostSessionId {
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostGapId(u64);

impl HostGapId {
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
pub enum LaunchMode {
    Foreground,
    Background,
    UpdateRelaunch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    Explicit,
    Update,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostGapReason {
    Crash,
    ExplicitExit,
    Update,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSession {
    id: HostSessionId,
    launch_mode: LaunchMode,
    started_at: Timestamp,
    last_seen_at: Timestamp,
    ended_at: Option<Timestamp>,
    end_reason: Option<ExitReason>,
}

impl HostSession {
    #[must_use]
    pub const fn restore(
        id: HostSessionId,
        launch_mode: LaunchMode,
        started_at: Timestamp,
        last_seen_at: Timestamp,
        ended_at: Option<Timestamp>,
        end_reason: Option<ExitReason>,
    ) -> Self {
        Self {
            id,
            launch_mode,
            started_at,
            last_seen_at,
            ended_at,
            end_reason,
        }
    }

    #[must_use]
    pub const fn id(&self) -> HostSessionId {
        self.id
    }

    #[must_use]
    pub const fn launch_mode(&self) -> LaunchMode {
        self.launch_mode
    }

    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.started_at
    }

    #[must_use]
    pub const fn last_seen_at(&self) -> Timestamp {
        self.last_seen_at
    }

    #[must_use]
    pub const fn ended_at(&self) -> Option<Timestamp> {
        self.ended_at
    }

    #[must_use]
    pub const fn end_reason(&self) -> Option<ExitReason> {
        self.end_reason
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRuntimeGap {
    id: HostGapId,
    from: Timestamp,
    to: Timestamp,
    reason: HostGapReason,
    clock_rollback: bool,
    recovered_by: HostSessionId,
}

impl HostRuntimeGap {
    #[must_use]
    pub const fn restore(
        id: HostGapId,
        from: Timestamp,
        to: Timestamp,
        reason: HostGapReason,
        clock_rollback: bool,
        recovered_by: HostSessionId,
    ) -> Self {
        Self {
            id,
            from,
            to,
            reason,
            clock_rollback,
            recovered_by,
        }
    }

    #[must_use]
    pub const fn id(&self) -> HostGapId {
        self.id
    }

    #[must_use]
    pub const fn from(&self) -> Timestamp {
        self.from
    }

    #[must_use]
    pub const fn to(&self) -> Timestamp {
        self.to
    }

    #[must_use]
    pub const fn reason(&self) -> HostGapReason {
        self.reason
    }

    #[must_use]
    pub const fn clock_rollback(&self) -> bool {
        self.clock_rollback
    }

    #[must_use]
    pub const fn recovered_by(&self) -> HostSessionId {
        self.recovered_by
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSessionStart {
    session: HostSession,
    recovered_gap: Option<HostRuntimeGap>,
}

impl HostSessionStart {
    #[must_use]
    pub const fn new(session: HostSession, recovered_gap: Option<HostRuntimeGap>) -> Self {
        Self {
            session,
            recovered_gap,
        }
    }

    #[must_use]
    pub const fn session(&self) -> &HostSession {
        &self.session
    }

    #[must_use]
    pub const fn recovered_gap(&self) -> Option<&HostRuntimeGap> {
        self.recovered_gap.as_ref()
    }
}
