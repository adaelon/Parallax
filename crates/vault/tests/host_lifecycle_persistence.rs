use std::sync::{Mutex, MutexGuard};

use eam_core::Timestamp;
use eam_desktop_host::{ExitReason, HostGapReason, HostLifecycleRepository, LaunchMode};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const VAULT_KEY_BYTES: [u8; 32] = [0x67; 32];
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

#[test]
fn recovers_one_crash_gap_from_the_last_committed_heartbeat() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let first = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    repository
        .heartbeat_host_session(first.session().id(), at(1_200))
        .unwrap();
    drop(repository);

    let mut reopened = VaultRepository::open(directory.path(), key()).unwrap();
    let second = reopened
        .begin_host_session(at(2_000), LaunchMode::Background)
        .unwrap();
    let gap = second.recovered_gap().unwrap();
    assert_eq!(gap.from(), at(1_200));
    assert_eq!(gap.to(), at(2_000));
    assert_eq!(gap.reason(), HostGapReason::Crash);
    assert!(!gap.clock_rollback());
    assert_eq!(gap.recovered_by(), second.session().id());
    assert_eq!(reopened.all_host_runtime_gaps().unwrap(), vec![gap.clone()]);

    reopened
        .heartbeat_host_session(second.session().id(), at(2_100))
        .unwrap();
    assert_eq!(reopened.all_host_runtime_gaps().unwrap().len(), 1);
}

#[test]
fn explicit_exit_and_update_gaps_keep_distinct_reasons_across_reopen() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let first = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    repository
        .finish_host_session(first.session().id(), at(1_100), ExitReason::Explicit)
        .unwrap();
    repository.close().unwrap();

    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let second = repository
        .begin_host_session(at(2_000), LaunchMode::Foreground)
        .unwrap();
    assert_eq!(
        second.recovered_gap().unwrap().reason(),
        HostGapReason::ExplicitExit
    );
    repository
        .finish_host_session(second.session().id(), at(2_100), ExitReason::Update)
        .unwrap();
    repository.close().unwrap();

    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let third = repository
        .begin_host_session(at(3_000), LaunchMode::UpdateRelaunch)
        .unwrap();
    assert_eq!(
        third.recovered_gap().unwrap().reason(),
        HostGapReason::Update
    );
    let gaps = repository.all_host_runtime_gaps().unwrap();
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0].from(), at(1_100));
    assert_eq!(gaps[0].to(), at(2_000));
    assert_eq!(gaps[1].from(), at(2_100));
    assert_eq!(gaps[1].to(), at(3_000));
}

#[test]
fn clock_rollback_produces_a_zero_length_auditable_gap() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let first = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    repository
        .heartbeat_host_session(first.session().id(), at(1_200))
        .unwrap();
    drop(repository);

    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let second = repository
        .begin_host_session(at(1_100), LaunchMode::Foreground)
        .unwrap();
    let gap = second.recovered_gap().unwrap();
    assert_eq!(gap.from(), at(1_100));
    assert_eq!(gap.to(), at(1_100));
    assert!(gap.clock_rollback());
    assert_eq!(gap.reason(), HostGapReason::Crash);
}

#[test]
fn stale_or_closed_sessions_cannot_heartbeat_or_finish_again() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let mut repository = VaultRepository::open(directory.path(), key()).unwrap();
    let first = repository
        .begin_host_session(at(1_000), LaunchMode::Foreground)
        .unwrap();
    repository
        .finish_host_session(first.session().id(), at(1_100), ExitReason::Explicit)
        .unwrap();

    assert!(
        repository
            .heartbeat_host_session(first.session().id(), at(1_200))
            .is_err()
    );
    assert!(
        repository
            .finish_host_session(first.session().id(), at(1_200), ExitReason::Explicit)
            .is_err()
    );
}
