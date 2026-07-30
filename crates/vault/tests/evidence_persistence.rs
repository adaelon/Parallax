use std::{fs, time::Duration};

use eam_ingestion::{
    ArchiveStatus, BLOCK_LINEAGE_RULE_VERSION, BlockLineageRepository, BlockLineageStatus,
    EvidenceBlockRef, EvidenceError, EvidenceQueryError, ImportOutcome, ImportPolicy, LineageBasis,
    MarkdownProcessingOutcome, NATIVE_NAVIGATION_UNAVAILABLE, ingest_inbox_file,
    materialize_accepted_markdown, materialize_incremental_markdown, open_evidence_block,
    process_archived_markdown,
};
use eam_markdown::{CONTRACT_VERSION, ParseLimits};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x5a; 32];
const MULTILINGUAL_MARKDOWN: &str =
    "# 标题 😀\n\nCafe\u{301} 与日本語、한국어。 ^stable-id\n\n- 引用一\n- reference two\n";
const DUPLICATE_MARKDOWN_V1: &str = "# 谱系\n\nRepeated evidence.\n\nRepeated evidence.\n";
const DUPLICATE_MARKDOWN_V2: &str =
    "# 谱系\n\nNew separator.\n\nRepeated evidence.\n\nRepeated evidence.\n";

fn test_policy() -> ImportPolicy {
    ImportPolicy {
        stability_window: Duration::ZERO,
        auto_import_limit_bytes: 1024 * 1024,
        hard_import_limit_bytes: 2 * 1024 * 1024,
    }
}

#[test]
fn extraction_revision_blocks_and_refs_are_stable_across_sqlcipher_reopen() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let path = inbox.path().join("multilingual.md");
    fs::write(&path, MULTILINGUAL_MARKDOWN).unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let ImportOutcome::Archived(receipt) =
        ingest_inbox_file(&mut repository, &path, &test_policy(), false, 100).unwrap()
    else {
        panic!("fixture must archive before parsing");
    };
    assert_eq!(receipt.status, ArchiveStatus::Archived);
    assert!(matches!(
        process_archived_markdown(
            &mut repository,
            receipt.archive_id,
            ParseLimits::default(),
            110,
            120,
        )
        .unwrap(),
        MarkdownProcessingOutcome::Accepted { .. }
    ));

    let first =
        materialize_accepted_markdown(&mut repository, receipt.archive_id, CONTRACT_VERSION)
            .unwrap();
    assert!(!first.reused());
    assert!(!first.blocks().is_empty());
    for block in first.blocks() {
        assert_eq!(block.reference().evidence_id(), receipt.archive_id);
        assert_eq!(
            block.anchor().quote(MULTILINGUAL_MARKDOWN).unwrap(),
            &MULTILINGUAL_MARKDOWN[block.anchor().start_byte()..block.anchor().end_byte()]
        );
    }
    let repeated =
        materialize_accepted_markdown(&mut repository, receipt.archive_id, CONTRACT_VERSION)
            .unwrap();
    assert!(repeated.reused());
    assert_eq!(repeated.revision(), first.revision());
    assert_eq!(repeated.blocks(), first.blocks());
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let reopened = repository
        .materialized_extraction(receipt.archive_id, CONTRACT_VERSION)
        .unwrap()
        .expect("committed extraction must survive reopen");
    assert_eq!(reopened.revision(), first.revision());
    assert_eq!(reopened.blocks(), first.blocks());

    let referenced = reopened
        .blocks()
        .iter()
        .find(|block| block.anchor().native_locator().is_some())
        .expect("fixture must expose a versioned native locator")
        .reference();
    let view = open_evidence_block(&repository, referenced).unwrap();
    assert_eq!(view.reference(), referenced);
    assert_eq!(
        view.verbatim(),
        &MULTILINGUAL_MARKDOWN
            [view.block().anchor().start_byte()..view.block().anchor().end_byte()]
    );
    assert_eq!(
        view.ui_range().start_utf16(),
        MULTILINGUAL_MARKDOWN[..view.block().anchor().start_byte()]
            .encode_utf16()
            .count()
    );
    assert_eq!(
        view.native_navigation(false).unwrap_err(),
        EvidenceError::NativeNavigationUnavailable
    );
    assert_eq!(
        view.native_navigation(false).unwrap_err().to_string(),
        NATIVE_NAVIGATION_UNAVAILABLE
    );
    assert_eq!(
        view.verbatim(),
        view.block().anchor().quote(MULTILINGUAL_MARKDOWN).unwrap()
    );
    assert!(view.native_navigation(true).is_ok());

    let wrong_evidence = EvidenceBlockRef::new(
        receipt.archive_id.checked_add(100).unwrap(),
        referenced.block_id(),
    )
    .unwrap();
    assert!(matches!(
        open_evidence_block(&repository, wrong_evidence),
        Err(EvidenceQueryError::Evidence(EvidenceError::BlockNotFound))
    ));
}

