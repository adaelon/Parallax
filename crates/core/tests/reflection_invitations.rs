use eam_core::{
    EvidenceCitation, EvidenceId, ForgetRepository, ForgetTarget, G08_IMMEDIATE_SAFETY_QUOTE,
    InMemoryRepository, IncrementingClock, MemoryCore, PersonTurnClassification,
    REFLECTION_DEFER_MILLIS, ReflectionDecision, ReflectionDelivery, ReflectionImportance,
    ReflectionInvitation, ReflectionInvitationBasis, ReflectionInvitationId,
    ReflectionInvitationProposal, ReflectionInvitationRejectionReason,
    ReflectionInvitationRepository, ReflectionInvitationState, ReflectionOpportunity,
    ReflectionRuntimeDisposition, RuntimeResponse, ScriptedRuntime, SessionId, Timestamp,
    decide_reflection_invitation, offer_reflection_invitation, reflection_delivery,
};

fn invitation(
    state: ReflectionInvitationState,
    importance: ReflectionImportance,
    next_eligible_at: Option<i64>,
    topic: &str,
) -> ReflectionInvitation {
    ReflectionInvitation::restore(
        ReflectionInvitationId::from_raw(1),
        topic,
        "最近一次直接变化值得一起看看。",
        vec![EvidenceCitation::new(EvidenceId::from_raw(1), "最近一次")],
        "此刻话题相关。",
        importance,
        ReflectionInvitationBasis::ImportantSingleChange,
        state,
        Timestamp::from_millis(1),
        Timestamp::from_millis(1),
        next_eligible_at.map(Timestamp::from_millis),
        None,
        0,
        false,
    )
}

fn parse_state(value: &str) -> ReflectionInvitationState {
    match value {
        "pending" => ReflectionInvitationState::Pending,
        "offered" => ReflectionInvitationState::Offered,
        "deferred" => ReflectionInvitationState::Deferred,
        "muted_by_person" => ReflectionInvitationState::MutedByPerson,
        "resolved" => ReflectionInvitationState::Resolved,
        _ => panic!("unknown state {value}"),
    }
}

fn parse_importance(value: &str) -> ReflectionImportance {
    match value {
        "ordinary" => ReflectionImportance::Ordinary,
        "important" => ReflectionImportance::Important,
        "immediate_safety_risk" => ReflectionImportance::ImmediateSafetyRisk,
        _ => panic!("unknown importance {value}"),
    }
}

fn parse_optional_millis(value: &str) -> Option<Timestamp> {
    (value != "-").then(|| Timestamp::from_millis(value.parse().unwrap()))
}

#[test]
fn g08_virtual_time_fixture_replays_natural_timing_and_silence_boundaries() {
    let fixture = include_str!("fixtures/g08-reflection-schedule.tsv");
    for row in fixture.lines().filter(|line| !line.starts_with('#')) {
        let columns = row.split('\t').collect::<Vec<_>>();
        let now = Timestamp::from_millis(columns[1].parse().unwrap());
        let opportunity = match columns[2] {
            "unrelated_task" => ReflectionOpportunity::UnrelatedTask,
            "conversation_idle" => ReflectionOpportunity::ConversationIdle,
            "scheduled_review" => ReflectionOpportunity::ScheduledReview,
            value if value.starts_with("related:") => {
                ReflectionOpportunity::RelatedTopic(value[8..].to_owned())
            }
            value => panic!("unknown opportunity {value}"),
        };
        let current = invitation(
            parse_state(columns[3]),
            parse_importance(columns[4]),
            parse_optional_millis(columns[5]).map(Timestamp::as_millis),
            columns[7],
        );
        let expected = match columns[8] {
            "queued" => ReflectionDelivery::Queued,
            "offered" => ReflectionDelivery::Offer,
            "discuss_only" => ReflectionDelivery::DiscussOnly,
            value => panic!("unknown delivery {value}"),
        };
        assert_eq!(
            reflection_delivery(
                &current,
                &opportunity,
                now,
                parse_optional_millis(columns[6]),
            ),
            expected,
            "fixture scenario {}",
            columns[0]
        );
    }
}

fn proposal(importance: ReflectionImportance, quote: &str) -> ReflectionInvitationProposal {
    ReflectionInvitationProposal::new(
        "work-rhythm",
        "这次工作节奏变化值得一起看看。",
        vec![EvidenceCitation::new(EvidenceId::from_raw(1), quote)],
        "刚出现了直接且重要的变化。",
        importance,
        ReflectionInvitationBasis::ImportantSingleChange,
    )
}

