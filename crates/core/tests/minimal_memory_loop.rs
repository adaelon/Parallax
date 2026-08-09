use eam_core::{
    ApplicableTime, ClaimOwner, CoreError, CounterpartInconsistencyReason, CounterpartReadiness,
    EvidenceCitation, FrozenEvidenceBlock, FrozenRetrievalWindow, InMemoryRepository,
    IncrementingClock, JudgmentProposal, JudgmentRejectionReason,
    MAX_PERSON_FACT_PROPOSALS_PER_TURN, MemoryCore, MemoryRepository, PersonFactProposal,
    PersonFactProposalBatch, PersonFactProposalRejectionReason, ReflectionImportance,
    ReflectionInvitationBasis, ReflectionInvitationProposal, ReflectionInvitationRejectionReason,
    RetrievalSnapshot, RetrievedContextItem, RuntimeResponse, ScriptedPersonFactResponse,
    ScriptedRuntime, SessionId, SourceCurrentness, Speaker, StructuredOperationRejectionReason,
    Timestamp, Uncertainty, WorkingContextError,
};

mod support;
use support::ready_repository;

fn session(id: &str) -> SessionId {
    SessionId::new(id)
}

fn no_person_facts() -> ScriptedPersonFactResponse {
    ScriptedPersonFactResponse::NoFacts
}

fn one_person_fact(evidence_id: u64, statement: &str) -> ScriptedPersonFactResponse {
    ScriptedPersonFactResponse::Exact(
        PersonFactProposalBatch::try_new([PersonFactProposal::new(
            ClaimOwner::Person,
            statement,
            EvidenceCitation::new(eam_core::EvidenceId::from_raw(evidence_id), statement),
            ApplicableTime::Unknown,
        )])
        .unwrap(),
    )
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
            [no_person_facts()],
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
        assert!(core.runtime().seen_person_fact_inputs().is_empty());
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
        [no_person_facts()],
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
    assert!(core.runtime().seen_person_fact_inputs().is_empty());
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
        [one_person_fact(1, first_statement), no_person_facts()],
        [response],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));

    let observation = core
        .record_person_turn(session("first"), first_statement)
        .expect("the first turn should be accepted");
    let source_id = observation.evidence_id();
    assert_eq!(observation.accepted_person_fact_ids().len(), 1);
    assert!(observation.rejected_person_fact_proposals().is_empty());
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
    let runtime = ScriptedRuntime::new([no_person_facts(), no_person_facts()], []);
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
            one_person_fact(1, "一条不会暴露给运行时的事实"),
            no_person_facts(),
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
        [no_person_facts()],
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
fn persistent_interpretation_uses_the_existing_judgment_path_only() {
    let source_id = eam_core::EvidenceId::from_raw(1);
    let source_quote = "这次改动还没有跑测试";
    let response = RuntimeResponse::new(
        "这次没跑测试不等于你是粗心型人格；我暂时只判断这次合并信心缺少验证依据。",
    )
    .with_judgment(JudgmentProposal::new(
        "我暂时认为这次合并信心缺少验证依据。",
        vec![EvidenceCitation::new(source_id, source_quote)],
        Uncertainty::High,
        ApplicableTime::Unknown,
    ))
    .with_unsupported_operation(1, "propose_personality_label");
    let runtime = ScriptedRuntime::new(
        [one_person_fact(1, source_quote), no_person_facts()],
        [response],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(4_500));

    core.record_person_turn(session("source"), source_quote)
        .unwrap();
    let context = core.freeze_working_context(&[source_id]).unwrap();
    let outcome = core
        .run_counterpart_turn(
            session("interpretation"),
            "你觉得我是不是一直都很粗心？",
            context,
        )
        .unwrap();

    assert_eq!(outcome.accepted_judgment_ids().len(), 1);
    assert_eq!(outcome.rejected_operations().len(), 1);
    assert_eq!(
        outcome.rejected_operations()[0].reason(),
        &StructuredOperationRejectionReason::NotWhitelisted("propose_personality_label".to_owned())
    );
    let counterpart_claims = core
        .repository()
        .all_claims()
        .unwrap()
        .into_iter()
        .filter(|claim| claim.owner() == ClaimOwner::Counterpart)
        .collect::<Vec<_>>();
    assert_eq!(counterpart_claims.len(), 1);
    assert_eq!(
        counterpart_claims[0].statement(),
        "我暂时认为这次合并信心缺少验证依据。"
    );
    assert_eq!(counterpart_claims[0].uncertainty(), Some(Uncertainty::High));
}

