use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, EvidenceCitation, EvidenceId, ForgetRequest,
    ForgetTarget, InMemoryRepository, IncrementingClock, MemoryCore, MemoryRepository,
    PersonTurnClassification, RuntimeResponse, ScriptedRuntime, SessionId,
    SharedAgreementCandidateStatus, SharedAgreementDecision, SharedExperience,
    SharedExperienceKind, SharedExperienceProposal, SharedExperienceRejectionReason,
    SharedExperienceRepository, Timestamp,
};

fn session() -> SessionId {
    SessionId::new("shared-experience")
}

fn proposal(
    kind: SharedExperienceKind,
    statement: &str,
    person_quote: &str,
    counterpart_quote: &str,
) -> SharedExperienceProposal {
    SharedExperienceProposal::new(
        kind,
        statement,
        vec![EvidenceCitation::new(EvidenceId::from_raw(1), person_quote)],
        counterpart_quote,
        Timestamp::from_millis(1_000),
    )
}

#[test]
fn ordinary_question_and_person_external_experience_do_not_enter_shared_ledger() {
    let runtime = ScriptedRuntime::new(
        [
            PersonTurnClassification::Question,
            PersonTurnClassification::DirectSelfReport,
        ],
        [RuntimeResponse::new("这是普通回答。")],
    );
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    core.run_counterpart_turn(session(), "普通问题", context)
        .unwrap();
    core.record_person_turn(session(), "我昨天独自完成了马拉松。")
        .unwrap();

    assert!(
        core.repository()
            .all_shared_agreement_candidates()
            .unwrap()
            .is_empty()
    );
    assert!(
        core.repository()
            .all_shared_experiences()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        core.repository()
            .all_claims()
            .unwrap()
            .iter()
            .filter(|claim| claim.owner() == ClaimOwner::Shared)
            .count(),
        0
    );
}

#[test]
fn shared_agreement_waits_for_person_ceremony_before_ledger_entry() {
    let response =
        RuntimeResponse::new("我也同意以后直接指出关键逃避。").with_shared_experience(proposal(
            SharedExperienceKind::Agreement,
            "发现关键逃避时直接指出",
            "我同意以后直接指出关键逃避",
            "我也同意以后直接指出关键逃避",
        ));
    let runtime = ScriptedRuntime::new([PersonTurnClassification::Question], [response]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(session(), "我同意以后直接指出关键逃避。", context)
        .unwrap();

    assert_eq!(outcome.pending_agreement_candidate_ids().len(), 1);
    assert!(outcome.admitted_shared_experience_ids().is_empty());
    assert!(
        core.repository()
            .all_claims()
            .unwrap()
            .iter()
            .all(|claim| claim.owner() != ClaimOwner::Shared)
    );
    let candidate = &core.repository().all_shared_agreement_candidates().unwrap()[0];
    assert_eq!(
        candidate.status(),
        SharedAgreementCandidateStatus::AwaitingPerson
    );
    assert_eq!(candidate.support().len(), 2);
}

#[test]
fn person_confirmation_admits_agreement_while_deferral_keeps_it_out() {
    let confirmed = RuntimeResponse::new("我同意 A。").with_shared_experience(proposal(
        SharedExperienceKind::Agreement,
        "共同决定 A",
        "我同意 A",
        "我同意 A",
    ));
    let deferred =
        RuntimeResponse::new("我同意 B。").with_shared_experience(SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "共同决定 B",
            vec![EvidenceCitation::new(EvidenceId::from_raw(3), "我同意 B")],
            "我同意 B",
            Timestamp::from_millis(2_000),
        ));
    let runtime = ScriptedRuntime::new(
        [
            PersonTurnClassification::Question,
            PersonTurnClassification::Question,
        ],
        [confirmed, deferred],
    );
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );

    let first_context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(session(), "我同意 A。", first_context)
        .unwrap();
    let first_id = first.pending_agreement_candidate_ids()[0];
    let resolution = core
        .resolve_shared_agreement(first_id, SharedAgreementDecision::Confirm)
        .unwrap();
    assert_eq!(
        resolution.status(),
        SharedAgreementCandidateStatus::Confirmed
    );
    assert!(resolution.claim_id().is_some());

    let second_context = core.freeze_working_context(&[]).unwrap();
    let second = core
        .run_counterpart_turn(session(), "我同意 B。", second_context)
        .unwrap();
    let second_id = second.pending_agreement_candidate_ids()[0];
    let resolution = core
        .resolve_shared_agreement(second_id, SharedAgreementDecision::Defer)
        .unwrap();
    assert_eq!(
        resolution.status(),
        SharedAgreementCandidateStatus::Deferred
    );
    assert_eq!(resolution.claim_id(), None);

    let shared_claims = core
        .repository()
        .all_claims()
        .unwrap()
        .into_iter()
        .filter(|claim| claim.owner() == ClaimOwner::Shared)
        .collect::<Vec<_>>();
    assert_eq!(shared_claims.len(), 1);
    assert_eq!(shared_claims[0].statement(), "共同决定 A");
}

