use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, EvidenceCitation, EvidenceId,
    IncrementingClock, MemoryRepository, RetrievedContextItem, SessionId, Speaker, Timestamp,
    Uncertainty,
};
use eam_memory::{
    LongTermMemoryRepository, MemoryBasis, MemoryConfidence, MemoryDisputeOutcome,
    MemoryDisputeRequest, MemoryDisputeReview, MemoryId, MemoryKind, MemoryMaintenance,
    MemoryProposal, MemoryStatus, MemorySubject,
};
use eam_retrieval::{
    AuthoritativeCandidate, RetrievalQuery, TimeRange, TokenBudget, freeze_working_context,
    retrieve,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0xA6; 32];

#[test]
fn explicit_versions_survive_reopen_and_only_current_memory_sources_are_recalled() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    seed_claim(
        &mut repository,
        1,
        ClaimOwner::Person,
        "I live in Shenzhen",
        None,
        ApplicableTime::Since(Timestamp::from_millis(10)),
    );
    seed_claim(
        &mut repository,
        2,
        ClaimOwner::Person,
        "I live in Guangzhou",
        None,
        ApplicableTime::Since(Timestamp::from_millis(40)),
    );
    seed_claim(
        &mut repository,
        3,
        ClaimOwner::Counterpart,
        "Their planning rhythm appears steadier",
        Some(Uncertainty::Medium),
        ApplicableTime::Since(Timestamp::from_millis(20)),
    );
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(1_000));
    let first = maintenance
        .propose(&direct_proposal(
            "I live in Shenzhen",
            ClaimId::from_raw(1),
            10,
        ))
        .unwrap();
    let second = maintenance
        .propose(
            &direct_proposal("I live in Guangzhou", ClaimId::from_raw(2), 40)
                .revising(first.id(), first.version()),
        )
        .unwrap();
    let inference = maintenance
        .propose(
            &MemoryProposal::new("continuity hypothesis for future planning")
                .with_subject(MemorySubject::Counterpart)
                .with_kind(MemoryKind::Hypothesis)
                .with_source_claim(ClaimId::from_raw(3))
                .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(20)))
                .with_confidence(MemoryConfidence::Medium)
                .with_salience_reason("Useful in future planning conversations")
                .with_basis(MemoryBasis::InterpretiveInference),
        )
        .unwrap();
    let (mut repository, _) = maintenance.into_parts();

    assert_eq!(second.version(), 2);
    assert_eq!(inference.status(), MemoryStatus::Provisional);
    assert_long_term_claim(
        &mut repository,
        "continuity hypothesis",
        ClaimId::from_raw(3),
    );
    assert_time_gated_long_term_claim(&mut repository, ClaimId::from_raw(3));
    assert_no_long_term_claim(&mut repository, "Shenzhen", ClaimId::from_raw(1));
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 17);
    let versions = repository.memory_versions(first.id()).unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].status(), MemoryStatus::Superseded);
    assert_eq!(versions[1].status(), MemoryStatus::Active);
    assert_eq!(repository.current_memory(first.id()).unwrap(), Some(second));
    assert_eq!(
        repository.current_memory(inference.id()).unwrap(),
        Some(inference)
    );
    assert_long_term_claim(
        &mut repository,
        "continuity hypothesis",
        ClaimId::from_raw(3),
    );
}

#[test]
fn ledger_rows_remain_zero_memory_state_until_an_explicit_proposal_commits() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    seed_claim(
        &mut repository,
        1,
        ClaimOwner::Shared,
        "We agreed to revisit this next month",
        None,
        ApplicableTime::At(Timestamp::from_millis(30)),
    );
    assert!(repository.all_memory_versions().unwrap().is_empty());
    assert_no_long_term_claim(
        &mut repository,
        "revisit this next month",
        ClaimId::from_raw(1),
    );
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert!(repository.all_memory_versions().unwrap().is_empty());
}

