use eam_capture_windows::{
    ActivitySnapshot, CaptureGapReason, CaptureMode, CaptureSpanKind, CaptureStateMachine,
    IdleState, ShutdownReason,
};
use eam_core::Timestamp;

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millis(millis)
}

fn activity(application: &str, title: &str, idle: IdleState) -> ActivitySnapshot {
    ActivitySnapshot::new(application, title, idle).unwrap()
}

#[test]
fn foreground_switches_split_but_identical_observations_extend_one_span() {
    let mut capture = CaptureStateMachine::new();
    let first = capture
        .observe(activity("code.exe", "S28", IdleState::Active), at(1_000))
        .unwrap()
        .unwrap();
    let same = capture
        .observe(activity("code.exe", "S28", IdleState::Active), at(2_000))
        .unwrap()
        .unwrap();
    let switched = capture
        .observe(
            activity("firefox.exe", "Docs", IdleState::Active),
            at(3_000),
        )
        .unwrap()
        .unwrap();

    assert!(first.begins_new_span());
    assert!(!same.begins_new_span());
    assert!(switched.begins_new_span());
}

#[test]
fn idle_state_is_part_of_the_activity_interval_identity() {
    let mut capture = CaptureStateMachine::new();
    capture
        .observe(activity("code.exe", "S28", IdleState::Active), at(1_000))
        .unwrap();
    let idle = capture
        .observe(activity("code.exe", "S28", IdleState::Idle), at(2_000))
        .unwrap()
        .unwrap();

    assert!(idle.begins_new_span());
    assert!(matches!(
        idle.kind(),
        CaptureSpanKind::Activity(snapshot) if snapshot.idle_state() == IdleState::Idle
    ));
}

#[test]
fn pause_and_resume_emit_an_explicit_gap_without_collecting_while_paused() {
    let mut capture = CaptureStateMachine::new();
    capture
        .observe(activity("code.exe", "S28", IdleState::Active), at(1_000))
        .unwrap();
    let paused = capture.pause(at(2_000)).unwrap();
    let ignored = capture
        .observe(
            activity("mail.exe", "Private", IdleState::Active),
            at(2_500),
        )
        .unwrap();
    let resumed = capture
        .resume(activity("code.exe", "S28", IdleState::Active), at(3_000))
        .unwrap();

    assert_eq!(capture.mode(), CaptureMode::Collecting);
    assert!(matches!(
        paused.kind(),
        CaptureSpanKind::Gap(CaptureGapReason::Paused)
    ));
    assert!(ignored.is_none());
    assert!(matches!(resumed.kind(), CaptureSpanKind::Activity(_)));
}

#[test]
fn lock_and_unlock_preserve_a_prior_person_pause() {
    let mut capture = CaptureStateMachine::new();
    capture.pause(at(1_000)).unwrap();
    let locked = capture.session_locked(at(2_000)).unwrap();
    let unlocked = capture
        .session_unlocked(
            activity("code.exe", "must not collect", IdleState::Active),
            at(3_000),
        )
        .unwrap();

    assert_eq!(capture.mode(), CaptureMode::Paused);
    assert!(matches!(
        locked.kind(),
        CaptureSpanKind::Gap(CaptureGapReason::SessionLocked)
    ));
    assert!(matches!(
        unlocked.kind(),
        CaptureSpanKind::Gap(CaptureGapReason::Paused)
    ));
}

#[test]
fn source_failure_and_explicit_stop_never_create_activity() {
    let mut capture = CaptureStateMachine::new();
    let unavailable = capture.source_unavailable(at(1_000)).unwrap().unwrap();
    let stopped = capture
        .stop(ShutdownReason::ExplicitExit, at(2_000))
        .unwrap();

    assert!(matches!(
        unavailable.kind(),
        CaptureSpanKind::Gap(CaptureGapReason::SourceUnavailable)
    ));
    assert!(matches!(
        stopped.kind(),
        CaptureSpanKind::Gap(CaptureGapReason::ExplicitExit)
    ));
    assert_eq!(capture.mode(), CaptureMode::Stopped);
    assert!(
        capture
            .observe(activity("code.exe", "late", IdleState::Active), at(3_000))
            .is_err()
    );
}
