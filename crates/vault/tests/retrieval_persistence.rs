use eam_core::{
    ApplicableTime, Claim, ClaimOwner, ConversationEvidence, EvidenceCitation, MemoryRepository,
    SessionId, Speaker, Timestamp,
};
use eam_ingestion::{
    MarkdownProcessingOutcome, materialize_incremental_markdown, process_archived_markdown,
};
use eam_markdown::{CONTRACT_VERSION, ParseLimits};
use eam_retrieval::{
    AuthoritativeCandidate, IndexDisposition, RetrievalQuery, SourceCurrentness, SourceScope,
    TimeRange, retrieve,
};
use eam_source_obsidian::{
    ObsidianSourceRepository, SourceArchiveInput, SourceFileKind, SourceRecordState,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x83; 32];
const ALPHA: &str = "# Alpha Project\n\nI coordinate Aurora with [[Target Person]].\n";
const TARGET: &str =
    "---\naliases: [Target Person]\n---\n# Target\n\nRelated biography evidence.\n";

#[test]
fn authoritative_multi_channel_retrieval_survives_scope_changes_and_reopen() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let root = repository
        .register_source_root("C:/notes/retrieval", 10)
        .unwrap();
    let (alpha_record_id, target_record_id) = archive_relation_pair(&mut repository, root.id());

    let lexical = retrieve(&mut repository, &RetrievalQuery::lexical("Aurora")).unwrap();
    assert_eq!(lexical.index().disposition(), IndexDisposition::Rebuilt);
    assert!(
        evidence_quotes(&lexical)
            .iter()
            .any(|quote| quote.contains("Aurora"))
    );
    assert!(lexical.candidates().iter().any(|candidate| {
        candidate.channels().contains_lexical()
            && matches!(candidate.authority(), AuthoritativeCandidate::Evidence(_))
    }));

    let related = retrieve(
        &mut repository,
        &RetrievalQuery::related_to("Target Person"),
    )
    .unwrap();
    assert!(related.index().relations() >= 1);
    assert!(related.candidates().iter().any(|candidate| {
        candidate.channels().contains_relation()
            && matches!(
                    candidate.authority(),
                    AuthoritativeCandidate::Evidence(evidence)
                        if evidence.view().verbatim().contains("Aurora")
            )
    }));

    append_and_assert_city_time(&mut repository);

    assert_source_scope_after_removal(
        &mut repository,
        root.id(),
        alpha_record_id,
        target_record_id,
    );
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let reopened = retrieve(
        &mut repository,
        &RetrievalQuery::lexical("biography").with_source_scope(SourceScope::Historical),
    )
    .unwrap();
    assert_eq!(reopened.index().disposition(), IndexDisposition::Current);
    assert!(!evidence_quotes(&reopened).is_empty());
}

fn append_and_assert_city_time(repository: &mut VaultRepository) {
    append_city_claim(
        repository,
        "My city was Beijing",
        ApplicableTime::Between {
            start: Timestamp::from_millis(100),
            end: Timestamp::from_millis(199),
        },
        300,
    );
    append_city_claim(
        repository,
        "My city is Shanghai",
        ApplicableTime::Since(Timestamp::from_millis(200)),
        301,
    );
    append_city_claim(
        repository,
        "My hobby was pottery",
        ApplicableTime::Between {
            start: Timestamp::from_millis(100),
            end: Timestamp::from_millis(199),
        },
        302,
    );
    let old_city = retrieve(
        repository,
        &RetrievalQuery::lexical("city").with_time(TimeRange::at(150)),
    )
    .unwrap();
    assert_eq!(ledger_statements(&old_city), vec!["My city was Beijing"]);
    let current_city = retrieve(
        repository,
        &RetrievalQuery::lexical("city").with_time(TimeRange::at(250)),
    )
    .unwrap();
    assert_eq!(
        ledger_statements(&current_city),
        vec!["My city is Shanghai"]
    );
}

