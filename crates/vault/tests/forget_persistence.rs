use std::{fs, time::Duration};

use eam_core::{
    ApplicableTime, ForgetRepository, ForgetTarget, MemoryRepository, PersonTurnClassification,
    SessionId, Timestamp,
};
use eam_ingestion::{
    ImportOutcome, ImportPolicy, MarkdownProcessingOutcome, ingest_inbox_file,
    materialize_incremental_markdown, process_archived_markdown,
};
use eam_markdown::{CONTRACT_VERSION, ParseLimits};
use eam_memory::{
    LongTermMemoryRepository, MemoryBasis, MemoryConfidence, MemoryDisputeRequest, MemoryKind,
    MemoryMaintenance, MemoryProposal, MemorySubject,
};
use eam_retrieval::{AuthoritativeCandidate, RetrievalQuery, SourceScope, retrieve};
use eam_source_obsidian::{ObsidianSourceRepository, SourceArchiveInput, SourceFileKind};
use eam_understanding::{
    ProjectionContent, ProjectionRecipe, ProjectionTrigger, SourcedStatement,
    UnderstandingRepository, materialize_projection,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0x91; 32];

fn test_policy() -> ImportPolicy {
    ImportPolicy {
        stability_window: Duration::ZERO,
        auto_import_limit_bytes: 1024,
        hard_import_limit_bytes: 2048,
    }
}

#[test]
fn forgotten_conversation_evidence_and_claim_are_absent_after_reopen() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let mut core = eam_core::MemoryCore::new(
        repository,
        eam_core::ScriptedRuntime::new([PersonTurnClassification::DirectSelfReport], []),
        eam_core::IncrementingClock::new(1_000),
    );
    let (evidence_id, _) = core
        .record_person_turn(SessionId::new("forget"), "我住在深圳。")
        .unwrap();
    assert_eq!(
        retrieve(core.repository_mut(), &RetrievalQuery::lexical("深圳"))
            .unwrap()
            .candidates()
            .len(),
        1
    );

    let receipt = core
        .forget(eam_core::ForgetRequest::new(
            ForgetTarget::ConversationEvidence(evidence_id),
            true,
        ))
        .unwrap();
    assert_eq!(
        receipt.target(),
        ForgetTarget::ConversationEvidence(evidence_id)
    );
    for scope in [SourceScope::Current, SourceScope::Historical] {
        assert!(
            retrieve(
                core.repository_mut(),
                &RetrievalQuery::lexical("深圳").with_source_scope(scope),
            )
            .unwrap()
            .candidates()
            .is_empty()
        );
    }
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert!(
        MemoryRepository::evidence(&repository, evidence_id)
            .unwrap()
            .is_none()
    );
    assert!(repository.all_claims().unwrap().is_empty());
    assert_eq!(repository.deletion_intents().unwrap(), vec![receipt]);
    assert!(repository.next_evidence_id().get() > evidence_id.get());
    assert!(
        retrieve(
            &mut repository,
            &RetrievalQuery::lexical("深圳").with_source_scope(SourceScope::Historical),
        )
        .unwrap()
        .candidates()
        .is_empty()
    );
}

