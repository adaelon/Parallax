use eam_core::{
    ApplicableTime, ClaimOwner, CoreError, CounterpartInconsistencyReason, CounterpartReadiness,
    EvidenceCitation, FrozenEvidenceBlock, FrozenRetrievalWindow, InMemoryRepository,
    IncrementingClock, JudgmentProposal, JudgmentRejectionReason, MemoryCore, MemoryRepository,
    PersonTurnClassification, RetrievalSnapshot, RetrievedContextItem, RuntimeResponse,
    ScriptedRuntime, SessionId, SourceCurrentness, Speaker, Timestamp, Uncertainty,
    WorkingContextError,
};

mod support;
use support::ready_repository;

fn session(id: &str) -> SessionId {
    SessionId::new(id)
}

#[test]
fn every_non_ready_state_fails_before_any_formal_conversation_side_effect() {
    let cases = [
        CounterpartReadiness::NeedsIntroduction,
        CounterpartReadiness::IntroductionRecorded,
        CounterpartReadiness::Inconsistent {
            reason: CounterpartInconsistencyReason::IntroductionMissing {
                identity_version: Some(1),
                self_bundle_version: Some(1),
            },
        },
        CounterpartReadiness::Inconsistent {
            reason: CounterpartInconsistencyReason::IdentityMissing {
                self_bundle_version: 1,
                referenced_identity_version: 1,
            },
        },
        CounterpartReadiness::Inconsistent {
            reason: CounterpartInconsistencyReason::SelfBundleMissing {
                identity_version: 1,
            },
        },
        CounterpartReadiness::Inconsistent {
            reason: CounterpartInconsistencyReason::IdentityVersionMismatch {
                identity_version: 1,
                self_bundle_version: 1,
                referenced_identity_version: 2,
            },
        },
    ];

    for readiness in cases {
        let runtime = ScriptedRuntime::new(
            [PersonTurnClassification::Question],
            [RuntimeResponse::new("这条回复不应被调用。")],
        );
        let repository = InMemoryRepository::new().with_counterpart_readiness(readiness.clone());
        let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(50));
        let context = core.freeze_working_context(&[]).unwrap();

        let error = core
            .run_counterpart_turn(session("blocked"), "这条本人消息不应落盘。", context)
            .expect_err("formal conversation must fail closed before counterpart creation");

        assert_eq!(error, CoreError::CounterpartNotReady(readiness));
        assert!(core.repository().all_evidence().unwrap().is_empty());
        assert!(core.repository().all_claims().unwrap().is_empty());
        assert!(core.runtime().seen_classification_inputs().is_empty());
        assert!(core.runtime().seen_requests().is_empty());
    }
}

#[test]
fn ready_versions_are_revalidated_before_any_formal_conversation_side_effect() {
    let repository = ready_repository().with_counterpart_readiness(CounterpartReadiness::Ready {
        identity_version: 2,
        self_bundle_version: 2,
    });
    let runtime = ScriptedRuntime::new(
        [PersonTurnClassification::Question],
        [RuntimeResponse::new("这条回复不应被调用。")],
    );
    let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(60));
    let context = core.freeze_working_context(&[]).unwrap();

    let error = core
        .run_counterpart_turn(session("stale-ready"), "这条本人消息不应落盘。", context)
        .expect_err("formal conversation must reject stale ready versions");

    assert_eq!(error, CoreError::CounterpartStateChanged);
    assert!(core.repository().all_evidence().unwrap().is_empty());
    assert!(core.repository().all_claims().unwrap().is_empty());
    assert!(core.runtime().seen_classification_inputs().is_empty());
    assert!(core.runtime().seen_requests().is_empty());
}

#[test]
fn retrieved_context_rejects_budget_overflow_and_preserves_source_boundaries() {
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new([], []),
        IncrementingClock::new(100),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let item = RetrievedContextItem::EvidenceWindow(FrozenRetrievalWindow::new(
        0,
        vec![FrozenEvidenceBlock::new(
            7,
            11,
            0,
            "逐字证据".to_owned(),
            5,
            "notes/source.md".to_owned(),
            SourceCurrentness::Present,
            Timestamp::from_millis(90),
        )],
        12,
    ));
    let snapshot = RetrievalSnapshot::new("eam-retrieval-v2", "model-v1", 11, 12, [3; 32]);

    assert_eq!(
        context.with_retrieval(vec![item], snapshot),
        Err(WorkingContextError::BudgetExceeded)
    );
}