#[test]
fn maintained_dispute_survives_reopen_and_freezes_only_as_a_complete_relevant_pair() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    seed_claim(
        &mut repository,
        1,
        ClaimOwner::Counterpart,
        "Planning has become steadier",
        Some(Uncertainty::Medium),
        ApplicableTime::Since(Timestamp::from_millis(10)),
    );
    seed_evidence(
        &mut repository,
        2,
        Speaker::Person,
        "That week was exceptional",
    );
    seed_evidence(
        &mut repository,
        3,
        Speaker::Counterpart,
        "The broader sequence still supports the interpretation",
    );
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(2_000));
    let memory = maintenance
        .propose(
            &MemoryProposal::new("Planning has become steadier")
                .with_subject(MemorySubject::Counterpart)
                .with_kind(MemoryKind::Hypothesis)
                .with_source_claim(ClaimId::from_raw(1))
                .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
                .with_confidence(MemoryConfidence::Medium)
                .with_salience_reason("Useful in future planning conversations")
                .with_basis(MemoryBasis::InterpretiveInference),
        )
        .unwrap();
    let dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                memory.id(),
                memory.version(),
                "One exceptional week should not define the pattern",
            )
            .with_counter_evidence(EvidenceCitation::new(
                EvidenceId::from_raw(2),
                "That week was exceptional",
            )),
        )
        .unwrap();
    maintenance
        .review_dispute(
            &MemoryDisputeReview::maintain(dispute.id(), "The longer sequence remains persuasive")
                .with_evidence(EvidenceCitation::new(
                    EvidenceId::from_raw(3),
                    "The broader sequence still supports the interpretation",
                )),
        )
        .unwrap();
    let (repository, _) = maintenance.into_parts();
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 17);
    assert_eq!(
        repository
            .current_memory(memory.id())
            .unwrap()
            .unwrap()
            .status(),
        MemoryStatus::Disputed
    );
    let disputes = repository.memory_disputes(memory.id()).unwrap();
    assert_eq!(disputes.len(), 1);
    assert_eq!(disputes[0].outcome(), MemoryDisputeOutcome::Maintained);

    assert_relevant_dispute_pair_and_unrelated_absence(&mut repository, memory.id());
}

#[test]
fn revised_dispute_survives_reopen_with_its_successor_version_link() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    seed_claim(
        &mut repository,
        1,
        ClaimOwner::Counterpart,
        "Planning has become steadier",
        Some(Uncertainty::Medium),
        ApplicableTime::Since(Timestamp::from_millis(10)),
    );
    seed_evidence(
        &mut repository,
        2,
        Speaker::Person,
        "That week was exceptional",
    );
    seed_evidence(
        &mut repository,
        3,
        Speaker::Counterpart,
        "The objection supports a narrower interpretation",
    );
    seed_claim(
        &mut repository,
        4,
        ClaimOwner::Counterpart,
        "Busy-week planning can become less steady",
        Some(Uncertainty::Medium),
        ApplicableTime::Since(Timestamp::from_millis(10)),
    );
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(3_000));
    let memory = maintenance
        .propose(
            &MemoryProposal::new("Planning has become steadier")
                .with_subject(MemorySubject::Counterpart)
                .with_kind(MemoryKind::Hypothesis)
                .with_source_claim(ClaimId::from_raw(1))
                .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
                .with_confidence(MemoryConfidence::Medium)
                .with_salience_reason("Useful in future planning conversations")
                .with_basis(MemoryBasis::InterpretiveInference),
        )
        .unwrap();
    let dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(
                memory.id(),
                memory.version(),
                "The conclusion is too broad",
            )
            .with_counter_evidence(EvidenceCitation::new(
                EvidenceId::from_raw(2),
                "That week was exceptional",
            )),
        )
        .unwrap();
    let revision = MemoryProposal::new("Busy-week planning can become less steady")
        .with_subject(MemorySubject::Counterpart)
        .with_kind(MemoryKind::Hypothesis)
        .with_source_claims([ClaimId::from_raw(1), ClaimId::from_raw(4)])
        .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
        .with_confidence(MemoryConfidence::Medium)
        .with_salience_reason("Retain the narrower, evidence-bounded interpretation")
        .with_basis(MemoryBasis::InterpretiveInference)
        .revising(memory.id(), memory.version());
    let resolution = maintenance
        .review_dispute(
            &MemoryDisputeReview::revise(
                dispute.id(),
                "The objection supports a narrower interpretation",
                revision,
            )
            .with_evidence(EvidenceCitation::new(
                EvidenceId::from_raw(3),
                "The objection supports a narrower interpretation",
            )),
        )
        .unwrap();
    assert_eq!(resolution.dispute().revised_version(), Some(2));
    let (repository, _) = maintenance.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let disputes = repository.memory_disputes(memory.id()).unwrap();
    assert_eq!(disputes.len(), 1);
    assert_eq!(disputes[0].outcome(), MemoryDisputeOutcome::Revised);
    assert_eq!(disputes[0].revised_version(), Some(2));
    let versions = repository.memory_versions(memory.id()).unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].status(), MemoryStatus::Superseded);
    assert_eq!(versions[1].predecessor_version(), Some(memory.version()));
    assert_eq!(
        versions[1].statement(),
        "Busy-week planning can become less steady"
    );
}