#[test]
fn forgotten_archive_is_unavailable_to_current_and_historical_retrieval() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let root = repository
        .register_source_root("C:/notes/forget", 10)
        .unwrap();
    let archived = repository
        .archive_source_file(SourceArchiveInput {
            root_id: root.id(),
            relative_path: "private.md",
            observed_relative_paths: &["private.md".to_owned()],
            claimed_source_record_ids: &[],
            content: b"# Private\n\nAurora deletion target.\n",
            kind: SourceFileKind::Markdown,
            observed_at_millis: 20,
        })
        .unwrap();
    repository
        .finish_source_reconciliation(root.id(), &[archived.source_record_id()], 30)
        .unwrap();
    assert!(matches!(
        process_archived_markdown(
            &mut repository,
            archived.archive_id(),
            ParseLimits::default(),
            40,
            41,
        )
        .unwrap(),
        MarkdownProcessingOutcome::Accepted { .. }
    ));
    let materialized = materialize_incremental_markdown(
        &mut repository,
        archived.archive_id(),
        CONTRACT_VERSION,
        42,
    )
    .unwrap();
    let source = materialized
        .extraction()
        .blocks()
        .last()
        .unwrap()
        .reference();
    let projection = materialize_projection(
        &mut repository,
        ProjectionRecipe::new(
            ProjectionTrigger::PersonDesignated {
                reason: "本人指定遗忘回归".to_owned(),
            },
            "Aurora",
            ProjectionContent::PhaseSummary(
                SourcedStatement::new("Aurora projection", vec![source]).unwrap(),
            ),
            43,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        retrieve(&mut repository, &RetrievalQuery::lexical("Aurora"))
            .unwrap()
            .candidates()
            .iter()
            .any(|candidate| matches!(candidate.authority(), AuthoritativeCandidate::Evidence(_)))
    );

    repository
        .commit_forget(
            ForgetTarget::ArchivedEvidence(archived.archive_id()),
            Timestamp::from_millis(50),
        )
        .unwrap()
        .expect("archive target exists");

    for scope in [SourceScope::Current, SourceScope::Historical] {
        assert!(
            retrieve(
                &mut repository,
                &RetrievalQuery::lexical("Aurora").with_source_scope(scope),
            )
            .unwrap()
            .candidates()
            .is_empty()
        );
    }
    assert!(repository.archived_evidence().unwrap().is_empty());
    assert!(
        repository
            .load_projection_recipe(projection.id())
            .unwrap()
            .is_none()
    );
}

#[test]
fn forgetting_original_evidence_removes_correction_memory_and_dispute_closure() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xA1; 32])).unwrap();
    let mut core = eam_core::MemoryCore::new(
        repository,
        eam_core::ScriptedRuntime::new(
            [
                PersonTurnClassification::DirectSelfReport,
                PersonTurnClassification::Question,
            ],
            [],
        ),
        eam_core::IncrementingClock::new(1_000),
    );
    let (original_evidence, _) = core
        .record_person_turn(SessionId::new("closure"), "我住在深圳。")
        .unwrap();
    let (counter_evidence, _) = core
        .record_person_turn(SessionId::new("closure"), "这条记忆还准确吗？")
        .unwrap();
    let original_claim = core.repository().all_claims().unwrap()[0].clone();
    let (repository, _, _) = core.into_parts();

    let mut maintenance =
        MemoryMaintenance::new(repository, eam_core::IncrementingClock::new(2_000));
    let memory = maintenance
        .propose(
            &MemoryProposal::new(original_claim.statement())
                .with_subject(MemorySubject::Person)
                .with_kind(MemoryKind::Fact)
                .with_source_claim(original_claim.id())
                .with_applicable_time(original_claim.applicable_time())
                .with_confidence(MemoryConfidence::High)
                .with_salience_reason("跨任务保留居住事实")
                .with_basis(MemoryBasis::DirectEvidence),
        )
        .unwrap();
    maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(memory.id(), memory.version(), "本人要求复核居住记忆")
                .with_counter_evidence(eam_core::EvidenceCitation::new(
                    counter_evidence,
                    "这条记忆还准确吗？",
                )),
        )
        .unwrap();
    let (repository, _) = maintenance.into_parts();
    let mut core = eam_core::MemoryCore::new(
        repository,
        eam_core::ScriptedRuntime::new([], []),
        eam_core::IncrementingClock::new(3_000),
    );
    core.correct_person_fact(
        SessionId::new("closure"),
        original_claim.id(),
        "我从 2026 年起住在香港。",
        ApplicableTime::Since(Timestamp::from_millis(2_026)),
    )
    .unwrap();
    assert_eq!(
        core.repository()
            .memory_versions(memory.id())
            .unwrap()
            .len(),
        2
    );

    core.forget(eam_core::ForgetRequest::new(
        ForgetTarget::ConversationEvidence(original_evidence),
        true,
    ))
    .unwrap();

    assert!(core.repository().all_claims().unwrap().is_empty());
    assert!(
        core.repository()
            .memory_versions(memory.id())
            .unwrap()
            .is_empty()
    );
    assert!(
        core.repository()
            .memory_disputes(memory.id())
            .unwrap()
            .is_empty()
    );
    for query in ["深圳", "香港"] {
        for scope in [SourceScope::Current, SourceScope::Historical] {
            assert!(
                retrieve(
                    core.repository_mut(),
                    &RetrievalQuery::lexical(query).with_source_scope(scope),
                )
                .unwrap()
                .candidates()
                .is_empty()
            );
        }
    }
}

