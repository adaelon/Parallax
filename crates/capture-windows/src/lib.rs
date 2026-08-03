//! Deterministic Windows activity timeline contract for S28.
//!
//! Native Windows APIs are adapters at the edge. This crate keeps interval,
//! gap, pause, lock, and recovery semantics independently testable.

mod domain;
mod engine;
mod native;
mod ports;

pub use domain::{
    ActivitySnapshot, CaptureCheckpoint, CaptureError, CaptureGapReason, CaptureMode,
    CaptureRecovery, CaptureSpan, CaptureSpanId, CaptureSpanKind, IdleState, ShutdownReason,
};
pub use engine::CaptureStateMachine;
pub use native::{DEFAULT_IDLE_THRESHOLD, NativeCaptureSample, sample_foreground_activity};
pub use ports::ActivityTimelineRepository;
