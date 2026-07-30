use eam_ingestion::{
    MarkdownProcessingOutcome, materialize_incremental_markdown, process_archived_markdown,
};
use eam_markdown::{CONTRACT_VERSION, ParseLimits};
use eam_source_obsidian::{
    ObsidianSourceRepository, SourceArchiveInput, SourceAvailability, SourceFileKind,
    SourceRecordState, SourceRelationKind,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x71; 32];
const ALPHA_V1: &str = "---\ntags: [project/test]\naliases: [Alpha alias]\nowner: me\n---\n# Alpha\n\n[[Target]] and ![[asset.png]]. ^alpha\n";
const ALPHA_V2: &str = "---\ntags: [project/test]\naliases: [Alpha alias]\nowner: me\n---\n# Alpha\n\nUpdated [[Target]] and ![[asset.png]]. ^alpha\n";
const TARGET: &str = "# Target\n\nLinked evidence.\n";

#[test]
fn source_state_move_removal_and_reappearance_survive_reopen() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let root = repository
        .register_source_root("C:/notes/personal", 10)
        .unwrap();
    let (alpha, target, snapshot) = archive_initial_pair(&mut repository, root.id());
    assert_eq!(snapshot.records().len(), 2);
    assert_root_unavailable_preserves_children(&mut repository, root.id());
    move_alpha(&mut repository, root.id(), alpha.source_record_id());
    let restored = remove_and_restore_target(
        &mut repository,
        root.id(),
        alpha.source_record_id(),
        target.source_record_id(),
    );
    assert_eq!(
        restored.root().availability(),
        SourceAvailability::Available
    );
    assert!(
        restored
            .records()
            .iter()
            .all(|record| record.state() == SourceRecordState::Present)
    );
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let reopened = repository.load_source_root(root.id()).unwrap();
    assert_eq!(reopened, restored);
}

#[test]
fn metadata_relations_and_s11_lineage_are_queryable_for_obsidian_versions() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let root = repository
        .register_source_root("C:/notes/relations", 10)
        .unwrap();
    let (alpha, target, attachment) = archive_relation_fixture(&mut repository, root.id());
    accept_and_materialize(&mut repository, alpha.archive_id(), 40);
    accept_and_materialize(&mut repository, target.archive_id(), 50);
    repository.refresh_source_relations(root.id()).unwrap();

    assert_source_projection(
        &repository,
        alpha.archive_id(),
        target.source_record_id(),
        attachment.source_record_id(),
    );
    assert_modified_source_uses_s11_lineage(&mut repository, root.id(), alpha.source_record_id());
}

fn archive_initial_pair(
    repository: &mut VaultRepository,
    root_id: u64,
) -> (
    eam_source_obsidian::SourceArchiveReceipt,
    eam_source_obsidian::SourceArchiveReceipt,
    eam_source_obsidian::SourceRootSnapshot,
) {
    let paths = vec!["Alpha.md".to_owned(), "Target.md".to_owned()];
    let alpha = archive(
        repository,
        root_id,
        "Alpha.md",
        ALPHA_V1.as_bytes(),
        &paths,
        &[],
        20,
    );
    let target = archive(
        repository,
        root_id,
        "Target.md",
        TARGET.as_bytes(),
        &paths,
        &[alpha.source_record_id()],
        21,
    );
    let snapshot = repository
        .finish_source_reconciliation(
            root_id,
            &[alpha.source_record_id(), target.source_record_id()],
            30,
        )
        .unwrap();
    (alpha, target, snapshot)
}

fn assert_root_unavailable_preserves_children(repository: &mut VaultRepository, root_id: u64) {
    let unavailable = repository.mark_source_unavailable(root_id, 40).unwrap();
    assert_eq!(
        unavailable.availability(),
        SourceAvailability::SourceUnavailable
    );
    assert!(
        repository
            .load_source_root(root_id)
            .unwrap()
            .records()
            .iter()
            .all(|record| record.state() == SourceRecordState::Present)
    );
}

fn move_alpha(repository: &mut VaultRepository, root_id: u64, alpha_record_id: u64) {
    let paths = vec!["moved/Alpha.md".to_owned(), "Target.md".to_owned()];
    let target = archive(
        repository,
        root_id,
        "Target.md",
        TARGET.as_bytes(),
        &paths,
        &[],
        50,
    );
    let alpha = archive(
        repository,
        root_id,
        "moved/Alpha.md",
        ALPHA_V1.as_bytes(),
        &paths,
        &[target.source_record_id()],
        51,
    );
    assert_eq!(alpha.source_record_id(), alpha_record_id);
    assert_eq!(alpha.previous_relative_path(), Some("Alpha.md"));
    assert!(alpha.source_version_reused());
    repository
        .finish_source_reconciliation(
            root_id,
            &[target.source_record_id(), alpha.source_record_id()],
            60,
        )
        .unwrap();
}