#[test]
fn one_performance_cannot_create_a_pattern_or_personality_label() {
    let response = RuntimeResponse::new("我只把这次表现当作一次可核对的事件。")
        .with_reflection_invitation(ReflectionInvitationProposal::new(
            "technical-confidence",
            "你总是在没有验证时过度自信。",
            vec![EvidenceCitation::new(
                eam_core::EvidenceId::from_raw(1),
                "这一次我没跑测试",
            )],
            "现在可以定义你的固定模式。",
            ReflectionImportance::Important,
            ReflectionInvitationBasis::ImportantSingleChange,
        ))
        .with_unsupported_operation(1, "propose_personality_label");
    let runtime = ScriptedRuntime::new([no_person_facts()], [response]);
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(4_600));
    let context = core.freeze_working_context(&[]).unwrap();

    let outcome = core
        .run_counterpart_turn(session("single-performance"), "这一次我没跑测试。", context)
        .unwrap();

    assert!(outcome.accepted_reflection_invitations().is_empty());
    assert_eq!(outcome.rejected_reflection_invitations().len(), 1);
    assert_eq!(
        outcome.rejected_reflection_invitations()[0].reason(),
        &ReflectionInvitationRejectionReason::PatternLanguageForSingleChange
    );
    assert_eq!(outcome.rejected_operations().len(), 1);
    assert_eq!(
        outcome.rejected_operations()[0].reason(),
        &StructuredOperationRejectionReason::NotWhitelisted("propose_personality_label".to_owned())
    );
    assert!(core.repository().all_claims().unwrap().is_empty());
}

#[test]
fn rejects_a_response_citation_that_is_not_a_verbatim_match() {
    let source_id = eam_core::EvidenceId::from_raw(1);
    let runtime = ScriptedRuntime::new(
        [one_person_fact(1, "原始逐字内容"), no_person_facts()],
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

#[test]
fn greeting_produces_zero_person_facts_and_one_verbatim_evidence() {
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], []),
        IncrementingClock::new(6_000),
    );

    let observation = core
        .record_person_turn(session("greeting"), "你好")
        .expect("a greeting remains valid conversation evidence");

    assert!(observation.accepted_person_fact_ids().is_empty());
    assert!(observation.rejected_person_fact_proposals().is_empty());
    assert_eq!(core.repository().all_evidence().unwrap().len(), 1);
    assert!(core.repository().all_claims().unwrap().is_empty());
}

#[test]
fn one_turn_can_admit_multiple_atomic_person_facts_with_exact_time_and_source() {
    let evidence_id = eam_core::EvidenceId::from_raw(1);
    let proposals = PersonFactProposalBatch::try_new([
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我叫小林",
            EvidenceCitation::new(evidence_id, "我叫小林"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我从 2024 年开始住在香港",
            EvidenceCitation::new(evidence_id, "我从 2024 年开始住在香港"),
            ApplicableTime::Since(Timestamp::from_millis(1_704_067_200_000)),
        ),
    ])
    .unwrap();
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new([ScriptedPersonFactResponse::Exact(proposals)], []),
        IncrementingClock::new(7_000),
    );

    let observation = core
        .record_person_turn(
            session("multiple-facts"),
            "我叫小林，而且我从 2024 年开始住在香港。",
        )
        .unwrap();

    assert_eq!(observation.accepted_person_fact_ids().len(), 2);
    assert!(observation.rejected_person_fact_proposals().is_empty());
    assert_eq!(core.repository().all_evidence().unwrap().len(), 1);
    let claims = core.repository().all_claims().unwrap();
    assert_eq!(claims.len(), 2);
    assert_eq!(claims[0].statement(), "我叫小林");
    assert_eq!(claims[0].applicable_time(), ApplicableTime::Unknown);
    assert_eq!(claims[0].support()[0].evidence_id(), evidence_id);
    assert_eq!(claims[0].support()[0].quote(), "我叫小林");
    assert_eq!(claims[1].statement(), "我从 2024 年开始住在香港");
    assert_eq!(
        claims[1].applicable_time(),
        ApplicableTime::Since(Timestamp::from_millis(1_704_067_200_000))
    );
    assert_eq!(claims[1].support()[0].evidence_id(), evidence_id);
}