#[test]
fn ordinary_invitation_queues_on_unrelated_work_then_offers_on_related_topic() {
    let first = RuntimeResponse::new("我先回答当前问题。")
        .with_reflection_invitation(proposal(ReflectionImportance::Important, "工作节奏"));
    let second = RuntimeResponse::new("既然现在谈到节奏，我想邀请你一起看看。 ");
    let runtime = ScriptedRuntime::new([PersonTurnClassification::Question; 2], [first, second]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );

    let unrelated = core.freeze_working_context(&[]).unwrap();
    let first_outcome = core
        .run_counterpart_turn(
            SessionId::new("reflection"),
            "这次工作节奏变快了，但先回答部署问题。",
            unrelated,
        )
        .unwrap();
    let receipt = first_outcome.accepted_reflection_invitations()[0];
    assert_eq!(receipt.state(), ReflectionInvitationState::Pending);
    assert_eq!(first_outcome.offered_reflection_invitation_id(), None);

    let related = core
        .freeze_working_context(&[])
        .unwrap()
        .with_reflection_opportunity(ReflectionOpportunity::RelatedTopic(
            "work-rhythm".to_owned(),
        ));
    let second_outcome = core
        .run_counterpart_turn(
            SessionId::new("reflection"),
            "现在聊聊这个工作节奏。",
            related,
        )
        .unwrap();
    assert_eq!(
        core.runtime().seen_requests()[1]
            .reflection()
            .unwrap()
            .disposition(),
        ReflectionRuntimeDisposition::Offer
    );
    assert_eq!(
        second_outcome.offered_reflection_invitation_id(),
        Some(receipt.id())
    );
    assert_eq!(
        core.repository()
            .reflection_invitation(receipt.id())
            .unwrap()
            .unwrap()
            .state(),
        ReflectionInvitationState::Offered
    );
}

#[test]
fn repeated_deferral_prompts_for_mute_once_and_mute_preserves_discussion() {
    let pending = invitation(
        ReflectionInvitationState::Pending,
        ReflectionImportance::Important,
        None,
        "work-rhythm",
    );
    let first_offer = offer_reflection_invitation(&pending, Timestamp::from_millis(10)).unwrap();
    assert!(!first_offer.mute_prompted());
    let first_defer = decide_reflection_invitation(
        &first_offer,
        ReflectionDecision::Defer,
        Timestamp::from_millis(20),
    )
    .unwrap();
    assert_eq!(first_defer.defer_count(), 1);
    assert_eq!(
        first_defer.next_eligible_at().unwrap().as_millis(),
        20 + REFLECTION_DEFER_MILLIS
    );

    let second_offer = offer_reflection_invitation(
        &first_defer,
        Timestamp::from_millis(20 + REFLECTION_DEFER_MILLIS),
    )
    .unwrap();
    assert!(second_offer.mute_prompted());
    assert_eq!(second_offer.defer_count(), 1);
    let second_defer = decide_reflection_invitation(
        &second_offer,
        ReflectionDecision::Defer,
        Timestamp::from_millis(30 + REFLECTION_DEFER_MILLIS),
    )
    .unwrap();
    let third_offer = offer_reflection_invitation(
        &second_defer,
        Timestamp::from_millis(30 + 2 * REFLECTION_DEFER_MILLIS),
    )
    .unwrap();
    assert_eq!(third_offer.defer_count(), 2);
    assert!(third_offer.mute_prompted());

    let muted = decide_reflection_invitation(
        &third_offer,
        ReflectionDecision::Mute,
        Timestamp::from_millis(40 + 2 * REFLECTION_DEFER_MILLIS),
    )
    .unwrap();
    assert_eq!(muted.state(), ReflectionInvitationState::MutedByPerson);
    assert_eq!(muted.observation(), pending.observation());
    assert_eq!(muted.evidence_refs(), pending.evidence_refs());
    assert_eq!(
        reflection_delivery(
            &muted,
            &ReflectionOpportunity::ScheduledReview,
            Timestamp::from_millis(900_000_000),
            None,
        ),
        ReflectionDelivery::Queued
    );
    assert_eq!(
        reflection_delivery(
            &muted,
            &ReflectionOpportunity::RelatedTopic("work-rhythm".to_owned()),
            Timestamp::from_millis(900_000_000),
            None,
        ),
        ReflectionDelivery::DiscussOnly
    );
}

