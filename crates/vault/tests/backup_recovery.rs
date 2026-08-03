#![cfg(windows)]

use std::fs;

use eam_core::{
    ConversationEvidence, ForgetRequest, ForgetTarget, MemoryRepository, PersonTurnClassification,
    SessionId, Speaker, Timestamp,
};
use eam_ingestion::{ArchiveInput, ArchiveRepository, ArchiveStatus};
use eam_retrieval::{RetrievalQuery, SourceScope, retrieve};
use eam_vault::{RecoveryKey, VaultBackup, VaultError, VaultKeyStore, VaultRepository};
use tempfile::{TempDir, tempdir};

const PRIVATE_TEXT: &str = "S30-private-aurora-evidence";

fn initialized_repository() -> (TempDir, VaultRepository, RecoveryKey) {
    let vault = tempdir().unwrap();
    let initialized = VaultKeyStore::initialize(vault.path()).unwrap();
    let (vault_key, recovery_key) = initialized.into_parts();
    let repository = VaultRepository::open(vault.path(), vault_key).unwrap();
    (vault, repository, recovery_key)
}

fn record_person_fact(
    repository: VaultRepository,
) -> (
    eam_core::MemoryCore<VaultRepository, eam_core::ScriptedRuntime, eam_core::IncrementingClock>,
    eam_core::EvidenceId,
) {
    let mut core = eam_core::MemoryCore::new(
        repository,
        eam_core::ScriptedRuntime::new([PersonTurnClassification::DirectSelfReport], []),
        eam_core::IncrementingClock::new(1_000),
    );
    let (evidence_id, _) = core
        .record_person_turn(SessionId::new("s30"), PRIVATE_TEXT)
        .unwrap();
    (core, evidence_id)
}

#[test]
fn encrypted_snapshot_round_trips_authority_and_rebuilds_indexes() {
    let (source, repository, recovery_key) = initialized_repository();
    let (mut core, evidence_id) = record_person_fact(repository);
    assert_eq!(
        retrieve(core.repository_mut(), &RetrievalQuery::lexical("aurora"))
            .unwrap()
            .candidates()
            .len(),
        1
    );
    let archived = core
        .repository_mut()
        .archive(ArchiveInput {
            source_locator: "inbox/s30.txt",
            content: PRIVATE_TEXT.as_bytes(),
            status: ArchiveStatus::Archived,
            archived_at_millis: 2_000,
        })
        .unwrap();
    let backup_set = tempdir().unwrap();
    let receipt = VaultBackup::create(core.repository_mut(), backup_set.path(), 3_000).unwrap();
    let snapshot_bytes = fs::read(receipt.snapshot_path()).unwrap();
    let deletion_head_bytes = fs::read(backup_set.path().join("deletion-head.eam")).unwrap();
    assert!(
        !snapshot_bytes
            .windows(PRIVATE_TEXT.len())
            .any(|value| value == PRIVATE_TEXT.as_bytes())
    );
    assert!(
        !deletion_head_bytes
            .windows(PRIVATE_TEXT.len())
            .any(|value| value == PRIVATE_TEXT.as_bytes())
    );
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let destination_parent = tempdir().unwrap();
    let destination = destination_parent.path().join("restored-vault");
    let restored = VaultBackup::restore(
        receipt.snapshot_path(),
        backup_set.path().join("deletion-head.eam"),
        &destination,
        &recovery_key,
    )
    .unwrap();
    assert!(restored.vault_key_rotated());
    assert_eq!(restored.replayed_deletions(), 0);
    assert_eq!(
        fs::read(backup_set.path().join("deletion-head.eam")).unwrap(),
        deletion_head_bytes
    );

    let restored_key = VaultKeyStore::unlock_recovery(&destination, &recovery_key).unwrap();
    let mut repository = VaultRepository::open(&destination, restored_key).unwrap();
    assert!(
        MemoryRepository::evidence(&repository, evidence_id)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        repository
            .read_archived_content(archived.archive_id)
            .unwrap(),
        PRIVATE_TEXT.as_bytes()
    );
    assert_eq!(
        retrieve(&mut repository, &RetrievalQuery::lexical("aurora"))
            .unwrap()
            .candidates()
            .len(),
        1
    );
    drop(source);
}

