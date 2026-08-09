use std::{
    fs,
    sync::{Mutex, MutexGuard},
};

use eam_core::{
    ApplicableTime, ClaimOwner, EvidenceCitation, EvidenceId, IncrementingClock, JudgmentProposal,
    MemoryCore, MemoryRepository, PersonFactProposal, PersonFactProposalBatch, RuntimeResponse,
    ScriptedPersonFactResponse, ScriptedRuntime, SessionId, Timestamp, Uncertainty,
};
use eam_vault::{VaultError, VaultKey, VaultRepository};
use tempfile::tempdir;

mod support;

use support::ready_repository;

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
fn preserves_multiple_atomic_person_facts_and_exact_citations_across_reopen() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let marker = "S07C-7-我叫小林，而且我从 2024 年开始住在香港。";
    let source_id = EvidenceId::from_raw(7);
    let proposals = PersonFactProposalBatch::try_new([
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我叫小林",
            EvidenceCitation::new(source_id, "我叫小林"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我从 2024 年开始住在香港",
            EvidenceCitation::new(source_id, "我从 2024 年开始住在香港"),
            ApplicableTime::Since(Timestamp::from_millis(1_704_067_200_000)),
        ),
    ])
    .unwrap();
    let runtime = ScriptedRuntime::new([ScriptedPersonFactResponse::Exact(proposals)], []);
    let repository = ready_repository(directory.path(), VAULT_KEY_BYTES);
    assert_eq!(repository.schema_version().unwrap(), 28);
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(1_000));

    let observation = core
        .record_person_turn(session("before-restart"), marker)
        .unwrap();
    assert_eq!(observation.evidence_id(), source_id);
    assert_eq!(observation.accepted_person_fact_ids().len(), 2);
    assert!(observation.rejected_person_fact_proposals().is_empty());
    let (repository, _, _) = core.into_parts();
    let database_path = repository.database_path().to_owned();
    repository.close().unwrap();

    let citation = EvidenceCitation::new(source_id, "我叫小林");
    let response = RuntimeResponse::new("重启后仍可精确引用。")
        .with_citation(citation.clone())
        .with_judgment(JudgmentProposal::new(
            "我判断这条自述在重启后仍保持来源。",
            vec![citation.clone()],
            Uncertainty::Low,
            ApplicableTime::Since(Timestamp::from_millis(1_000)),
        ));
    let runtime = ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [response]);
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let reopened_person_facts = repository
        .all_claims()
        .unwrap()
        .into_iter()
        .filter(|claim| {
            claim.owner() == ClaimOwner::Person
                && claim
                    .support()
                    .iter()
                    .any(|support| support.evidence_id() == source_id)
        })
        .collect::<Vec<_>>();
    assert_eq!(reopened_person_facts.len(), 2);
    assert_eq!(reopened_person_facts[0].statement(), "我叫小林");
    assert_eq!(reopened_person_facts[0].support()[0], citation);
    assert_eq!(
        reopened_person_facts[0].applicable_time(),
        ApplicableTime::Unknown
    );
    assert_eq!(
        reopened_person_facts[1].applicable_time(),
        ApplicableTime::Since(Timestamp::from_millis(1_704_067_200_000))
    );
    assert_eq!(
        reopened_person_facts[1].support()[0].quote(),
        "我从 2024 年开始住在香港"
    );
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(2_000));
    let frozen = core.freeze_working_context(&[source_id]).unwrap();
    let outcome = core
        .run_counterpart_turn(session("after-restart"), "请精确引用。", frozen)
        .unwrap();

    assert_eq!(outcome.accepted_judgment_ids().len(), 1);
    assert_eq!(core.resolve_citation(&citation).unwrap(), "我叫小林");
    let evidence = core.repository().all_evidence().unwrap();
    let claims = core.repository().all_claims().unwrap();
    let cited_claims = claims
        .iter()
        .filter(|claim| claim.support() == std::slice::from_ref(&citation))
        .collect::<Vec<_>>();
    assert_eq!(evidence.len(), 9);
    assert_eq!(cited_claims.len(), 2);
    assert_eq!(cited_claims[0].owner(), ClaimOwner::Person);
    assert_eq!(cited_claims[1].owner(), ClaimOwner::Counterpart);

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
    let runtime =
        ScriptedRuntime::new([ScriptedPersonFactResponse::VerbatimFactAtRecordedTime], []);
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
