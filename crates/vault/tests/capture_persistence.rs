use std::sync::{Mutex, MutexGuard};

use eam_capture_windows::{
    ActivitySnapshot, ActivityTimelineRepository, CaptureGapReason, CaptureMode, CaptureSpanKind,
    CaptureStateMachine, IdleState, ShutdownReason,
};
use eam_core::Timestamp;
use eam_desktop_host::{HostLifecycleRepository, LaunchMode};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const VAULT_KEY_BYTES: [u8; 32] = [0x28; 32];
static SQLCIPHER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn sqlcipher_test_lock() -> MutexGuard<'static, ()> {
    SQLCIPHER_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn key() -> VaultKey {
    VaultKey::new(VAULT_KEY_BYTES)
}

fn at(millis: i64) -> Timestamp {
    Timestamp::from_millis(millis)
}

fn activity(application: &str, title: &str, idle: IdleState) -> ActivitySnapshot {
    ActivitySnapshot::new(application, title, idle).unwrap()
}

#[test]
fn merges_continuous_activity_and_recovers_a_crash_without_inventing_time() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 26);
    let host = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    let recovery = repository
        .recover_capture_timeline(host.session().id(), at(1_000), None)
        .unwrap();
    let mut capture = CaptureStateMachine::restore(&recovery);

    for checkpoint in [
        capture
            .observe(activity("code.exe", "S28", IdleState::Active), at(1_100))
            .unwrap()
            .unwrap(),
        capture
            .observe(activity("code.exe", "S28", IdleState::Active), at(1_200))
            .unwrap()
            .unwrap(),
        capture
            .observe(activity("code.exe", "S28", IdleState::Idle), at(1_300))
            .unwrap()
            .unwrap(),
    ] {
        repository
            .record_capture_checkpoint(host.session().id(), &checkpoint)
            .unwrap();
    }
    let before_crash = repository.all_capture_spans().unwrap();
    assert_eq!(before_crash.len(), 2);
    assert_eq!(before_crash[0].started_at(), at(1_100));
    assert_eq!(before_crash[0].ended_at(), Some(at(1_300)));
    assert_eq!(before_crash[1].observed_until(), at(1_300));
    drop(repository);

    let mut reopened = VaultRepository::open(directory.path(), key()).unwrap();
    let next_host = reopened
        .begin_host_session(at(2_000), LaunchMode::Background)
        .unwrap();
    let recovery = reopened
        .recover_capture_timeline(
            next_host.session().id(),
            at(2_000),
            next_host
                .recovered_gap()
                .map(eam_desktop_host::HostRuntimeGap::reason),
        )
        .unwrap();
    assert_eq!(recovery.mode(), CaptureMode::Collecting);
    assert!(matches!(
        recovery.open_kind(),
        Some(CaptureSpanKind::Gap(CaptureGapReason::Crash))
    ));
    let recovered = reopened.all_capture_spans().unwrap();
    assert_eq!(recovered.len(), 3);
    assert_eq!(recovered[1].ended_at(), Some(at(1_300)));
    assert_eq!(recovered[2].started_at(), at(1_300));
    assert_eq!(recovered[2].observed_until(), at(2_000));
}

