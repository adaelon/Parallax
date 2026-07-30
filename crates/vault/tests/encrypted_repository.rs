use std::{
    fs,
    sync::{Mutex, MutexGuard},
};

use eam_core::{
    ApplicableTime, ClaimOwner, EvidenceCitation, IncrementingClock, JudgmentProposal, MemoryCore,
    MemoryRepository, PersonTurnClassification, RuntimeResponse, ScriptedRuntime, SessionId,
    Timestamp, Uncertainty,
};
use eam_vault::{VaultError, VaultKey, VaultRepository};
use tempfile::tempdir;

const VAULT_KEY_BYTES: [u8; 32] = [0x42; 32];
static SQLCIPHER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn sqlcipher_test_lock() -> MutexGuard<'static, ()> {
    SQLCIPHER_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn key() -> VaultKey {
    VaultKey::new(VAULT_KEY_BYTES)
}

fn session(value: &str) -> SessionId {
    SessionId::new(value)
}

#[test]
fn preserves_exact_citations_and_separate_ledgers_across_reopen() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let marker = "S02-固定明文-不应出现在数据库字节中";
    let runtime = ScriptedRuntime::new([PersonTurnClassification::DirectSelfReport], []);
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 12);
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(1_000));

    let (source_id, classification) = core
        .record_person_turn(session("before-restart"), marker)
        .unwrap();
    assert_eq!(classification, PersonTurnClassification::DirectSelfReport);
    let (repository, _, _) = core.into_parts();
    let database_path = repository.database_path().to_owned();
    repository.close().unwrap();

    let citation = EvidenceCitation::new(source_id, marker);
    let response = RuntimeResponse::new("重启后仍可精确引用。")
        .with_citation(citation.clone())
        .with_judgment(JudgmentProposal::new(
            "我判断这条自述在重启后仍保持来源。",
            vec![citation.clone()],
            Uncertainty::Low,
            ApplicableTime::Since(Timestamp::from_millis(1_000)),
        ));
    let runtime = ScriptedRuntime::new([PersonTurnClassification::Question], [response]);
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(2_000));
    let frozen = core.freeze_working_context(&[source_id]).unwrap();
    let outcome = core
        .run_counterpart_turn(session("after-restart"), "请精确引用。", frozen)
        .unwrap();

    assert_eq!(outcome.accepted_judgment_ids().len(), 1);
    assert_eq!(core.resolve_citation(&citation).unwrap(), marker);
    let evidence = core.repository().all_evidence().unwrap();
    let claims = core.repository().all_claims().unwrap();
    assert_eq!(evidence.len(), 3);
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].owner(), ClaimOwner::Person);
    assert_eq!(claims[1].owner(), ClaimOwner::Counterpart);
    assert_eq!(claims[0].support(), std::slice::from_ref(&citation));
    assert_eq!(claims[1].support(), std::slice::from_ref(&citation));

    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();
    let encrypted_bytes = fs::read(database_path).unwrap();
    assert!(!contains_bytes(&encrypted_bytes, marker.as_bytes()));
    assert!(!contains_bytes(
        &encrypted_bytes,
        "重启后仍可精确引用。".as_bytes()
    ));
}

#[test]
fn rejects_a_wrong_key_without_modifying_the_vault() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    repository.close().unwrap();

    let wrong_result = VaultRepository::open(directory.path(), VaultKey::new([0x24; 32]));
    assert!(matches!(wrong_result, Err(VaultError::InvalidKeyOrCorrupt)));

    let repository = VaultRepository::open(directory.path(), key())
        .expect("the original key must still open after a rejected key");
    repository.close().unwrap();
}

#[test]
fn rejects_a_second_writer_until_the_first_closes() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();

    let second = VaultRepository::open(directory.path(), key());
    assert!(matches!(second, Err(VaultError::AlreadyOpen)));

    repository.close().unwrap();
    let reopened = VaultRepository::open(directory.path(), key()).unwrap();
    reopened.close().unwrap();
}

#[test]
fn rejects_an_authenticated_page_corruption() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let runtime = ScriptedRuntime::new([PersonTurnClassification::DirectSelfReport], []);
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let database_path = repository.database_path().to_owned();
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(3_000));
    let large_evidence = "加密页完整性固定语料".repeat(2_000);
    core.record_person_turn(session("corruption"), large_evidence)
        .unwrap();
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let mut bytes = fs::read(&database_path).unwrap();
    assert!(
        bytes.len() >= 8_192,
        "test fixture must span multiple pages"
    );
    let tamper_offset = bytes.len() - 32;
    bytes[tamper_offset] ^= 0x01;
    fs::write(&database_path, bytes).unwrap();

    let result = VaultRepository::open(directory.path(), key());
    assert!(matches!(result, Err(VaultError::InvalidKeyOrCorrupt)));
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