#[test]
fn closes_the_minimal_memory_loop_with_exact_sources_and_separate_ledgers() {
    let first_statement = "我从 2024 年开始住在香港。";
    let response_citation = EvidenceCitation::new(
        // InMemoryRepository allocates deterministically from one.
        eam_core::EvidenceId::from_raw(1),
        "2024 年开始住在香港",
    );
    let response = RuntimeResponse::new("你说过自己从 2024 年开始住在香港。")
        .with_citation(response_citation.clone())
        .with_judgment(JudgmentProposal::new(
            "我认为香港是本人当前生活背景的一部分。",
            vec![response_citation.clone()],
            Uncertainty::Low,
            ApplicableTime::Since(Timestamp::from_millis(1_000)),
        ));
    let runtime = ScriptedRuntime::new(
        [
            PersonTurnClassification::DirectSelfReport,
            PersonTurnClassification::Question,
        ],
        [response],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));

    let (source_id, classification) = core
        .record_person_turn(session("first"), first_statement)
        .expect("the first turn should be accepted");
    assert_eq!(classification, PersonTurnClassification::DirectSelfReport);
    assert_eq!(source_id.get(), 1);

    let frozen = core
        .freeze_working_context(&[source_id])
        .expect("the source should freeze into the next session");
    let outcome = core
        .run_counterpart_turn(session("later"), "你记得我住在哪里吗？", frozen)
        .expect("the later turn should complete");

    assert_eq!(outcome.accepted_judgment_ids().len(), 1);
    assert!(outcome.rejected_judgments().is_empty());
    assert_eq!(
        core.resolve_citation(&response_citation)
            .expect("the citation must match verbatim evidence"),
        "2024 年开始住在香港"
    );

    let evidence = core.repository().all_evidence().expect("read evidence");
    assert_eq!(evidence.len(), 3);
    assert_eq!(evidence[0].speaker(), Speaker::Person);
    assert_eq!(evidence[0].verbatim(), first_statement);
    assert_eq!(evidence[1].speaker(), Speaker::Person);
    assert_eq!(evidence[2].speaker(), Speaker::Counterpart);

    let claims = core.repository().all_claims().expect("read claims");
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].owner(), ClaimOwner::Person);
    assert_eq!(claims[0].statement(), first_statement);
    assert_eq!(claims[0].support()[0].evidence_id(), source_id);
    assert_eq!(claims[1].owner(), ClaimOwner::Counterpart);
    assert_eq!(claims[1].support()[0], response_citation);

    // The runtime sees a frozen value containing only the evidence selected by
    // Core. Its port has no repository handle or repository operation.
    let request = &core.runtime().seen_requests()[0];
    assert_eq!(request.working_context().evidence().len(), 1);
    assert_eq!(request.working_context().evidence()[0].id(), source_id);
    assert_eq!(request.prompt().id(), outcome.person_evidence_id());
}

#[test]
fn questions_and_jokes_remain_verbatim_evidence_without_person_facts() {
    let runtime = ScriptedRuntime::new(
        [
            PersonTurnClassification::Question,
            PersonTurnClassification::Joke,
        ],
        [],
    );
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(2_000),
    );

    core.record_person_turn(session("chat"), "我是不是该搬去月球？")
        .expect("question should be retained");
    core.record_person_turn(session("chat"), "我是火星人——开玩笑的。")
        .expect("joke should be retained");

    assert_eq!(core.repository().all_evidence().unwrap().len(), 2);
    assert!(core.repository().all_claims().unwrap().is_empty());
}

#[test]
fn rejects_unsourced_and_out_of_context_judgments() {
    let hidden_source = eam_core::EvidenceId::from_raw(1);
    let unsourced = JudgmentProposal::new(
        "这条判断没有来源。",
        vec![],
        Uncertainty::High,
        ApplicableTime::Unknown,
    );
    let guessed_repository_reference = JudgmentProposal::new(
        "运行时猜中了一个仓储 ID，但它不在工作上下文。",
        vec![EvidenceCitation::new(
            hidden_source,
            "一条不会暴露给运行时的事实",
        )],
        Uncertainty::High,
        ApplicableTime::Unknown,
    );
    let response = RuntimeResponse::new("我只能把这些内容留在普通对话证据中。")
        .with_judgment(unsourced)
        .with_judgment(guessed_repository_reference);
    let runtime = ScriptedRuntime::new(
        [
            PersonTurnClassification::DirectSelfReport,
            PersonTurnClassification::Question,
        ],
        [response],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(3_000));

    core.record_person_turn(session("first"), "一条不会暴露给运行时的事实")
        .unwrap();
    let empty_context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(session("later"), "你能判断什么？", empty_context)
        .unwrap();

    assert!(outcome.accepted_judgment_ids().is_empty());
    assert_eq!(outcome.rejected_judgments().len(), 2);
    assert_eq!(
        outcome.rejected_judgments()[0].reason(),
        &JudgmentRejectionReason::MissingSupport
    );
    assert_eq!(
        outcome.rejected_judgments()[1].reason(),
        &JudgmentRejectionReason::EvidenceOutsideWorkingContext(hidden_source)
    );
    assert_eq!(
        core.repository()
            .all_claims()
            .unwrap()
            .iter()
            .filter(|claim| claim.owner() == ClaimOwner::Counterpart)
            .count(),
        0
    );
}

#[test]
fn free_text_response_is_evidence_but_cannot_write_a_ledger() {
    let runtime = ScriptedRuntime::new(
        [PersonTurnClassification::Question],
        [RuntimeResponse::new("这是普通回答，不是持久判断提议。")],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(4_000));

    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(session("chat"), "给我一个普通回答。", context)
        .unwrap();

    assert!(outcome.accepted_judgment_ids().is_empty());
    assert!(core.repository().all_claims().unwrap().is_empty());
    let evidence = core.repository().all_evidence().unwrap();
    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[1].speaker(), Speaker::Counterpart);
    assert_eq!(evidence[1].verbatim(), "这是普通回答，不是持久判断提议。");
}

#[test]
fn rejects_a_response_citation_that_is_not_a_verbatim_match() {
    let source_id = eam_core::EvidenceId::from_raw(1);
    let runtime = ScriptedRuntime::new(
        [
            PersonTurnClassification::DirectSelfReport,
            PersonTurnClassification::Question,
        ],
        [RuntimeResponse::new("错误地改写了来源。")
            .with_citation(EvidenceCitation::new(source_id, "并不存在的逐字内容"))],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(5_000));

    core.record_person_turn(session("first"), "原始逐字内容")
        .unwrap();
    let context = core.freeze_working_context(&[source_id]).unwrap();
    let error = core
        .run_counterpart_turn(session("later"), "请引用我。", context)
        .expect_err("a rewritten quote must be rejected");

    assert_eq!(
        error,
        CoreError::InvalidResponseCitation(JudgmentRejectionReason::QuoteMismatch(source_id))
    );
    assert_eq!(core.repository().all_evidence().unwrap().len(), 2);
    assert_eq!(core.repository().all_claims().unwrap().len(), 1);
}
