use std::{fs, time::Duration};

use eam_ingestion::{
    ArchiveReceipt, ArchiveStatus, ImportOutcome, ImportPolicy, UnparsedReason, ingest_inbox_file,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x68; 32];

fn test_policy() -> ImportPolicy {
    ImportPolicy {
        stability_window: Duration::ZERO,
        auto_import_limit_bytes: 1024,
        hard_import_limit_bytes: 2048,
    }
}

#[test]
fn encrypted_objects_deduplicate_and_survive_source_deletion_and_reopen() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let markdown = inbox.path().join("evidence.md");
    let binary = inbox.path().join("copy.bin");
    let content = b"same private evidence";
    fs::write(&markdown, content).unwrap();
    fs::write(&binary, content).unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();

    let first =
        ingest_inbox_file(&mut repository, &markdown, &test_policy(), false, 1_000).unwrap();
    let second = ingest_inbox_file(&mut repository, &binary, &test_policy(), false, 2_000).unwrap();

    let ImportOutcome::Archived(first) = first else {
        panic!("Markdown should archive");
    };
    let ImportOutcome::Archived(second) = second else {
        panic!("non-Markdown should archive as unsupported");
    };
    assert_eq!(first.status, ArchiveStatus::Archived);
    assert!(!first.object_reused);
    assert_eq!(
        second,
        ArchiveReceipt {
            archive_id: 2,
            status: ArchiveStatus::ArchivedUnparsed(UnparsedReason::UnsupportedFormat),
            object_reused: true,
            source_version_reused: false,
        }
    );
    assert_eq!(
        fs::read_dir(vault.path().join("objects")).unwrap().count(),
        1
    );

    fs::remove_file(markdown).unwrap();
    fs::remove_file(binary).unwrap();
    assert_eq!(repository.read_archived_content(1).unwrap(), content);
    assert_eq!(repository.read_archived_content(2).unwrap(), content);
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let archived = repository.archived_evidence().unwrap();
    assert_eq!(archived.len(), 2);
    assert_eq!(archived[0].status, ArchiveStatus::Archived);
    assert_eq!(
        archived[1].status,
        ArchiveStatus::ArchivedUnparsed(UnparsedReason::UnsupportedFormat)
    );
    assert_eq!(repository.read_archived_content(1).unwrap(), content);
}

#[test]
fn same_source_version_is_idempotent() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let path = inbox.path().join("note.md");
    fs::write(&path, b"one version").unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();

    let first = ingest_inbox_file(&mut repository, &path, &test_policy(), false, 10).unwrap();
    let second = ingest_inbox_file(&mut repository, &path, &test_policy(), false, 20).unwrap();

    let ImportOutcome::Archived(first) = first else {
        panic!("first import should archive");
    };
    let ImportOutcome::Archived(second) = second else {
        panic!("second import should be idempotent");
    };
    assert_eq!(first.archive_id, second.archive_id);
    assert!(second.object_reused);
    assert!(second.source_version_reused);
    assert_eq!(repository.archived_evidence().unwrap().len(), 1);
}

#[test]
fn missing_referenced_object_fails_closed_on_reopen() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let path = inbox.path().join("note.md");
    fs::write(&path, b"must remain present").unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    ingest_inbox_file(&mut repository, &path, &test_policy(), false, 30).unwrap();
    repository.close().unwrap();

    let object_path = fs::read_dir(vault.path().join("objects"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::remove_file(object_path).unwrap();

    assert!(VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).is_err());
}