#[test]
fn forgetting_any_archive_version_removes_the_stable_source_history() {
    let vault = tempdir().unwrap();
    let mut repository = VaultRepository::open(vault.path(), VaultKey::new([0xB1; 32])).unwrap();
    let root = repository
        .register_source_root("C:/notes/versions", 10)
        .unwrap();
    let paths = ["history.md".to_owned()];
    let first = repository
        .archive_source_file(SourceArchiveInput {
            root_id: root.id(),
            relative_path: "history.md",
            observed_relative_paths: &paths,
            claimed_source_record_ids: &[],
            content: b"# History\n\nFirst private version.\n",
            kind: SourceFileKind::Markdown,
            observed_at_millis: 20,
        })
        .unwrap();
    let second = repository
        .archive_source_file(SourceArchiveInput {
            root_id: root.id(),
            relative_path: "history.md",
            observed_relative_paths: &paths,
            claimed_source_record_ids: &[first.source_record_id()],
            content: b"# History\n\nSecond private version.\n",
            kind: SourceFileKind::Markdown,
            observed_at_millis: 30,
        })
        .unwrap();
    assert_eq!(first.source_record_id(), second.source_record_id());
    assert_eq!(repository.archived_evidence().unwrap().len(), 2);

    repository
        .commit_forget(
            ForgetTarget::ArchivedEvidence(first.archive_id()),
            Timestamp::from_millis(40),
        )
        .unwrap()
        .unwrap();

    assert!(repository.archived_evidence().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(vault.path().join("objects")).unwrap().count(),
        0
    );
}

#[test]
fn shared_ciphertext_is_deleted_only_after_its_last_archive_reference() {
    let vault = tempdir().unwrap();
    let inbox = tempdir().unwrap();
    let first_path = inbox.path().join("first.md");
    let second_path = inbox.path().join("second.md");
    fs::write(&first_path, b"same private evidence").unwrap();
    fs::write(&second_path, b"same private evidence").unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let ImportOutcome::Archived(first) =
        ingest_inbox_file(&mut repository, &first_path, &test_policy(), false, 10).unwrap()
    else {
        panic!("first archive");
    };
    let ImportOutcome::Archived(second) =
        ingest_inbox_file(&mut repository, &second_path, &test_policy(), false, 20).unwrap()
    else {
        panic!("second archive");
    };
    let objects = vault.path().join("objects");
    assert_eq!(fs::read_dir(&objects).unwrap().count(), 1);

    repository
        .commit_forget(
            ForgetTarget::ArchivedEvidence(first.archive_id),
            Timestamp::from_millis(30),
        )
        .unwrap()
        .unwrap();
    assert_eq!(fs::read_dir(&objects).unwrap().count(), 1);
    assert_eq!(
        repository.read_archived_content(second.archive_id).unwrap(),
        b"same private evidence"
    );

    repository
        .commit_forget(
            ForgetTarget::ArchivedEvidence(second.archive_id),
            Timestamp::from_millis(40),
        )
        .unwrap()
        .unwrap();
    assert_eq!(fs::read_dir(&objects).unwrap().count(), 0);
    repository.close().unwrap();

    let third_path = inbox.path().join("third.md");
    fs::write(&third_path, b"new evidence after forgetting").unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let ImportOutcome::Archived(third) =
        ingest_inbox_file(&mut repository, &third_path, &test_policy(), false, 50).unwrap()
    else {
        panic!("third archive");
    };
    assert!(third.archive_id > second.archive_id);
}