#[test]
fn mixed_invalid_person_fact_proposals_are_rejected_independently() {
    let evidence_id = eam_core::EvidenceId::from_raw(1);
    let proposals = PersonFactProposalBatch::try_new([
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我在香港工作",
            EvidenceCitation::new(evidence_id, "我在香港工作"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Counterpart,
            "我在香港工作",
            EvidenceCitation::new(evidence_id, "我在香港工作"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Person,
            " ",
            EvidenceCitation::new(evidence_id, "我在香港工作"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我是木星人",
            EvidenceCitation::new(evidence_id, "我是木星人"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我住在香港",
            EvidenceCitation::new(evidence_id, "我在香港工作"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我在香港工作",
            EvidenceCitation::new(evidence_id, "我在香港工作"),
            ApplicableTime::Between {
                start: Timestamp::from_millis(2),
                end: Timestamp::from_millis(1),
            },
        ),
    ])
    .unwrap();
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new([ScriptedPersonFactResponse::Exact(proposals)], []),
        IncrementingClock::new(8_000),
    );

    let observation = core
        .record_person_turn(session("mixed"), "我在香港工作。也许我是火星人——开玩笑的。")
        .unwrap();

    assert_eq!(observation.accepted_person_fact_ids().len(), 1);
    assert_eq!(
        observation
            .rejected_person_fact_proposals()
            .iter()
            .map(eam_core::PersonFactProposalRejection::reason)
            .collect::<Vec<_>>(),
        vec![
            &PersonFactProposalRejectionReason::OwnerNotPerson(ClaimOwner::Counterpart),
            &PersonFactProposalRejectionReason::EmptyStatement,
            &PersonFactProposalRejectionReason::QuoteMismatch(evidence_id),
            &PersonFactProposalRejectionReason::StatementNotVerbatim,
            &PersonFactProposalRejectionReason::InvalidApplicableTime,
        ]
    );
    let claims = core.repository().all_claims().unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].statement(), "我在香港工作");
}

#[test]
fn duplicate_person_facts_do_not_create_duplicate_claims() {
    let first_id = eam_core::EvidenceId::from_raw(1);
    let second_id = eam_core::EvidenceId::from_raw(2);
    let first = PersonFactProposalBatch::try_new([
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我住在香港",
            EvidenceCitation::new(first_id, "我住在香港"),
            ApplicableTime::Unknown,
        ),
        PersonFactProposal::new(
            ClaimOwner::Person,
            "我住在香港",
            EvidenceCitation::new(first_id, "我住在香港"),
            ApplicableTime::Unknown,
        ),
    ])
    .unwrap();
    let repeated = PersonFactProposalBatch::try_new([PersonFactProposal::new(
        ClaimOwner::Person,
        "我住在香港",
        EvidenceCitation::new(second_id, "我住在香港"),
        ApplicableTime::Unknown,
    )])
    .unwrap();
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new(
            [
                ScriptedPersonFactResponse::Exact(first),
                ScriptedPersonFactResponse::Exact(repeated),
            ],
            [],
        ),
        IncrementingClock::new(9_000),
    );

    let first_observation = core
        .record_person_turn(session("duplicates"), "我住在香港")
        .unwrap();
    let repeated_observation = core
        .record_person_turn(session("duplicates"), "我住在香港")
        .unwrap();

    assert_eq!(first_observation.accepted_person_fact_ids().len(), 1);
    assert_eq!(first_observation.rejected_person_fact_proposals().len(), 1);
    assert_eq!(
        first_observation.rejected_person_fact_proposals()[0].reason(),
        &PersonFactProposalRejectionReason::DuplicateFact
    );
    assert!(repeated_observation.accepted_person_fact_ids().is_empty());
    assert_eq!(
        repeated_observation.rejected_person_fact_proposals()[0].reason(),
        &PersonFactProposalRejectionReason::DuplicateFact
    );
    assert_eq!(core.repository().all_evidence().unwrap().len(), 2);
    assert_eq!(core.repository().all_claims().unwrap().len(), 1);
}

#[test]
fn person_fact_proposal_batch_is_bounded() {
    let proposals = (0..=MAX_PERSON_FACT_PROPOSALS_PER_TURN).map(|index| {
        PersonFactProposal::new(
            ClaimOwner::Person,
            format!("事实 {index}"),
            EvidenceCitation::new(eam_core::EvidenceId::from_raw(1), format!("事实 {index}")),
            ApplicableTime::Unknown,
        )
    });

    let error = PersonFactProposalBatch::try_new(proposals)
        .expect_err("more than the contract maximum must fail closed");
    assert_eq!(error.actual(), MAX_PERSON_FACT_PROPOSALS_PER_TURN + 1);
    assert_eq!(error.maximum(), MAX_PERSON_FACT_PROPOSALS_PER_TURN);
}
