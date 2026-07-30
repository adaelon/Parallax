use eam_ingestion::{
    IncrementalMaterialization, MarkdownProcessingOutcome, materialize_incremental_markdown,
    process_archived_markdown,
};
use eam_markdown::{CONTRACT_VERSION, ParseLimits};
use eam_retrieval::{AuthoritativeCandidate, RetrievalQuery, retrieve};
use eam_source_obsidian::{ObsidianSourceRepository, SourceArchiveInput, SourceFileKind};
use eam_understanding::{
    ProjectionContent, ProjectionId, ProjectionRecipe, ProjectionStatus, ProjectionTrigger,
    SourcedStatement, UnderstandingRepository, materialize_projection, rebuild_projection,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x97; 32];
const INITIAL_ALPHA: &str = "# Project\n\nStable Aurora milestone.\n";
const MOVED_ALPHA: &str =
    "# Prelude\n\nInserted context.\n\n# Project\n\nStable Aurora milestone.\n";
const MODIFIED_ALPHA: &str =
    "# Prelude\n\nInserted context.\n\n# Project\n\nChanged Aurora milestone.\n";
const BETA: &str = "# Beta\n\nIndependent source.\n";

#[test]
fn projections_survive_reopen_rebuild_artifacts_and_only_follow_safe_related_lineage() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let fixture = seed_projections(&mut repository);
    let alpha_digest = *repository
        .load_projection_recipe(fixture.alpha_projection)
        .unwrap()
        .unwrap()
        .projection()
        .material_digest();
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_deleted_artifact_rebuilds(&mut repository, fixture.alpha_projection, alpha_digest);
    let moved_ref = move_alpha_source(&mut repository, &fixture);
    assert_safe_lineage_advanced(&repository, &fixture, moved_ref);
    modify_alpha_source(&mut repository, &fixture);
    assert_related_projection_invalidated(&mut repository, &fixture);
}

#[test]
fn active_projection_routes_only_authoritative_evidence_and_invalidated_projection_stops() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let fixture = seed_projections(&mut repository);

    assert_understanding_hit(&mut repository, "Stable Aurora milestone.");
    move_alpha_source(&mut repository, &fixture);
    assert_understanding_hit(&mut repository, "Stable Aurora milestone.");
    modify_alpha_source(&mut repository, &fixture);
    let after_invalidation =
        retrieve(&mut repository, &RetrievalQuery::lexical("chronology")).unwrap();
    assert!(
        after_invalidation
            .candidates()
            .iter()
            .all(|candidate| !candidate.channels().contains_understanding())
    );
}

struct ProjectionFixture {
    root_id: u64,
    paths: Vec<String>,
    alpha_record_id: u64,
    beta_record_id: u64,
    alpha_projection: ProjectionId,
    beta_projection: ProjectionId,
}

fn seed_projections(repository: &mut VaultRepository) -> ProjectionFixture {
    let root = repository
        .register_source_root("C:/notes/understanding", 10)
        .unwrap();
    let paths = vec!["Alpha.md".to_owned(), "Beta.md".to_owned()];
    let alpha = archive_markdown(
        repository,
        root.id(),
        "Alpha.md",
        INITIAL_ALPHA,
        &paths,
        &[],
        20,
    );
    let beta = archive_markdown(
        repository,
        root.id(),
        "Beta.md",
        BETA,
        &paths,
        &[alpha.source_record_id()],
        21,
    );
    repository
        .finish_source_reconciliation(
            root.id(),
            &[alpha.source_record_id(), beta.source_record_id()],
            30,
        )
        .unwrap();
    let alpha_materialized = accept_and_materialize(repository, alpha.archive_id(), 40);
    let beta_materialized = accept_and_materialize(repository, beta.archive_id(), 50);
    let alpha_ref = alpha_materialized
        .extraction()
        .blocks()
        .last()
        .unwrap()
        .reference();
    let beta_ref = beta_materialized
        .extraction()
        .blocks()
        .last()
        .unwrap()
        .reference();
    let alpha_projection = materialize_projection(
        repository,
        recipe("Aurora", "chronology route", alpha_ref, 60),
    )
    .unwrap();
    let beta_projection =
        materialize_projection(repository, recipe("Beta", "Beta phase", beta_ref, 61)).unwrap();
    ProjectionFixture {
        root_id: root.id(),
        paths,
        alpha_record_id: alpha.source_record_id(),
        beta_record_id: beta.source_record_id(),
        alpha_projection: alpha_projection.id(),
        beta_projection: beta_projection.id(),
    }
}

fn assert_understanding_hit(repository: &mut VaultRepository, expected_verbatim: &str) {
    let result = retrieve(repository, &RetrievalQuery::lexical("chronology")).unwrap();
    assert!(result.candidates().iter().any(|candidate| {
        candidate.channels().contains_understanding()
            && matches!(
                candidate.authority(),
                AuthoritativeCandidate::Evidence(evidence)
                    if evidence.view().verbatim().contains(expected_verbatim)
            )
    }));
}