#[test]
fn retracted_dispute_survives_reopen_and_contributes_no_recall_path() {
    let vault = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    seed_claim(
        &mut repository,
        1,
        ClaimOwner::Counterpart,
        "Planning has become steadier",
        Some(Uncertainty::Medium),
        ApplicableTime::Since(Timestamp::from_millis(10)),
    );
    seed_evidence(&mut repository, 2, Speaker::Person, "A direct exception");
    seed_evidence(
        &mut repository,
        3,
        Speaker::Counterpart,
        "The exception defeats the conclusion",
    );
    let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(4_000));
    let memory = maintenance
        .propose(
            &MemoryProposal::new("Planning has become steadier")
                .with_subject(MemorySubject::Counterpart)
                .with_kind(MemoryKind::Hypothesis)
                .with_source_claim(ClaimId::from_raw(1))
                .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(10)))
                .with_confidence(MemoryConfidence::Medium)
                .with_salience_reason("Useful in future planning conversations")
                .with_basis(MemoryBasis::InterpretiveInference),
        )
        .unwrap();
    let dispute = maintenance
        .raise_dispute(
            &MemoryDisputeRequest::new(memory.id(), memory.version(), "A direct exception exists")
                .with_counter_evidence(EvidenceCitation::new(
                    EvidenceId::from_raw(2),
                    "A direct exception",
                )),
        )
        .unwrap();
    maintenance
        .review_dispute(
            &MemoryDisputeReview::retract(dispute.id(), "The exception changes my view")
                .with_evidence(EvidenceCitation::new(
                    EvidenceId::from_raw(3),
                    "The exception defeats the conclusion",
                )),
        )
        .unwrap();
    let (repository, _) = maintenance.into_parts();
    repository.close().unwrap();

    let mut repository =
        VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_eq!(
        repository
            .current_memory(memory.id())
            .unwrap()
            .unwrap()
            .status(),
        MemoryStatus::Retracted
    );
    let result = retrieve(
        &mut repository,
        &RetrievalQuery::lexical("planning steadier"),
    )
    .unwrap();
    assert!(result.disputed_memories().is_empty());
    assert!(result.candidates().iter().all(|candidate| {
        !candidate.channels().contains_long_term_memory()
            || !matches!(
                candidate.authority(),
                AuthoritativeCandidate::Ledger(claim) if claim.id() == ClaimId::from_raw(1)
            )
    }));
}

fn direct_proposal(statement: &str, claim_id: ClaimId, since_millis: i64) -> MemoryProposal {
    MemoryProposal::new(statement)
        .with_subject(MemorySubject::Person)
        .with_kind(MemoryKind::Fact)
        .with_source_claim(claim_id)
        .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(since_millis)))
        .with_confidence(MemoryConfidence::High)
        .with_salience_reason("Needed across future conversations")
        .with_basis(MemoryBasis::DirectEvidence)
}

fn seed_claim(
    repository: &mut VaultRepository,
    id: u64,
    owner: ClaimOwner,
    statement: &str,
    uncertainty: Option<Uncertainty>,
    applicable_time: ApplicableTime,
) {
    let evidence = ConversationEvidence::restore(
        EvidenceId::from_raw(id),
        SessionId::new("memory-test"),
        Speaker::Person,
        statement.to_owned(),
        Timestamp::from_millis(i64::try_from(id).unwrap()),
    );
    repository.append_evidence(evidence).unwrap();
    repository
        .append_claim(Claim::restore(
            ClaimId::from_raw(id),
            owner,
            statement.to_owned(),
            vec![EvidenceCitation::new(EvidenceId::from_raw(id), statement)],
            uncertainty,
            applicable_time,
            Timestamp::from_millis(i64::try_from(id).unwrap()),
        ))
        .unwrap();
}