#[test]
fn truncated_and_tampered_snapshots_fail_without_publishing_destination() {
    let (_source, mut repository, recovery_key) = initialized_repository();
    let backup_set = tempdir().unwrap();
    let receipt = VaultBackup::create(&mut repository, backup_set.path(), 1_000).unwrap();
    repository.close().unwrap();
    let original = fs::read(receipt.snapshot_path()).unwrap();
    let original_head = fs::read(backup_set.path().join("deletion-head.eam")).unwrap();

    for (name, snapshot_bytes, head_bytes) in [
        (
            "truncated-snapshot",
            original[..original.len() - 1].to_vec(),
            original_head.clone(),
        ),
        (
            "tampered-snapshot",
            {
                let mut bytes = original.clone();
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                bytes
            },
            original_head.clone(),
        ),
        (
            "truncated-head",
            original.clone(),
            original_head[..original_head.len() - 1].to_vec(),
        ),
        ("tampered-head", original.clone(), {
            let mut bytes = original_head.clone();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            bytes
        }),
    ] {
        let candidate = backup_set.path().join(format!("{name}.eambak"));
        let candidate_head = backup_set.path().join(format!("{name}.head"));
        fs::write(&candidate, snapshot_bytes).unwrap();
        fs::write(&candidate_head, head_bytes).unwrap();
        let destination = backup_set.path().join(format!("restore-{name}"));
        assert!(matches!(
            VaultBackup::restore(&candidate, &candidate_head, &destination, &recovery_key,),
            Err(VaultError::InvalidBackup)
        ));
        assert!(!destination.exists());
    }
}

#[test]
fn latest_deletion_head_prevents_an_old_backup_from_reviving_forgotten_evidence() {
    let (_source, repository, recovery_key) = initialized_repository();
    let (mut core, evidence_id) = record_person_fact(repository);
    let backup_set = tempdir().unwrap();
    let old = VaultBackup::create(core.repository_mut(), backup_set.path(), 2_000).unwrap();
    let absent_from_snapshot = core.repository_mut().next_evidence_id();
    MemoryRepository::append_evidence(
        core.repository_mut(),
        ConversationEvidence::restore(
            absent_from_snapshot,
            SessionId::new("s30-later"),
            Speaker::Person,
            "later deletion target".to_owned(),
            Timestamp::from_millis(2_500),
        ),
    )
    .unwrap();
    core.forget(ForgetRequest::new(
        ForgetTarget::ConversationEvidence(evidence_id),
        true,
    ))
    .unwrap();
    core.forget(ForgetRequest::new(
        ForgetTarget::ConversationEvidence(absent_from_snapshot),
        true,
    ))
    .unwrap();
    VaultBackup::synchronize_deletions(core.repository(), backup_set.path(), 3_000).unwrap();
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let destination = backup_set.path().join("restored-old");
    let receipt = VaultBackup::restore(
        old.snapshot_path(),
        backup_set.path().join("deletion-head.eam"),
        &destination,
        &recovery_key,
    )
    .unwrap();
    assert_eq!(receipt.replayed_deletions(), 2);
    let key = VaultKeyStore::unlock_recovery(&destination, &recovery_key).unwrap();
    let mut repository = VaultRepository::open(&destination, key).unwrap();
    assert!(
        MemoryRepository::evidence(&repository, evidence_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(repository.deletion_intents().unwrap().len(), 2);
    assert!(repository.next_evidence_id().get() > absent_from_snapshot.get());
    for scope in [SourceScope::Current, SourceScope::Historical] {
        assert!(
            retrieve(
                &mut repository,
                &RetrievalQuery::lexical("aurora").with_source_scope(scope),
            )
            .unwrap()
            .candidates()
            .is_empty()
        );
    }
}

#[test]
fn missing_referenced_object_blocks_backup_and_retention_keeps_three_generations() {
    let (_source, mut repository, _recovery_key) = initialized_repository();
    repository
        .archive(ArchiveInput {
            source_locator: "inbox/missing.txt",
            content: PRIVATE_TEXT.as_bytes(),
            status: ArchiveStatus::Archived,
            archived_at_millis: 1_000,
        })
        .unwrap();
    let object_path = fs::read_dir(repository.database_path().parent().unwrap().join("objects"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::remove_file(object_path).unwrap();
    let broken_set = tempdir().unwrap();
    assert!(matches!(
        VaultBackup::create(&mut repository, broken_set.path(), 2_000),
        Err(VaultError::InvalidBackup)
    ));
    repository.close().unwrap();

    let (_source, mut repository, _recovery_key) = initialized_repository();
    let retained_set = tempdir().unwrap();
    for created_at in 1..=4 {
        VaultBackup::create(&mut repository, retained_set.path(), created_at).unwrap();
    }
    let snapshot_count = fs::read_dir(retained_set.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".eambak"))
        .count();
    assert_eq!(snapshot_count, 3);
}
