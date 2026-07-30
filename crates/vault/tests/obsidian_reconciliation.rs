use std::{collections::BTreeMap, fs, path::Path};

use eam_ingestion::{
    ObsidianReconciliationOutcome, ReconciledSourceFile, reconcile_obsidian_source,
};
use eam_markdown::ParseLimits;
use eam_source_obsidian::{
    ObsidianSourceRepository, SourceAvailability, SourceRecordState, SourceRootSnapshot,
};
use eam_vault::{VaultKey, VaultRepository};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x72; 32];
const HARD_LIMIT_BYTES: u64 = 1024 * 1024;

#[test]
fn fixed_obsidian_vault_reconciles_end_to_end_without_source_writes() {
    let vault = tempdir().unwrap();
    let source_parent = tempdir().unwrap();
    let source = source_parent.path().join("obsidian-vault");
    copy_tree(fixture_root(), &source);
    let baseline_digest = directory_digest(&source);
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let root = repository
        .register_source_root(&source.to_string_lossy(), 10)
        .unwrap();

    let (initial, files) = reconcile_complete(&mut repository, root.id(), &source, 20);
    assert_eq!(
        files
            .iter()
            .map(ReconciledSourceFile::relative_path)
            .collect::<Vec<_>>(),
        vec!["Root.md", "asset.bin", "folder/Linked.md"]
    );
    assert_eq!(directory_digest(&source), baseline_digest);
    assert!(
        files
            .iter()
            .filter(|file| {
                Path::new(file.relative_path())
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            })
            .all(|file| file.markdown_outcome().is_some())
    );
    assert!(
        files
            .iter()
            .find(|file| file.relative_path() == "asset.bin")
            .unwrap()
            .markdown_outcome()
            .is_none()
    );
    assert_initial_relations(&repository, &initial);

    let (_, unchanged) = reconcile_complete(&mut repository, root.id(), &source, 30);
    assert!(
        unchanged
            .iter()
            .all(ReconciledSourceFile::source_version_reused)
    );
    let after_changes = apply_file_changes(&mut repository, root.id(), &source, &initial);
    assert_offline_and_restore(&mut repository, root.id(), &source, &after_changes);
}

fn apply_file_changes(
    repository: &mut VaultRepository,
    root_id: u64,
    source: &Path,
    initial: &SourceRootSnapshot,
) -> SourceRootSnapshot {
    let root_record = record(initial, "Root.md");
    let linked_record = record(initial, "folder/Linked.md");
    let asset_record = record(initial, "asset.bin");
    fs::write(
        source.join("Root.md"),
        fs::read_to_string(source.join("Root.md")).unwrap() + "\nModified evidence.\n",
    )
    .unwrap();
    let (modified, _) = reconcile_complete(repository, root_id, source, 40);
    assert_ne!(
        record(&modified, "Root.md").current_evidence_id(),
        root_record.current_evidence_id()
    );

    fs::rename(
        source.join("folder/Linked.md"),
        source.join("folder/Moved.md"),
    )
    .unwrap();
    let (moved, _) = reconcile_complete(repository, root_id, source, 50);
    assert_eq!(record(&moved, "folder/Moved.md").id(), linked_record.id());

    fs::remove_file(source.join("asset.bin")).unwrap();
    let (removed, _) = reconcile_complete(repository, root_id, source, 60);
    assert_eq!(
        removed
            .records()
            .iter()
            .find(|candidate| candidate.id() == asset_record.id())
            .unwrap()
            .state(),
        SourceRecordState::SourceRemoved
    );
    removed
}

fn assert_offline_and_restore(
    repository: &mut VaultRepository,
    root_id: u64,
    source: &Path,
    before_offline: &SourceRootSnapshot,
) {
    let asset_id = before_offline
        .records()
        .iter()
        .find(|record| record.relative_path() == "asset.bin")
        .unwrap()
        .id();
    let offline = source.with_file_name("obsidian-vault-offline");
    fs::rename(source, &offline).unwrap();
    let outcome = reconcile_obsidian_source(
        repository,
        root_id,
        source,
        HARD_LIMIT_BYTES,
        ParseLimits::default(),
        70,
    )
    .unwrap();
    let ObsidianReconciliationOutcome::SourceUnavailable(root) = outcome else {
        panic!("missing root must be unavailable");
    };
    assert_eq!(root.availability(), SourceAvailability::SourceUnavailable);
    assert_eq!(
        repository.load_source_root(root_id).unwrap().records(),
        before_offline.records()
    );

    fs::rename(&offline, source).unwrap();
    fs::write(source.join("asset.bin"), b"fixture attachment\n").unwrap();
    let (restored, _) = reconcile_complete(repository, root_id, source, 80);
    let asset = record(&restored, "asset.bin");
    assert_eq!(asset.id(), asset_id);
    assert_eq!(asset.state(), SourceRecordState::Present);
    assert_eq!(
        restored.root().availability(),
        SourceAvailability::Available
    );
}

fn assert_initial_relations(repository: &VaultRepository, snapshot: &SourceRootSnapshot) {
    let root_record = record(snapshot, "Root.md");
    let linked_id = record(snapshot, "folder/Linked.md").id();
    let asset_id = record(snapshot, "asset.bin").id();
    let projection = repository
        .source_document_projection(root_record.current_evidence_id().unwrap())
        .unwrap();
    assert!(projection.tags().contains(&"fixture/s12".to_owned()));
    let resolved = projection
        .relations()
        .iter()
        .filter_map(eam_source_obsidian::SourceRelation::resolved_source_record_id)
        .collect::<Vec<_>>();
    assert!(resolved.contains(&linked_id));
    assert!(resolved.contains(&asset_id));
}

fn reconcile_complete(
    repository: &mut VaultRepository,
    root_id: u64,
    source: &Path,
    timestamp: i64,
) -> (SourceRootSnapshot, Vec<eam_ingestion::ReconciledSourceFile>) {
    let outcome = reconcile_obsidian_source(
        repository,
        root_id,
        source,
        HARD_LIMIT_BYTES,
        ParseLimits::default(),
        timestamp,
    )
    .unwrap();
    let ObsidianReconciliationOutcome::Completed { snapshot, files } = outcome else {
        panic!("available fixture must complete");
    };
    (snapshot, files)
}

fn record<'a>(
    snapshot: &'a SourceRootSnapshot,
    relative_path: &str,
) -> &'a eam_source_obsidian::SourceRecord {
    snapshot
        .records()
        .iter()
        .find(|record| record.relative_path() == relative_path)
        .unwrap()
}

fn fixture_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../source-obsidian/tests/fixtures/obsidian-vault")
        .leak()
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn directory_digest(root: &Path) -> [u8; 32] {
    fn collect(root: &Path, current: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    let mut hasher = Sha256::new();
    for (path, content) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(content);
        hasher.update([0]);
    }
    hasher.finalize().into()
}
