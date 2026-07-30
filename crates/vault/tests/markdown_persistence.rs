use std::{fs, time::Duration};

use eam_ingestion::{
    ArchiveStatus, ImportOutcome, ImportPolicy, MarkdownArchiveRepository, MarkdownParseStart,
    MarkdownParseState, MarkdownProcessingOutcome, UnparsedReason, ingest_inbox_file,
    process_archived_markdown,
};
use eam_markdown::{CONTRACT_VERSION, ParseLimits, ParseResource, parse_markdown};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x39; 32];
const FULL_DIALECT: &str = include_str!("../../markdown/tests/fixtures/full-dialect.md");

fn test_policy() -> ImportPolicy {
    ImportPolicy {
        stability_window: Duration::ZERO,
        auto_import_limit_bytes: 1024 * 1024,
        hard_import_limit_bytes: 2 * 1024 * 1024,
    }
}

fn archive_markdown(
    repository: &mut VaultRepository,
    path: &std::path::Path,
    archived_at_millis: i64,
) -> u64 {
    let outcome =
        ingest_inbox_file(repository, path, &test_policy(), false, archived_at_millis).unwrap();
    let ImportOutcome::Archived(receipt) = outcome else {
        panic!("Markdown fixture should be encrypted before parsing");
    };
    assert_eq!(receipt.status, ArchiveStatus::Archived);
    receipt.archive_id
}

#[test]
fn accepted_parse_artifact_and_attempt_survive_sqlcipher_reopen() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let path = inbox.path().join("full.md");
    fs::write(&path, FULL_DIALECT).unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let archive_id = archive_markdown(&mut repository, &path, 100);

    let outcome = process_archived_markdown(
        &mut repository,
        archive_id,
        ParseLimits::default(),
        110,
        120,
    )
    .unwrap();

    assert!(matches!(
        outcome,
        MarkdownProcessingOutcome::Accepted {
            archive_id: accepted_id,
            block_count,
            relation_count,
        } if accepted_id == archive_id && block_count > 0 && relation_count > 0
    ));
    assert_eq!(
        repository.archived_evidence().unwrap()[0].status,
        ArchiveStatus::Extracted
    );
    assert_eq!(
        repository.markdown_parse_attempts().unwrap(),
        vec![eam_ingestion::MarkdownParseAttempt {
            archive_id,
            parser_version: CONTRACT_VERSION.to_owned(),
            state: MarkdownParseState::Accepted,
            failure_reason: None,
            started_at_millis: 110,
            finished_at_millis: Some(120),
        }]
    );
    let expected = parse_markdown(FULL_DIALECT, ParseLimits::default()).unwrap();
    assert_eq!(
        repository
            .read_markdown_artifact(archive_id, CONTRACT_VERSION)
            .unwrap(),
        expected
    );
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_eq!(
        repository.archived_evidence().unwrap()[0].status,
        ArchiveStatus::Extracted
    );
    assert_eq!(
        repository
            .read_markdown_artifact(archive_id, CONTRACT_VERSION)
            .unwrap(),
        expected
    );
}

#[test]
fn invalid_utf8_and_source_limit_reject_without_partial_artifacts() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let invalid_path = inbox.path().join("invalid.md");
    let limited_path = inbox.path().join("limited.md");
    fs::write(&invalid_path, [0xff, 0xfe, 0xfd]).unwrap();
    fs::write(&limited_path, "# bounded").unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let invalid_id = archive_markdown(&mut repository, &invalid_path, 200);
    let limited_id = archive_markdown(&mut repository, &limited_path, 201);

    assert_eq!(
        process_archived_markdown(
            &mut repository,
            invalid_id,
            ParseLimits::default(),
            210,
            220,
        )
        .unwrap(),
        MarkdownProcessingOutcome::Rejected {
            archive_id: invalid_id,
            reason: UnparsedReason::InvalidEncoding,
        }
    );
    let tight = ParseLimits::new(1, 100, 64, 1024, 100).unwrap();
    assert_eq!(
        process_archived_markdown(&mut repository, limited_id, tight, 211, 221).unwrap(),
        MarkdownProcessingOutcome::Rejected {
            archive_id: limited_id,
            reason: UnparsedReason::ResourceLimit(ParseResource::SourceBytes),
        }
    );

    assert_eq!(
        repository.archived_evidence().unwrap()[0].status,
        ArchiveStatus::ArchivedUnparsed(UnparsedReason::InvalidEncoding)
    );
    assert_eq!(
        repository.archived_evidence().unwrap()[1].status,
        ArchiveStatus::ArchivedUnparsed(UnparsedReason::ResourceLimit(ParseResource::SourceBytes))
    );
    assert!(
        repository
            .read_markdown_artifact(invalid_id, CONTRACT_VERSION)
            .is_err()
    );
    assert!(
        repository
            .read_markdown_artifact(limited_id, CONTRACT_VERSION)
            .is_err()
    );
    assert_eq!(
        process_archived_markdown(
            &mut repository,
            limited_id,
            ParseLimits::default(),
            230,
            240,
        )
        .unwrap(),
        MarkdownProcessingOutcome::NotRetried {
            archive_id: limited_id,
            state: MarkdownParseState::Rejected,
        }
    );
}

#[test]
fn orphaned_started_attempt_becomes_interrupted_and_is_not_automatically_retried() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let path = inbox.path().join("crash-loop.md");
    fs::write(&path, "# never auto retry").unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let archive_id = archive_markdown(&mut repository, &path, 300);
    assert_eq!(
        repository
            .begin_markdown_parse(archive_id, CONTRACT_VERSION, 310)
            .unwrap(),
        MarkdownParseStart::Started
    );
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_eq!(
        repository.archived_evidence().unwrap()[0].status,
        ArchiveStatus::ArchivedUnparsed(UnparsedReason::ParserInterrupted)
    );
    assert_eq!(
        repository.markdown_parse_attempts().unwrap()[0].state,
        MarkdownParseState::Interrupted
    );
    assert_eq!(
        process_archived_markdown(
            &mut repository,
            archive_id,
            ParseLimits::default(),
            320,
            330,
        )
        .unwrap(),
        MarkdownProcessingOutcome::NotRetried {
            archive_id,
            state: MarkdownParseState::Interrupted,
        }
    );
    assert!(
        repository
            .read_markdown_artifact(archive_id, CONTRACT_VERSION)
            .is_err()
    );
}
