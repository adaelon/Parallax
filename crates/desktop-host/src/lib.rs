//! Trusted lifecycle contract for the S07 tray-resident desktop host.
//!
//! This crate contains no Tauri, `WebView`, credential, filesystem, or provider
//! API. The thin host adapts native events to this state machine and persists
//! sessions through [`HostLifecycleRepository`].

mod domain;
mod lifecycle;
mod ports;

pub use domain::{
    ExitReason, HostGapId, HostGapReason, HostRuntimeGap, HostSession, HostSessionId,
    HostSessionStart, LaunchMode,
};
pub use lifecycle::{ExitPlan, HostLifecycle, HostLifecycleError, HostState};
pub use ports::HostLifecycleRepository;