#[test]
fn only_the_fixed_immediate_safety_fixture_can_interrupt_or_override_mute() {
    let exact = ReflectionInvitationProposal::new(
        "safety",
        "这次直接安全变化需要现在关心。",
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(1),
            G08_IMMEDIATE_SAFETY_QUOTE,
        )],
        "存在固定夹具命中的即时风险。",
        ReflectionImportance::ImmediateSafetyRisk,
        ReflectionInvitationBasis::ImportantSingleChange,
    );
    let runtime = ScriptedRuntime::new(
        [PersonTurnClassification::DirectSelfReport],
        [RuntimeResponse::new("我需要先关心你的即时安全。").with_reflection_invitation(exact)],
    );
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(2_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(
            SessionId::new("safety"),
            G08_IMMEDIATE_SAFETY_QUOTE,
            context,
        )
        .unwrap();
    assert_eq!(
        outcome.accepted_reflection_invitations()[0].state(),
        ReflectionInvitationState::Offered
    );
    assert_eq!(
        outcome.offered_reflection_invitation_id(),
        Some(outcome.accepted_reflection_invitations()[0].id())
    );

    let invalid = proposal(ReflectionImportance::ImmediateSafetyRisk, "工作节奏");
    let mut invalid_core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new(
            [PersonTurnClassification::Question],
            [RuntimeResponse::new("普通变化不能冒充即时风险。")
                .with_reflection_invitation(invalid)],
        ),
        IncrementingClock::new(3_000),
    );
    let context = invalid_core.freeze_working_context(&[]).unwrap();
    let rejected = invalid_core
        .run_counterpart_turn(SessionId::new("not-safety"), "这次工作节奏变了。", context)
        .unwrap();
    assert_eq!(
        rejected.rejected_reflection_invitations()[0].reason(),
        &ReflectionInvitationRejectionReason::ImmediateSafetyFixtureMismatch
    );
    assert!(
        invalid_core
            .repository()
            .all_reflection_invitations()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn counterpart_quote_cannot_impersonate_person_immediate_safety_evidence() {
    let counterpart_quote = ReflectionInvitationProposal::new(
        "counterpart-quote-is-not-person-risk",
        "第二自我的历史原话不能充当本人的即时风险证据。",
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(2),
            G08_IMMEDIATE_SAFETY_QUOTE,
        )],
        "不能把第二自我的话归属给本人。",
        ReflectionImportance::ImmediateSafetyRisk,
        ReflectionInvitationBasis::ImportantSingleChange,
    );
    let mut speaker_core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new(
            [PersonTurnClassification::Question; 2],
            [
                RuntimeResponse::new(G08_IMMEDIATE_SAFETY_QUOTE),
                RuntimeResponse::new("这不是本人的直接风险陈述。")
                    .with_reflection_invitation(counterpart_quote),
            ],
        ),
        IncrementingClock::new(3_500),
    );
    let seed_context = speaker_core.freeze_working_context(&[]).unwrap();
    speaker_core
        .run_counterpart_turn(
            SessionId::new("counterpart-safety-quote"),
            "请原样复述测试句。",
            seed_context,
        )
        .unwrap();
    let quoted_context = speaker_core
        .freeze_working_context(&[EvidenceId::from_raw(2)])
        .unwrap();
    let rejected = speaker_core
        .run_counterpart_turn(
            SessionId::new("counterpart-safety-quote"),
            "这只是普通问题。",
            quoted_context,
        )
        .unwrap();
    assert_eq!(
        rejected.rejected_reflection_invitations()[0].reason(),
        &ReflectionInvitationRejectionReason::ImmediateSafetyFixtureMismatch
    );
    assert!(
        speaker_core
            .repository()
            .all_reflection_invitations()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn s26_rejects_pattern_basis_and_forget_removes_sourced_invitation() {
    let pattern = ReflectionInvitationProposal::new(
        "work-rhythm",
        "这可能是一项重复变化。",
        vec![EvidenceCitation::new(EvidenceId::from_raw(1), "工作节奏")],
        "现在值得检查。",
        ReflectionImportance::Important,
        ReflectionInvitationBasis::RepeatedPattern,
    );
    let accepted = proposal(ReflectionImportance::Important, "工作节奏");
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new(
            [PersonTurnClassification::Question; 2],
            [
                RuntimeResponse::new("模式门槛留给下一片。").with_reflection_invitation(pattern),
                RuntimeResponse::new("这次只记录直接变化。").with_reflection_invitation(accepted),
            ],
        ),
        IncrementingClock::new(4_000),
    );
    let first_context = core.freeze_working_context(&[]).unwrap();
    let rejected = core
        .run_counterpart_turn(
            SessionId::new("pattern"),
            "这次工作节奏变化了。",
            first_context,
        )
        .unwrap();
    assert_eq!(
        rejected.rejected_reflection_invitations()[0].reason(),
        &ReflectionInvitationRejectionReason::RepeatedPatternRequiresS27
    );
    let second_context = core
        .freeze_working_context(&[EvidenceId::from_raw(1)])
        .unwrap();
    let accepted = core
        .run_counterpart_turn(
            SessionId::new("pattern"),
            "工作节奏又有一次直接变化。",
            second_context,
        )
        .unwrap();
    assert_eq!(accepted.accepted_reflection_invitations().len(), 1);
    let receipt = core
        .repository_mut()
        .commit_forget(
            ForgetTarget::ConversationEvidence(EvidenceId::from_raw(1)),
            Timestamp::from_millis(5_000),
        )
        .unwrap()
        .unwrap();
    assert_eq!(receipt.removed_derived_records(), 1);
    assert!(
        core.repository()
            .all_reflection_invitations()
            .unwrap()
            .is_empty()
    );
}