fn seed_evidence(repository: &mut VaultRepository, id: u64, speaker: Speaker, statement: &str) {
    repository
        .append_evidence(ConversationEvidence::restore(
            EvidenceId::from_raw(id),
            SessionId::new("memory-dispute-persistence"),
            speaker,
            statement.to_owned(),
            Timestamp::from_millis(i64::try_from(id).unwrap()),
        ))
        .unwrap();
}

fn assert_relevant_dispute_pair_and_unrelated_absence(
    repository: &mut VaultRepository,
    memory_id: MemoryId,
) {
    let related = freeze_working_context(
        repository,
        &RetrievalQuery::lexical("planning steadier"),
        TokenBudget::new(512).unwrap(),
        Vec::new(),
        Timestamp::from_millis(3_000),
    )
    .unwrap();
    let pair = related
        .retrieved()
        .iter()
        .find_map(|item| match item {
            RetrievedContextItem::MemoryDispute(pair) => Some(pair),
            _ => None,
        })
        .expect("the directly relevant dispute must be frozen as one pair");
    assert_eq!(pair.memory_id(), memory_id.get());
    assert_eq!(pair.counterpart_view(), "Planning has become steadier");
    assert_eq!(pair.counterpart_sources().len(), 1);
    assert_eq!(
        pair.person_position(),
        "One exceptional week should not define the pattern"
    );
    assert_eq!(pair.person_evidence().len(), 1);
    assert_eq!(pair.review_evidence().len(), 1);
    assert_eq!(pair.state(), eam_core::DisputeState::Maintained);

    let unrelated = freeze_working_context(
        repository,
        &RetrievalQuery::lexical("holiday recipes"),
        TokenBudget::new(512).unwrap(),
        Vec::new(),
        Timestamp::from_millis(3_001),
    )
    .unwrap();
    assert!(
        unrelated
            .retrieved()
            .iter()
            .all(|item| !matches!(item, RetrievedContextItem::MemoryDispute(_)))
    );
}

fn assert_long_term_claim(repository: &mut VaultRepository, query: &str, expected_claim: ClaimId) {
    let result = retrieve(repository, &RetrievalQuery::lexical(query)).unwrap();
    assert!(result.candidates().iter().any(|candidate| {
        candidate.channels().contains_long_term_memory()
            && matches!(
                candidate.authority(),
                AuthoritativeCandidate::Ledger(claim) if claim.id() == expected_claim
            )
    }));
}

fn assert_no_long_term_claim(
    repository: &mut VaultRepository,
    query: &str,
    unexpected_claim: ClaimId,
) {
    let result = retrieve(repository, &RetrievalQuery::lexical(query)).unwrap();
    assert!(result.candidates().iter().all(|candidate| {
        !candidate.channels().contains_long_term_memory()
            || !matches!(
                candidate.authority(),
                AuthoritativeCandidate::Ledger(claim) if claim.id() == unexpected_claim
            )
    }));
}

fn assert_time_gated_long_term_claim(repository: &mut VaultRepository, claim_id: ClaimId) {
    let before_applicable = retrieve(
        repository,
        &RetrievalQuery::lexical("continuity hypothesis").with_time(TimeRange::at(19)),
    )
    .unwrap();
    assert!(
        before_applicable
            .candidates()
            .iter()
            .all(|candidate| !candidate.channels().contains_long_term_memory())
    );
    let while_applicable = retrieve(
        repository,
        &RetrievalQuery::lexical("continuity hypothesis").with_time(TimeRange::at(25)),
    )
    .unwrap();
    assert!(while_applicable.candidates().iter().any(|candidate| {
        candidate.channels().contains_long_term_memory()
            && matches!(
                candidate.authority(),
                AuthoritativeCandidate::Ledger(claim) if claim.id() == claim_id
            )
    }));
}