fn assert_source_scope_after_removal(
    repository: &mut VaultRepository,
    root_id: u64,
    alpha_record_id: u64,
    target_record_id: u64,
) {
    repository
        .finish_source_reconciliation(root_id, &[alpha_record_id], 500)
        .unwrap();
    repository.refresh_source_relations(root_id).unwrap();
    assert_eq!(
        repository
            .load_source_root(root_id)
            .unwrap()
            .records()
            .iter()
            .find(|record| record.id() == target_record_id)
            .unwrap()
            .state(),
        SourceRecordState::SourceRemoved
    );
    let current = retrieve(
        repository,
        &RetrievalQuery::lexical("biography").with_source_scope(SourceScope::Current),
    )
    .unwrap();
    assert!(evidence_quotes(&current).is_empty());
    let historical = retrieve(
        repository,
        &RetrievalQuery::lexical("biography").with_source_scope(SourceScope::Historical),
    )
    .unwrap();
    assert!(historical.candidates().iter().any(|candidate| {
        matches!(
            candidate.authority(),
            AuthoritativeCandidate::Evidence(evidence)
                if evidence.currentness() == SourceCurrentness::SourceRemoved
            && evidence.view().verbatim().contains("biography")
        )
    }));
}

fn archive_relation_pair(repository: &mut VaultRepository, root_id: u64) -> (u64, u64) {
    let paths = vec!["Alpha.md".to_owned(), "Target.md".to_owned()];
    let alpha = archive_markdown(repository, root_id, "Alpha.md", ALPHA, &paths, &[], 20);
    let target = archive_markdown(
        repository,
        root_id,
        "Target.md",
        TARGET,
        &paths,
        &[alpha.source_record_id()],
        21,
    );
    repository
        .finish_source_reconciliation(
            root_id,
            &[alpha.source_record_id(), target.source_record_id()],
            30,
        )
        .unwrap();
    accept_and_materialize(repository, alpha.archive_id(), 40);
    accept_and_materialize(repository, target.archive_id(), 50);
    repository.refresh_source_relations(root_id).unwrap();
    (alpha.source_record_id(), target.source_record_id())
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

fn accept_and_materialize(repository: &mut VaultRepository, archive_id: u64, at: i64) {
    assert!(matches!(
        process_archived_markdown(repository, archive_id, ParseLimits::default(), at, at + 1,)
            .unwrap(),
        MarkdownProcessingOutcome::Accepted { .. }
    ));
    materialize_incremental_markdown(repository, archive_id, CONTRACT_VERSION, at + 2).unwrap();
}

fn append_city_claim(
    repository: &mut VaultRepository,
    statement: &str,
    applicable_time: ApplicableTime,
    recorded_at: i64,
) {
    let evidence_id = repository.next_evidence_id();
    repository
        .append_evidence(ConversationEvidence::restore(
            evidence_id,
            SessionId::new("retrieval-time-fixture"),
            Speaker::Person,
            statement.to_owned(),
            Timestamp::from_millis(recorded_at),
        ))
        .unwrap();
    let claim_id = repository.next_claim_id();
    repository
        .append_claim(Claim::restore(
            claim_id,
            ClaimOwner::Person,
            statement.to_owned(),
            vec![EvidenceCitation::new(evidence_id, statement)],
            None,
            applicable_time,
            Timestamp::from_millis(recorded_at),
        ))
        .unwrap();
}

fn evidence_quotes(result: &eam_retrieval::RetrievalResult) -> Vec<&str> {
    result
        .candidates()
        .iter()
        .filter_map(|candidate| match candidate.authority() {
            AuthoritativeCandidate::Evidence(evidence) => Some(evidence.view().verbatim()),
            AuthoritativeCandidate::Ledger(_) => None,
        })
        .collect()
}

fn ledger_statements(result: &eam_retrieval::RetrievalResult) -> Vec<&str> {
    result
        .candidates()
        .iter()
        .filter_map(|candidate| match candidate.authority() {
            AuthoritativeCandidate::Ledger(claim) => Some(claim.statement()),
            AuthoritativeCandidate::Evidence(_) => None,
        })
        .collect()
}