fn assert_deleted_artifact_rebuilds(
    repository: &mut VaultRepository,
    projection_id: ProjectionId,
    expected_digest: [u8; 32],
) {
    let restored = repository
        .load_projection_recipe(projection_id)
        .unwrap()
        .unwrap();
    assert_eq!(restored.projection().generation(), 1);
    repository
        .delete_understanding_artifact(projection_id)
        .unwrap();
    assert!(
        !repository
            .understanding_artifact_present(projection_id)
            .unwrap()
    );
    let rebuilt = rebuild_projection(repository, projection_id).unwrap();
    assert_eq!(rebuilt.generation(), 1);
    assert_eq!(rebuilt.material_digest(), &expected_digest);
    assert!(
        repository
            .understanding_artifact_present(projection_id)
            .unwrap()
    );
}

fn move_alpha_source(
    repository: &mut VaultRepository,
    fixture: &ProjectionFixture,
) -> eam_ingestion::EvidenceBlockRef {
    let moved = archive_markdown(
        repository,
        fixture.root_id,
        "Alpha.md",
        MOVED_ALPHA,
        &fixture.paths,
        &[fixture.alpha_record_id, fixture.beta_record_id],
        70,
    );
    repository
        .finish_source_reconciliation(
            fixture.root_id,
            &[fixture.alpha_record_id, fixture.beta_record_id],
            71,
        )
        .unwrap();
    accept_and_materialize(repository, moved.archive_id(), 72)
        .extraction()
        .blocks()
        .last()
        .unwrap()
        .reference()
}

fn assert_safe_lineage_advanced(
    repository: &VaultRepository,
    fixture: &ProjectionFixture,
    moved_ref: eam_ingestion::EvidenceBlockRef,
) {
    let advanced = repository
        .load_projection_recipe(fixture.alpha_projection)
        .unwrap()
        .unwrap();
    assert_eq!(advanced.projection().status(), ProjectionStatus::Active);
    assert_eq!(advanced.projection().generation(), 2);
    assert_eq!(advanced.recipe().sources(), vec![moved_ref]);
    assert!(
        repository
            .understanding_artifact_present(fixture.alpha_projection)
            .unwrap()
    );
    assert_unaffected(repository, fixture);
}

fn modify_alpha_source(repository: &mut VaultRepository, fixture: &ProjectionFixture) {
    let modified = archive_markdown(
        repository,
        fixture.root_id,
        "Alpha.md",
        MODIFIED_ALPHA,
        &fixture.paths,
        &[fixture.alpha_record_id, fixture.beta_record_id],
        80,
    );
    repository
        .finish_source_reconciliation(
            fixture.root_id,
            &[fixture.alpha_record_id, fixture.beta_record_id],
            81,
        )
        .unwrap();
    accept_and_materialize(repository, modified.archive_id(), 82);
}

fn assert_related_projection_invalidated(
    repository: &mut VaultRepository,
    fixture: &ProjectionFixture,
) {
    let invalidated = repository
        .load_projection_recipe(fixture.alpha_projection)
        .unwrap()
        .unwrap();
    assert_eq!(
        invalidated.projection().status(),
        ProjectionStatus::Invalidated
    );
    assert_eq!(invalidated.projection().generation(), 3);
    assert!(
        !repository
            .understanding_artifact_present(fixture.alpha_projection)
            .unwrap()
    );
    assert!(rebuild_projection(repository, fixture.alpha_projection).is_err());
    assert_unaffected(repository, fixture);
}

fn assert_unaffected(repository: &VaultRepository, fixture: &ProjectionFixture) {
    let unaffected = repository
        .load_projection_recipe(fixture.beta_projection)
        .unwrap()
        .unwrap();
    assert_eq!(unaffected.projection().generation(), 1);
    assert_eq!(unaffected.projection().status(), ProjectionStatus::Active);
}

fn recipe(
    subject: &str,
    summary: &str,
    source: eam_ingestion::EvidenceBlockRef,
    at: i64,
) -> ProjectionRecipe {
    ProjectionRecipe::new(
        ProjectionTrigger::PersonDesignated {
            reason: format!("本人指定 {subject}"),
        },
        subject,
        ProjectionContent::PhaseSummary(SourcedStatement::new(summary, vec![source]).unwrap()),
        at,
    )
    .unwrap()
}

fn archive_markdown(
    repository: &mut VaultRepository,
    root_id: u64,
    relative_path: &str,
    content: &str,
    paths: &[String],
    claimed: &[u64],
    at: i64,
) -> eam_source_obsidian::SourceArchiveReceipt {
    repository
        .archive_source_file(SourceArchiveInput {
            root_id,
            relative_path,
            observed_relative_paths: paths,
            claimed_source_record_ids: claimed,
            content: content.as_bytes(),
            kind: SourceFileKind::Markdown,
            observed_at_millis: at,
        })
        .unwrap()
}

fn accept_and_materialize(
    repository: &mut VaultRepository,
    archive_id: u64,
    at: i64,
) -> IncrementalMaterialization {
    assert!(matches!(
        process_archived_markdown(repository, archive_id, ParseLimits::default(), at, at + 1,)
            .unwrap(),
        MarkdownProcessingOutcome::Accepted { .. }
    ));
    materialize_incremental_markdown(repository, archive_id, CONTRACT_VERSION, at + 2).unwrap()
}
