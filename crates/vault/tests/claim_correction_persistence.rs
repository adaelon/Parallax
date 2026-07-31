use eam_core::{
    ApplicableTime, ClaimId, ClaimStatus, IncrementingClock, MemoryCore, MemoryRepository,
    PersonTurnClassification, ScriptedRuntime, SessionId, Timestamp,
};
use eam_memory::{
    LongTermMemoryRepository, MemoryBasis, MemoryConfidence, MemoryDisputeOutcome,
    MemoryDisputeRequest, MemoryKind, MemoryMaintenance, MemoryProposal, MemoryStatus,
    MemorySubject,
};
use eam_retrieval::{
    AuthoritativeCandidate, IndexDisposition, RetrievalQuery, RetrievalResult, SourceScope,
    retrieve,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0xC8; 32];

#[test]
fn correction_propagates_only_to_affected_memory_and_survives_reopen() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new(
            [
                PersonTurnClassification::DirectSelfReport,
                PersonTurnClassification::DirectSelfReport,
            ],
            [],
        ),
        IncrementingClock::new(1_000),
    );
    core.record_person_turn(SessionId::new("facts"), "我住在深圳。")
        .unwrap();
    core.record_person_turn(SessionId::new("facts"), "我喜欢徒步。")
        .unwrap();
    let claims = core.repository().all_claims().unwrap();
    let old_home = claims[0].clone();
    let hiking = claims[1].clone();
    let (repository, _, _) = core.into_parts();

    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(2_000));
    let home_memory = maintenance
        .propose(&direct_memory(
            old_home.statement(),
            old_home.id(),
            old_home.applicable_time(),
        ))
        .unwrap();
    let hiking_memory = maintenance
        .propose(&direct_memory(
            hiking.statement(),
            hiking.id(),
            hiking.applicable_time(),
        ))
        .unwrap();
    let (repository, _) = maintenance.into_parts();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([], []),
        IncrementingClock::new(3_000),
    );

    let receipt = core
        .correct_person_fact(
            SessionId::new("facts"),
            old_home.id(),
            "我从 2026 年起住在香港。",
            ApplicableTime::Since(Timestamp::from_millis(2_026)),
        )
        .unwrap();
    assert_eq!(correction_counts(receipt), (1, 1, 0, 2));

    let replacement_id = receipt.replacement_claim_id();
    let repository = core.repository();
    let home_versions = repository.memory_versions(home_memory.id()).unwrap();
    assert_eq!(home_versions.len(), 2);
    assert_eq!(home_versions[0].status(), MemoryStatus::Superseded);
    assert_eq!(home_versions[1].status(), MemoryStatus::Active);
    assert_eq!(home_versions[1].statement(), "我从 2026 年起住在香港。");
    assert_eq!(home_versions[1].source_claim_ids(), &[replacement_id]);
    assert_eq!(
        repository.current_memory(hiking_memory.id()).unwrap(),
        Some(hiking_memory.clone())
    );

    assert!(
        !ledger_claim_ids(&mut core, &RetrievalQuery::lexical("深圳")).contains(&old_home.id())
    );
    let current_replacement =
        retrieve(core.repository_mut(), &RetrievalQuery::lexical("香港")).unwrap();
    assert_eq!(
        current_replacement.index().disposition(),
        IndexDisposition::Current
    );
    assert!(ledger_ids(&current_replacement).contains(&replacement_id));
    assert_historical_supersession(&mut core, old_home.id(), replacement_id);

    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 18);
    let claims = repository.all_claims().unwrap();
    assert_eq!(claims.len(), 3);
    assert_eq!(claims[0].status(), ClaimStatus::Superseded);
    assert_eq!(claims[0].superseded_by(), Some(replacement_id));
    assert_eq!(claims[2].supersedes(), Some(old_home.id()));
    assert_eq!(
        repository
            .current_memory(home_memory.id())
            .unwrap()
            .unwrap()
            .source_claim_ids(),
        &[replacement_id]
    );
    assert!(
        !ledger_claim_ids_from_repository(&mut repository, &RetrievalQuery::lexical("深圳"))
            .contains(&old_home.id())
    );
    assert!(
        ledger_claim_ids_from_repository(&mut repository, &RetrievalQuery::lexical("香港"))
            .contains(&replacement_id)
    );
}

#[test]
fn interpretive_memory_is_invalidated_for_review_instead_of_being_semantically_rewritten() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xD8; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([PersonTurnClassification::DirectSelfReport], []),
        IncrementingClock::new(1_000),
    );
    core.record_person_turn(SessionId::new("facts"), "我住在深圳。")
        .unwrap();
    let old_home = core.repository().all_claims().unwrap()[0].clone();
    let (repository, _, _) = core.into_parts();
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(2_000));
    let interpretation = maintenance
        .propose(
            &MemoryProposal::new("深圳生活塑造了我的日常节奏")
                .with_subject(MemorySubject::Person)
                .with_kind(MemoryKind::Hypothesis)
                .with_source_claim(old_home.id())
                .with_applicable_time(old_home.applicable_time())
                .with_confidence(MemoryConfidence::High)
                .with_salience_reason("跨任务保留对生活阶段的解释")
                .with_basis(MemoryBasis::InterpretiveInference),
        )
        .unwrap();
    let (repository, _) = maintenance.into_parts();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([], []),
        IncrementingClock::new(3_000),
    );

    let receipt = core
        .correct_person_fact(
            SessionId::new("facts"),
            old_home.id(),
            "我从 2026 年起住在香港。",
            ApplicableTime::Since(Timestamp::from_millis(2_026)),
        )
        .unwrap();
    assert_eq!(receipt.invalidated_memories(), 1);
    assert_eq!(receipt.rebuilt_memories(), 0);
    let stored = core
        .repository()
        .current_memory(interpretation.id())
        .unwrap()
        .unwrap();
    assert_eq!(stored.version(), interpretation.version());
    assert_eq!(stored.status(), MemoryStatus::Superseded);
    assert!(
        !ledger_claim_ids(&mut core, &RetrievalQuery::lexical("塑造节奏")).contains(&old_home.id())
    );
}