#[test]
fn substantive_disagreement_with_both_positions_enters_history_without_veto() {
    let response =
        RuntimeResponse::new("我不同意把这看成可以忽略的小事。").with_shared_experience(proposal(
            SharedExperienceKind::SubstantiveDisagreement,
            "双方对这件事的重要性持不相容立场",
            "这只是小事",
            "我不同意把这看成可以忽略的小事",
        ));
    let runtime = ScriptedRuntime::new([PersonTurnClassification::Question], [response]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(session(), "这只是小事。", context)
        .unwrap();

    assert_eq!(outcome.admitted_shared_experience_ids().len(), 1);
    assert!(outcome.pending_agreement_candidate_ids().is_empty());
    let experiences = core.repository().all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 1);
    assert_eq!(
        experiences[0].kind(),
        SharedExperienceKind::SubstantiveDisagreement
    );
    assert_eq!(experiences[0].claim().owner(), ClaimOwner::Shared);
    assert_eq!(experiences[0].claim().support().len(), 2);
}

#[test]
fn invalid_or_person_only_proposals_are_rejected_without_shared_history() {
    let response = RuntimeResponse::new("我不能把单方事件冒充共同历史。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::SharedAchievement,
            "本人独自完成的事情",
            vec![],
            "我不能把单方事件冒充共同历史",
            Timestamp::from_millis(1_000),
        ),
    );
    let runtime = ScriptedRuntime::new([PersonTurnClassification::Question], [response]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(session(), "我独自完成了这件事。", context)
        .unwrap();

    assert_eq!(outcome.rejected_shared_experiences().len(), 1);
    assert_eq!(
        outcome.rejected_shared_experiences()[0].reason(),
        &SharedExperienceRejectionReason::MissingPersonSupport
    );
    assert!(
        core.repository()
            .all_shared_experiences()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn repository_rejects_shared_history_supported_only_by_one_participant() {
    let runtime = ScriptedRuntime::new(
        [
            PersonTurnClassification::Question,
            PersonTurnClassification::Question,
        ],
        [],
    );
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );
    core.record_person_turn(session(), "第一段本人证据")
        .unwrap();
    core.record_person_turn(session(), "第二段本人证据")
        .unwrap();
    let claim = Claim::restore(
        ClaimId::from_raw(99),
        ClaimOwner::Shared,
        "不能由本人两段发言构成共同历史".to_owned(),
        vec![
            EvidenceCitation::new(EvidenceId::from_raw(1), "第一段本人证据"),
            EvidenceCitation::new(EvidenceId::from_raw(2), "第二段本人证据"),
        ],
        None,
        ApplicableTime::At(Timestamp::from_millis(1_000)),
        Timestamp::from_millis(1_000),
    );

    let result = core
        .repository_mut()
        .commit_shared_experience(SharedExperience::restore(
            SharedExperienceKind::SharedAchievement,
            claim,
            false,
        ));

    assert!(result.is_err());
    assert!(core.repository().all_claims().unwrap().is_empty());
    assert!(
        core.repository()
            .all_shared_experiences()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn forgetting_support_removes_pending_and_admitted_shared_derivatives() {
    let agreement = RuntimeResponse::new("我同意保留这个约定。").with_shared_experience(proposal(
        SharedExperienceKind::Agreement,
        "保留这个约定",
        "我同意保留这个约定",
        "我同意保留这个约定",
    ));
    let runtime = ScriptedRuntime::new([PersonTurnClassification::Question], [agreement]);
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    core.run_counterpart_turn(session(), "我同意保留这个约定。", context)
        .unwrap();
    assert_eq!(
        core.repository()
            .all_shared_agreement_candidates()
            .unwrap()
            .len(),
        1
    );

    core.forget(ForgetRequest::new(
        ForgetTarget::ConversationEvidence(EvidenceId::from_raw(1)),
        true,
    ))
    .unwrap();

    assert!(
        core.repository()
            .all_shared_agreement_candidates()
            .unwrap()
            .is_empty()
    );
    assert!(
        core.repository()
            .all_shared_experiences()
            .unwrap()
            .is_empty()
    );
}