fn remove_and_restore_target(
    repository: &mut VaultRepository,
    root_id: u64,
    alpha_record_id: u64,
    target_record_id: u64,
) -> eam_source_obsidian::SourceRootSnapshot {
    let only_alpha = vec!["moved/Alpha.md".to_owned()];
    let alpha = archive(
        repository,
        root_id,
        "moved/Alpha.md",
        ALPHA_V1.as_bytes(),
        &only_alpha,
        &[],
        70,
    );
    let removed = repository
        .finish_source_reconciliation(root_id, &[alpha.source_record_id()], 80)
        .unwrap();
    assert_eq!(
        removed
            .records()
            .iter()
            .find(|record| record.id() == target_record_id)
            .unwrap()
            .state(),
        SourceRecordState::SourceRemoved
    );
    let paths = vec!["moved/Alpha.md".to_owned(), "Target.md".to_owned()];
    let alpha = archive(
        repository,
        root_id,
        "moved/Alpha.md",
        ALPHA_V1.as_bytes(),
        &paths,
        &[],
        90,
    );
    assert_eq!(alpha.source_record_id(), alpha_record_id);
    let target = archive(
        repository,
        root_id,
        "Target.md",
        TARGET.as_bytes(),
        &paths,
        &[alpha.source_record_id()],
        91,
    );
    assert_eq!(target.source_record_id(), target_record_id);
    repository
        .finish_source_reconciliation(
            root_id,
            &[alpha.source_record_id(), target.source_record_id()],
            100,
        )
        .unwrap()
}

fn archive_relation_fixture(
    repository: &mut VaultRepository,
    root_id: u64,
) -> (
    eam_source_obsidian::SourceArchiveReceipt,
    eam_source_obsidian::SourceArchiveReceipt,
    eam_source_obsidian::SourceArchiveReceipt,
) {
    let paths = vec![
        "Alpha.md".to_owned(),
        "Target.md".to_owned(),
        "asset.png".to_owned(),
    ];
    let alpha = archive(
        repository,
        root_id,
        "Alpha.md",
        ALPHA_V1.as_bytes(),
        &paths,
        &[],
        20,
    );
    let target = archive(
        repository,
        root_id,
        "Target.md",
        TARGET.as_bytes(),
        &paths,
        &[alpha.source_record_id()],
        21,
    );
    let attachment = repository
        .archive_source_file(SourceArchiveInput {
            root_id,
            relative_path: "asset.png",
            observed_relative_paths: &paths,
            claimed_source_record_ids: &[alpha.source_record_id(), target.source_record_id()],
            content: b"png",
            kind: SourceFileKind::Attachment,
            observed_at_millis: 22,
        })
        .unwrap();
    repository
        .finish_source_reconciliation(
            root_id,
            &[
                alpha.source_record_id(),
                target.source_record_id(),
                attachment.source_record_id(),
            ],
            30,
        )
        .unwrap();
    (alpha, target, attachment)
}

fn assert_source_projection(
    repository: &VaultRepository,
    evidence_id: u64,
    target_record_id: u64,
    attachment_record_id: u64,
) {
    let projection = repository.source_document_projection(evidence_id).unwrap();
    assert!(
        projection
            .properties()
            .contains(&("owner".to_owned(), "me".to_owned()))
    );
    assert!(projection.tags().contains(&"project/test".to_owned()));
    assert_eq!(projection.aliases(), &["Alpha alias".to_owned()]);
    let wiki = projection
        .relations()
        .iter()
        .find(|relation| relation.kind() == SourceRelationKind::Wikilink)
        .unwrap();
    assert_eq!(wiki.target(), "Target");
    assert_eq!(wiki.resolved_source_record_id(), Some(target_record_id));
    let embed = projection
        .relations()
        .iter()
        .find(|relation| relation.kind() == SourceRelationKind::Embed)
        .unwrap();
    assert_eq!(
        embed.resolved_source_record_id(),
        Some(attachment_record_id)
    );
}

fn assert_modified_source_uses_s11_lineage(
    repository: &mut VaultRepository,
    root_id: u64,
    alpha_record_id: u64,
) {
    let paths = vec![
        "Alpha.md".to_owned(),
        "Target.md".to_owned(),
        "asset.png".to_owned(),
    ];
    let updated = archive(
        repository,
        root_id,
        "Alpha.md",
        ALPHA_V2.as_bytes(),
        &paths,
        &[],
        60,
    );
    assert_eq!(updated.source_record_id(), alpha_record_id);
    assert!(!updated.source_version_reused());
    assert!(matches!(
        process_archived_markdown(
            repository,
            updated.archive_id(),
            ParseLimits::default(),
            61,
            62,
        )
        .unwrap(),
        MarkdownProcessingOutcome::Accepted { .. }
    ));
    let incremental =
        materialize_incremental_markdown(repository, updated.archive_id(), CONTRACT_VERSION, 63)
            .unwrap();
    assert!(incremental.lineage().is_some());
}

fn archive(
    repository: &mut VaultRepository,
    root_id: u64,
    relative_path: &str,
    content: &[u8],
    observed_relative_paths: &[String],
    claimed_source_record_ids: &[u64],
    observed_at_millis: i64,
) -> eam_source_obsidian::SourceArchiveReceipt {
    repository
        .archive_source_file(SourceArchiveInput {
            root_id,
            relative_path,
            observed_relative_paths,
            claimed_source_record_ids,
            content,
            kind: SourceFileKind::Markdown,
            observed_at_millis,
        })
        .unwrap()
}

fn accept_and_materialize(repository: &mut VaultRepository, archive_id: u64, timestamp: i64) {
    assert!(matches!(
        process_archived_markdown(
            repository,
            archive_id,
            ParseLimits::default(),
            timestamp,
            timestamp + 1,
        )
        .unwrap(),
        MarkdownProcessingOutcome::Accepted { .. }
    ));
    materialize_incremental_markdown(repository, archive_id, CONTRACT_VERSION, timestamp + 2)
        .unwrap();
}