#[test]
fn ambiguous_lineage_and_work_plan_survive_reopen_without_rewriting_history() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let path = inbox.path().join("lineage.md");
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();

    fs::write(&path, DUPLICATE_MARKDOWN_V1).unwrap();
    let first_id = archive_and_parse(&mut repository, &path, 200);
    let first =
        materialize_incremental_markdown(&mut repository, first_id, CONTRACT_VERSION, 230).unwrap();
    assert!(first.lineage().is_none());
    let old_repeated_ref = first
        .extraction()
        .blocks()
        .iter()
        .find(|block| {
            block
                .anchor()
                .quote(DUPLICATE_MARKDOWN_V1)
                .unwrap()
                .contains("Repeated")
        })
        .unwrap()
        .reference();

    fs::write(&path, DUPLICATE_MARKDOWN_V2).unwrap();
    let second_id = archive_and_parse(&mut repository, &path, 300);
    let second =
        materialize_incremental_markdown(&mut repository, second_id, CONTRACT_VERSION, 330)
            .unwrap();
    let batch = second.lineage().unwrap().clone();
    let ambiguous = batch
        .lineages()
        .iter()
        .filter(|lineage| lineage.status() == BlockLineageStatus::Ambiguous)
        .collect::<Vec<_>>();
    assert_eq!(ambiguous.len(), 2);
    assert!(ambiguous.iter().all(|lineage| {
        lineage.to_ref().is_none()
            && matches!(
                lineage.basis(),
                LineageBasis::AmbiguousCandidates { candidates } if candidates.len() == 2
            )
    }));
    assert_eq!(
        open_evidence_block(&repository, old_repeated_ref)
            .unwrap()
            .verbatim(),
        "Repeated evidence.\n"
    );
    let retried =
        materialize_incremental_markdown(&mut repository, second_id, CONTRACT_VERSION, 9_999)
            .unwrap();
    assert_eq!(retried.lineage(), Some(&batch));
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let restored = repository
        .load_lineage_batch(batch.to_revision_id(), BLOCK_LINEAGE_RULE_VERSION)
        .unwrap()
        .expect("committed lineage must survive SQLCipher reopen");
    assert_eq!(restored, batch);
    assert_eq!(
        open_evidence_block(&repository, old_repeated_ref)
            .unwrap()
            .verbatim(),
        "Repeated evidence.\n"
    );
}

fn archive_and_parse(repository: &mut VaultRepository, path: &std::path::Path, time: i64) -> u64 {
    let ImportOutcome::Archived(receipt) =
        ingest_inbox_file(repository, path, &test_policy(), false, time).unwrap()
    else {
        panic!("fixture must archive before parsing");
    };
    assert!(matches!(
        process_archived_markdown(
            repository,
            receipt.archive_id,
            ParseLimits::default(),
            time + 10,
            time + 20,
        )
        .unwrap(),
        MarkdownProcessingOutcome::Accepted { .. }
    ));
    receipt.archive_id
}
