use eam_core::{
    ApplicableTime, Claim, ClaimCorrectionRepository, ClaimOwner, ClaimStatus, CoreError,
    EvidenceCitation, InMemoryRepository, IncrementingClock, MemoryCore, MemoryRepository,
    ScriptedPersonFactResponse, ScriptedRuntime, SessionId, Timestamp,
};

fn core_with_person_fact(
    statement: &str,
) -> MemoryCore<InMemoryRepository, ScriptedRuntime, IncrementingClock> {
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new([ScriptedPersonFactResponse::VerbatimFactAtRecordedTime], []),
        IncrementingClock::new(1_000),
    );
    core.record_person_turn(SessionId::new("facts"), statement)
        .expect("seed person fact");
    core
}

#[test]
fn person_correction_appends_a_temporal_successor_and_preserves_history() {
    let mut core = core_with_person_fact("我住在深圳。");
    let original = core.repository().all_claims().unwrap()[0].clone();

    let receipt = core
        .correct_person_fact(
            SessionId::new("facts"),
            original.id(),
            "我从 2026 年起住在香港。",
            ApplicableTime::Since(Timestamp::from_millis(2_026)),
        )
        .expect("correction should commit atomically");

    let claims = core.repository().all_claims().unwrap();
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].status(), ClaimStatus::Superseded);
    assert_eq!(claims[0].supersedes(), None);
    assert_eq!(claims[0].superseded_by(), Some(claims[1].id()));
    assert_eq!(claims[1].status(), ClaimStatus::Current);
    assert_eq!(claims[1].supersedes(), Some(original.id()));
    assert_eq!(claims[1].superseded_by(), None);
    assert_eq!(claims[1].owner(), ClaimOwner::Person);
    assert_eq!(claims[1].statement(), "我从 2026 年起住在香港。");
    assert_eq!(
        claims[1].applicable_time(),
        ApplicableTime::Since(Timestamp::from_millis(2_026))
    );
    assert_eq!(receipt.superseded_claim_id(), original.id());
    assert_eq!(receipt.replacement_claim_id(), claims[1].id());
    assert_eq!(
        receipt.correction_evidence_id(),
        claims[1].support()[0].evidence_id()
    );
    assert_eq!(core.repository().all_evidence().unwrap().len(), 2);
}

#[test]
fn correction_rejects_non_person_and_already_superseded_claims_without_partial_evidence() {
    let mut core = core_with_person_fact("我住在深圳。");
    let original = core.repository().all_claims().unwrap()[0].clone();
    core.correct_person_fact(
        SessionId::new("facts"),
        original.id(),
        "我住在香港。",
        ApplicableTime::At(Timestamp::from_millis(1_001)),
    )
    .expect("first correction");
    let evidence_count = core.repository().all_evidence().unwrap().len();

    assert_eq!(
        core.correct_person_fact(
            SessionId::new("facts"),
            original.id(),
            "我住在澳门。",
            ApplicableTime::At(Timestamp::from_millis(1_002)),
        ),
        Err(CoreError::ClaimNotCurrent(original.id()))
    );
    assert_eq!(
        core.repository().all_evidence().unwrap().len(),
        evidence_count
    );
    assert_eq!(core.repository().all_claims().unwrap().len(), 2);
}

#[test]
fn correction_rejects_invalid_time_before_allocating_persistent_state() {
    let mut core = core_with_person_fact("我住在深圳。");
    let original = core.repository().all_claims().unwrap()[0].clone();
    let reversed = ApplicableTime::Between {
        start: Timestamp::from_millis(20),
        end: Timestamp::from_millis(10),
    };

    assert_eq!(
        core.correct_person_fact(
            SessionId::new("facts"),
            original.id(),
            "我曾住在香港。",
            reversed,
        ),
        Err(CoreError::InvalidCorrectionTime)
    );
    assert_eq!(core.repository().all_evidence().unwrap().len(), 1);
    assert_eq!(core.repository().all_claims().unwrap().len(), 1);
}

#[test]
fn correction_rejects_an_unchanged_statement_before_allocating_persistent_state() {
    let mut core = core_with_person_fact("我住在深圳。");
    let original = core.repository().all_claims().unwrap()[0].clone();

    assert_eq!(
        core.correct_person_fact(
            SessionId::new("facts"),
            original.id(),
            "  我住在深圳。  ",
            ApplicableTime::At(Timestamp::from_millis(1_001)),
        ),
        Err(CoreError::UnchangedCorrection)
    );
    assert_eq!(core.repository().all_evidence().unwrap().len(), 1);
    assert_eq!(core.repository().all_claims().unwrap().len(), 1);
}

#[test]
fn correction_rejects_a_counterpart_claim_without_partial_evidence() {
    let mut core = core_with_person_fact("我住在深圳。");
    let source = core.repository().all_evidence().unwrap()[0].clone();
    let counterpart_claim_id = core.repository_mut().next_claim_id();
    core.repository_mut()
        .append_claim(Claim::restore(
            counterpart_claim_id,
            ClaimOwner::Counterpart,
            "我认为深圳仍影响着本人。".to_owned(),
            vec![EvidenceCitation::new(source.id(), source.verbatim())],
            None,
            ApplicableTime::Unknown,
            Timestamp::from_millis(1_001),
        ))
        .unwrap();
    let evidence_count = core.repository().all_evidence().unwrap().len();

    assert_eq!(
        core.correct_person_fact(
            SessionId::new("facts"),
            counterpart_claim_id,
            "我不再这样判断。",
            ApplicableTime::At(Timestamp::from_millis(1_002)),
        ),
        Err(CoreError::ClaimNotPerson(counterpart_claim_id))
    );
    assert_eq!(
        core.repository().all_evidence().unwrap().len(),
        evidence_count
    );
}

#[test]
fn correction_repository_exposes_the_current_claim_without_erasing_history() {
    let mut core = core_with_person_fact("我住在深圳。");
    let original = core.repository().all_claims().unwrap()[0].clone();
    core.correct_person_fact(
        SessionId::new("facts"),
        original.id(),
        "我住在香港。",
        ApplicableTime::At(Timestamp::from_millis(1_001)),
    )
    .unwrap();

    assert_eq!(
        core.repository()
            .claim(original.id())
            .unwrap()
            .unwrap()
            .status(),
        ClaimStatus::Superseded
    );
}