#[test]
fn correction_preserves_an_old_open_dispute_without_blocking_the_successor_version() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xE8; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new(
            [
                PersonTurnClassification::DirectSelfReport,
                PersonTurnClassification::DirectSelfReport,
            ],
            [],
        ),
        IncrementingClock::new(1_000),
    );
    core.record_person_turn(SessionId::new("facts"), "我住在深圳。")
        .unwrap();
    core.record_person_turn(SessionId::new("facts"), "我不再住在深圳。")
        .unwrap();
    let claims = core.repository().all_claims().unwrap();
    let old_home = claims[0].clone();
    let first_counter_evidence = claims[1].support()[0].clone();
    let second_counter_evidence = old_home.support()[0].clone();
    let (repository, _, _) = core.into_parts();

    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(2_000));
    let home_memory = maintenance
        .propose(&direct_memory(
            old_home.statement(),
            old_home.id(),
            old_home.applicable_time(),
        ))
        .unwrap();
    let first_dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                home_memory.id(),
                home_memory.version(),
                "这条居住记忆已经过时。",
            )
            .with_counter_evidence(first_counter_evidence),
        )
        .unwrap();
    let (repository, _) = maintenance.into_parts();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([], []),
        IncrementingClock::new(3_000),
    );
    core.correct_person_fact(
        SessionId::new("facts"),
        old_home.id(),
        "我从 2026 年起住在香港。",
        ApplicableTime::Since(Timestamp::from_millis(2_026)),
    )
    .unwrap();
    let successor = core
        .repository()
        .current_memory(home_memory.id())
        .unwrap()
        .unwrap();
    assert_eq!(successor.version(), 2);
    assert_eq!(successor.status(), MemoryStatus::Active);

    let (repository, _, _) = core.into_parts();
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(4_000));
    let second_dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                successor.id(),
                successor.version(),
                "后继记忆也有新的反证。",
            )
            .with_counter_evidence(second_counter_evidence),
        )
        .expect("an open dispute on a superseded version must not block its successor");

    let disputes = maintenance
        .repository()
        .memory_disputes(home_memory.id())
        .unwrap();
    assert_eq!(disputes.len(), 2);
    assert_eq!(disputes[0].id(), first_dispute.id());
    assert_eq!(disputes[0].memory_version(), 1);
    assert_eq!(disputes[0].outcome(), MemoryDisputeOutcome::Open);
    assert_eq!(disputes[1].id(), second_dispute.id());
    assert_eq!(disputes[1].memory_version(), 2);
    assert_eq!(disputes[1].outcome(), MemoryDisputeOutcome::Open);
}

fn direct_memory(
    statement: &str,
    claim_id: ClaimId,
    applicable_time: ApplicableTime,
) -> MemoryProposal {
    MemoryProposal::new(statement)
        .with_subject(MemorySubject::Person)
        .with_kind(MemoryKind::Fact)
        .with_source_claim(claim_id)
        .with_applicable_time(applicable_time)
        .with_confidence(MemoryConfidence::High)
        .with_salience_reason("跨任务保留本人当前事实")
        .with_basis(MemoryBasis::DirectEvidence)
}

fn correction_counts(receipt: eam_core::ClaimCorrectionReceipt) -> (usize, usize, usize, usize) {
    (
        receipt.invalidated_memories(),
        receipt.rebuilt_memories(),
        receipt.invalidated_projections(),
        receipt.reindexed_claims(),
    )
}

fn ledger_claim_ids(
    core: &mut MemoryCore<VaultRepository, ScriptedRuntime, IncrementingClock>,
    query: &RetrievalQuery,
) -> Vec<ClaimId> {
    ledger_ids(&retrieve(core.repository_mut(), query).unwrap())
}

fn ledger_claim_ids_from_repository(
    repository: &mut VaultRepository,
    query: &RetrievalQuery,
) -> Vec<ClaimId> {
    ledger_ids(&retrieve(repository, query).unwrap())
}

fn ledger_ids(result: &RetrievalResult) -> Vec<ClaimId> {
    result
        .candidates()
        .iter()
        .filter_map(|candidate| match candidate.authority() {
            AuthoritativeCandidate::Ledger(claim) => Some(claim.id()),
            AuthoritativeCandidate::Evidence(_) => None,
        })
        .collect()
}

fn assert_historical_supersession(
    core: &mut MemoryCore<VaultRepository, ScriptedRuntime, IncrementingClock>,
    superseded_id: ClaimId,
    replacement_id: ClaimId,
) {
    let historical = retrieve(
        core.repository_mut(),
        &RetrievalQuery::lexical("深圳").with_source_scope(SourceScope::Historical),
    )
    .unwrap();
    let historical_old = historical
        .candidates()
        .iter()
        .find_map(|candidate| match candidate.authority() {
            AuthoritativeCandidate::Ledger(claim) if claim.id() == superseded_id => Some(claim),
            _ => None,
        })
        .expect("historical query keeps the superseded claim");
    assert_eq!(historical_old.status(), ClaimStatus::Superseded);
    assert_eq!(historical_old.superseded_by(), Some(replacement_id));
}