#[test]
fn person_pause_survives_reopen_and_collects_nothing_until_resume() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let host = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    let mut capture = CaptureStateMachine::new();
    let observed = capture
        .observe(activity("code.exe", "S28", IdleState::Active), at(1_100))
        .unwrap()
        .unwrap();
    repository
        .record_capture_checkpoint(host.session().id(), &observed)
        .unwrap();
    let paused = capture.pause(at(1_200)).unwrap();
    repository
        .record_capture_checkpoint(host.session().id(), &paused)
        .unwrap();
    drop(repository);

    let mut reopened = VaultRepository::open(directory.path(), key()).unwrap();
    let next_host = reopened
        .begin_host_session(at(2_000), LaunchMode::Foreground)
        .unwrap();
    let recovery = reopened
        .recover_capture_timeline(
            next_host.session().id(),
            at(2_000),
            next_host
                .recovered_gap()
                .map(eam_desktop_host::HostRuntimeGap::reason),
        )
        .unwrap();
    assert_eq!(recovery.mode(), CaptureMode::Paused);
    let mut capture = CaptureStateMachine::restore(&recovery);
    assert!(
        capture
            .observe(
                activity("mail.exe", "Private", IdleState::Active),
                at(2_050)
            )
            .unwrap()
            .is_none()
    );
    let resumed = capture
        .resume(activity("code.exe", "S28", IdleState::Active), at(2_100))
        .unwrap();
    reopened
        .record_capture_checkpoint(next_host.session().id(), &resumed)
        .unwrap();

    let spans = reopened.all_capture_spans().unwrap();
    assert_eq!(spans.len(), 3);
    assert!(matches!(
        spans[1].kind(),
        CaptureSpanKind::Gap(CaptureGapReason::Paused)
    ));
    assert_eq!(spans[1].started_at(), at(1_200));
    assert_eq!(spans[1].ended_at(), Some(at(2_100)));
    assert!(matches!(spans[2].kind(), CaptureSpanKind::Activity(_)));
}

#[test]
fn stale_host_session_cannot_append_capture_data() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let first = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    repository
        .finish_host_session(
            first.session().id(),
            at(1_100),
            eam_desktop_host::ExitReason::Explicit,
        )
        .unwrap();
    let second = repository
        .begin_host_session(at(2_000), LaunchMode::Foreground)
        .unwrap();
    let checkpoint = CaptureStateMachine::new()
        .observe(activity("code.exe", "S28", IdleState::Active), at(2_100))
        .unwrap()
        .unwrap();

    assert!(
        repository
            .record_capture_checkpoint(first.session().id(), &checkpoint)
            .is_err()
    );
    repository
        .record_capture_checkpoint(second.session().id(), &checkpoint)
        .unwrap();
}

#[test]
fn lock_and_explicit_exit_are_reasoned_gaps_not_activity() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let host = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    let mut capture = CaptureStateMachine::new();
    let observed = capture
        .observe(activity("code.exe", "S28", IdleState::Active), at(1_100))
        .unwrap()
        .unwrap();
    repository
        .record_capture_checkpoint(host.session().id(), &observed)
        .unwrap();
    let locked = capture.session_locked(at(1_200)).unwrap();
    repository
        .record_capture_checkpoint(host.session().id(), &locked)
        .unwrap();
    assert!(
        capture
            .observe(
                activity("mail.exe", "must not persist", IdleState::Active),
                at(1_300)
            )
            .unwrap()
            .is_none()
    );
    let stopped = capture
        .stop(ShutdownReason::ExplicitExit, at(1_400))
        .unwrap();
    repository
        .record_capture_checkpoint(host.session().id(), &stopped)
        .unwrap();
    repository
        .finish_host_session(
            host.session().id(),
            at(1_400),
            eam_desktop_host::ExitReason::Explicit,
        )
        .unwrap();
    repository.close().unwrap();

    let mut reopened = VaultRepository::open(directory.path(), key()).unwrap();
    let next_host = reopened
        .begin_host_session(at(2_000), LaunchMode::Foreground)
        .unwrap();
    reopened
        .recover_capture_timeline(
            next_host.session().id(),
            at(2_000),
            next_host
                .recovered_gap()
                .map(eam_desktop_host::HostRuntimeGap::reason),
        )
        .unwrap();
    let spans = reopened.all_capture_spans().unwrap();
    assert_eq!(spans.len(), 3);
    assert!(matches!(
        spans[1].kind(),
        CaptureSpanKind::Gap(CaptureGapReason::SessionLocked)
    ));
    assert!(matches!(
        spans[2].kind(),
        CaptureSpanKind::Gap(CaptureGapReason::ExplicitExit)
    ));
    assert_eq!(spans[2].observed_until(), at(2_000));
    assert!(spans.iter().all(|span| !matches!(
        span.kind(),
        CaptureSpanKind::Activity(snapshot) if snapshot.application() == "mail.exe"
    )));
}
