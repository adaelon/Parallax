use eam_core::{
    ActiveRelationalConstraint, AgreementWithdrawalActor, AgreementWithdrawalProposal,
    AgreementWithdrawalRejectionReason, ApplicableTime, Claim, ClaimId, ClaimOwner, CoreError,
    EvidenceCitation, EvidenceId, ForgetRequest, ForgetTarget, IncrementingClock, MemoryCore,
    MemoryRepository, RuntimeResponse, ScriptedPersonFactResponse, ScriptedRuntime, SessionId,
    SharedAgreementAssent, SharedAgreementAssentRejectionReason, SharedAgreementCandidateStatus,
    SharedAgreementDecision, SharedAgreementRevision, SharedExperience, SharedExperienceKind,
    SharedExperienceProposal, SharedExperienceRejectionReason, SharedExperienceRepository,
    Timestamp,
};

mod support;
use support::ready_repository;

fn session() -> SessionId {
    SessionId::new("shared-experience")
}

fn proposal(
    kind: SharedExperienceKind,
    statement: &str,
    person_quote: &str,
    counterpart_quote: &str,
) -> SharedExperienceProposal {
    let proposal = SharedExperienceProposal::new(
        kind,
        statement,
        vec![EvidenceCitation::new(EvidenceId::from_raw(1), person_quote)],
        counterpart_quote,
        Timestamp::from_millis(1_000),
    );
    if kind == SharedExperienceKind::Agreement {
        proposal.with_agreement_terms(
            "双方关于重要议题的持续对话",
            Timestamp::from_millis(2_000),
            None,
            None,
        )
    } else {
        proposal
    }
}

#[test]
fn ordinary_question_and_person_external_experience_do_not_enter_shared_ledger() {
    let runtime = ScriptedRuntime::new(
        [
            ScriptedPersonFactResponse::NoFacts,
            ScriptedPersonFactResponse::VerbatimFactAtRecordedTime,
        ],
        [RuntimeResponse::new("这是普通回答。")],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));

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
    let runtime = ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [response]);
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));

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
    assert_eq!(candidate.version(), 1);
    assert_eq!(candidate.scope(), Some("双方关于重要议题的持续对话"));
    assert_eq!(
        candidate.effective_from(),
        Some(Timestamp::from_millis(2_000))
    );
    assert_eq!(candidate.effective_until(), None);
    assert_eq!(candidate.end_condition(), None);
}

#[test]
fn agreement_without_scope_or_effective_time_is_rejected_before_signing() {
    let missing_scope = RuntimeResponse::new("我同意，但还没有边界。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "边界不完整的约定",
            vec![EvidenceCitation::new(
                EvidenceId::from_raw(1),
                "我先同意内容",
            )],
            "我同意",
            Timestamp::from_millis(1_000),
        ),
    );
    let missing_time = RuntimeResponse::new("我同意范围，但没有生效时间。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "仍不完整的约定",
            vec![EvidenceCitation::new(EvidenceId::from_raw(3), "我同意范围")],
            "我同意范围",
            Timestamp::from_millis(2_000),
        )
        .with_agreement_scope("重要议题"),
    );
    let mut core = MemoryCore::new(
        ready_repository(),
        ScriptedRuntime::new(
            [
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
            ],
            [missing_scope, missing_time],
        ),
        IncrementingClock::new(1_000),
    );

    let first_context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(session(), "我先同意内容。", first_context)
        .unwrap();
    assert_eq!(
        first.rejected_shared_experiences()[0].reason(),
        &SharedExperienceRejectionReason::MissingAgreementScope
    );

    let second_context = core.freeze_working_context(&[]).unwrap();
    let second = core
        .run_counterpart_turn(session(), "我同意范围。", second_context)
        .unwrap();
    assert_eq!(
        second.rejected_shared_experiences()[0].reason(),
        &SharedExperienceRejectionReason::MissingAgreementEffectiveFrom
    );
    assert!(
        core.repository()
            .all_shared_agreement_candidates()
            .unwrap()
            .is_empty()
    );
}

#[test]
#[allow(clippy::too_many_lines)] // One end-to-end state-machine test keeps every transition visible.
fn person_revision_creates_new_version_that_requires_exact_counterpart_assent() {
    let initial = RuntimeResponse::new("我同意第一版。").with_shared_experience(proposal(
        SharedExperienceKind::Agreement,
        "第一版约定",
        "我同意第一版",
        "我同意第一版",
    ));
    let wrong_version = RuntimeResponse::new("我接受修改后的约定。").with_shared_agreement_assent(
        SharedAgreementAssent::new(
            eam_core::SharedAgreementCandidateId::from_raw(2),
            1,
            "我接受修改后的约定",
        ),
    );
    let exact_version = RuntimeResponse::new("我明确接受第二版约定。")
        .with_shared_agreement_assent(SharedAgreementAssent::new(
            eam_core::SharedAgreementCandidateId::from_raw(2),
            2,
            "我明确接受第二版约定",
        ));
    let mut core = MemoryCore::new(
        ready_repository(),
        ScriptedRuntime::new(
            [
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
            ],
            [initial, wrong_version, exact_version],
        ),
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let initial_outcome = core
        .run_counterpart_turn(session(), "我同意第一版。", context)
        .unwrap();
    let first_id = initial_outcome.pending_agreement_candidate_ids()[0];
    let second_id = core
        .revise_shared_agreement(
            first_id,
            session(),
            SharedAgreementRevision::new(
                "第二版约定",
                "只适用于共同项目复盘",
                Timestamp::from_millis(5_000),
                Some(Timestamp::from_millis(9_000)),
                None,
            ),
        )
        .unwrap();

    let candidates = core.repository().all_shared_agreement_candidates().unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].statement(), "第一版约定");
    assert_eq!(
        candidates[0].status(),
        SharedAgreementCandidateStatus::Deferred
    );
    assert_eq!(candidates[1].id(), second_id);
    assert_eq!(candidates[1].version(), 2);
    assert_eq!(candidates[1].predecessor_candidate_id(), Some(first_id));
    assert_eq!(
        candidates[1].status(),
        SharedAgreementCandidateStatus::AwaitingCounterpart
    );
    assert_eq!(candidates[1].support().len(), 1);
    assert!(
        core.resolve_shared_agreement(second_id, SharedAgreementDecision::Confirm)
            .is_err()
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let wrong = core
        .run_counterpart_turn(session(), "请核对修改版。", context)
        .unwrap();
    assert_eq!(
        wrong.rejected_agreement_assents()[0].reason(),
        &SharedAgreementAssentRejectionReason::VersionMismatch {
            candidate_id: second_id,
            expected: 2,
            actual: 1,
        }
    );
    assert_eq!(
        core.runtime().seen_requests()[1].pending_agreement_candidates()[0].id(),
        second_id
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let exact = core
        .run_counterpart_turn(session(), "再确认精确版本。", context)
        .unwrap();
    assert_eq!(exact.assented_agreement_candidate_ids(), &[second_id]);
    let second = core
        .repository()
        .shared_agreement_candidate(second_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        second.status(),
        SharedAgreementCandidateStatus::AwaitingPerson
    );
    assert_eq!(second.support().len(), 2);

    let resolution = core
        .resolve_shared_agreement(second_id, SharedAgreementDecision::Confirm)
        .unwrap();
    let claim = core
        .repository()
        .all_claims()
        .unwrap()
        .into_iter()
        .find(|claim| claim.id() == resolution.claim_id().unwrap())
        .unwrap();
    assert_eq!(
        claim.applicable_time(),
        ApplicableTime::Between {
            start: Timestamp::from_millis(5_000),
            end: Timestamp::from_millis(9_000),
        }
    );
    assert_eq!(claim.support(), second.support());
}

#[test]
fn person_confirmation_admits_agreement_while_deferral_keeps_it_out() {
    let confirmed = RuntimeResponse::new("我同意 A。").with_shared_experience(proposal(
        SharedExperienceKind::Agreement,
        "共同决定 A",
        "我同意 A",
        "我同意 A",
    ));
    let deferred = RuntimeResponse::new("我同意 B。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "共同决定 B",
            vec![EvidenceCitation::new(EvidenceId::from_raw(3), "我同意 B")],
            "我同意 B",
            Timestamp::from_millis(2_000),
        )
        .with_agreement_terms("共同项目 B", Timestamp::from_millis(2_000), None, None),
    );
    let runtime = ScriptedRuntime::new(
        [
            ScriptedPersonFactResponse::NoFacts,
            ScriptedPersonFactResponse::NoFacts,
        ],
        [confirmed, deferred],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));

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
    let runtime = ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [response]);
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));

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
    let runtime = ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [response]);
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));

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
            ScriptedPersonFactResponse::NoFacts,
            ScriptedPersonFactResponse::NoFacts,
        ],
        [],
    );
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));
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
    let runtime = ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [agreement]);
    let mut core = MemoryCore::new(ready_repository(), runtime, IncrementingClock::new(1_000));
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

#[test]
fn conflicting_agreement_requires_explicit_whole_supersession_before_staging() {
    let original = RuntimeResponse::new("我同意在复盘时直接指出关键逃避。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "复盘时直接指出关键逃避",
            vec![EvidenceCitation::new(
                EvidenceId::from_raw(1),
                "我同意在复盘时直接指出关键逃避",
            )],
            "我同意在复盘时直接指出关键逃避",
            Timestamp::from_millis(1_000),
        )
        .with_agreement_terms(
            "双方共同项目复盘",
            Timestamp::from_millis(2_000),
            None,
            None,
        ),
    );
    let undeclared = RuntimeResponse::new("我同意复盘时不要直接指出关键逃避。")
        .with_shared_experience(
            SharedExperienceProposal::new(
                SharedExperienceKind::Agreement,
                "复盘时不要直接指出关键逃避",
                vec![EvidenceCitation::new(
                    EvidenceId::from_raw(3),
                    "我同意复盘时不要直接指出关键逃避",
                )],
                "我同意复盘时不要直接指出关键逃避",
                Timestamp::from_millis(3_000),
            )
            .with_agreement_terms(
                "双方共同项目复盘",
                Timestamp::from_millis(4_000),
                None,
                None,
            ),
        );
    let declared = RuntimeResponse::new("我确认这份新约定将整份取代旧约定。")
        .with_shared_experience(
            SharedExperienceProposal::new(
                SharedExperienceKind::Agreement,
                "复盘时不要直接指出关键逃避",
                vec![EvidenceCitation::new(
                    EvidenceId::from_raw(5),
                    "我确认这份新约定将整份取代旧约定",
                )],
                "我确认这份新约定将整份取代旧约定",
                Timestamp::from_millis(5_000),
            )
            .with_agreement_terms(
                "双方共同项目复盘",
                Timestamp::from_millis(6_000),
                None,
                None,
            )
            .with_superseded_agreements(vec![ClaimId::from_raw(1)]),
        );
    let mut core = MemoryCore::new(
        ready_repository(),
        ScriptedRuntime::new(
            [
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
            ],
            [original, undeclared, declared],
        ),
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(session(), "我同意在复盘时直接指出关键逃避。", context)
        .unwrap();
    let original_resolution = core
        .resolve_shared_agreement(
            first.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap();
    let original_claim_id = original_resolution.claim_id().unwrap();

    let context = core.freeze_working_context(&[]).unwrap();
    let blocked = core
        .run_counterpart_turn(session(), "我同意复盘时不要直接指出关键逃避。", context)
        .unwrap();
    assert_eq!(
        blocked.rejected_shared_experiences()[0].reason(),
        &SharedExperienceRejectionReason::ConflictingAgreementsRequireExplicitSupersession(vec![
            original_claim_id,
        ])
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let staged = core
        .run_counterpart_turn(session(), "我确认这份新约定将整份取代旧约定。", context)
        .unwrap();
    let replacement = core
        .repository()
        .shared_agreement_candidate(staged.pending_agreement_candidate_ids()[0])
        .unwrap()
        .unwrap();
    assert_eq!(replacement.supersedes_agreement_ids(), &[original_claim_id]);
}

#[test]
fn compatible_agreement_with_overlapping_scope_can_remain_parallel() {
    let first = RuntimeResponse::new("我同意复盘前提供议程。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "复盘前提供议程",
            vec![EvidenceCitation::new(
                EvidenceId::from_raw(1),
                "我同意复盘前提供议程",
            )],
            "我同意复盘前提供议程",
            Timestamp::from_millis(1_000),
        )
        .with_agreement_terms(
            "双方共同项目复盘",
            Timestamp::from_millis(2_000),
            None,
            None,
        ),
    );
    let second = RuntimeResponse::new("我同意复盘后记录结论。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "复盘后记录结论",
            vec![EvidenceCitation::new(
                EvidenceId::from_raw(3),
                "我同意复盘后记录结论",
            )],
            "我同意复盘后记录结论",
            Timestamp::from_millis(3_000),
        )
        .with_agreement_terms(
            "双方共同项目复盘",
            Timestamp::from_millis(4_000),
            None,
            None,
        ),
    );
    let mut core = MemoryCore::new(
        ready_repository(),
        ScriptedRuntime::new(
            [
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
            ],
            [first, second],
        ),
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(session(), "我同意复盘前提供议程。", context)
        .unwrap();
    core.resolve_shared_agreement(
        first.pending_agreement_candidate_ids()[0],
        SharedAgreementDecision::Confirm,
    )
    .unwrap();
    let context = core.freeze_working_context(&[]).unwrap();
    let second = core
        .run_counterpart_turn(session(), "我同意复盘后记录结论。", context)
        .unwrap();

    assert!(second.rejected_shared_experiences().is_empty());
    let candidate = core
        .repository()
        .shared_agreement_candidate(second.pending_agreement_candidate_ids()[0])
        .unwrap()
        .unwrap();
    assert!(candidate.supersedes_agreement_ids().is_empty());
}

#[test]
fn person_withdrawal_requires_confirmation_and_preserves_agreement_history() {
    let agreement = RuntimeResponse::new("我同意保留退出自由。").with_shared_experience(proposal(
        SharedExperienceKind::Agreement,
        "重要议题中直接指出关键逃避",
        "我同意保留退出自由",
        "我同意保留退出自由",
    ));
    let mut core = MemoryCore::new(
        ready_repository(),
        ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [agreement]),
        IncrementingClock::new(2_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let staged = core
        .run_counterpart_turn(session(), "我同意保留退出自由。", context)
        .unwrap();
    let agreement_claim_id = core
        .resolve_shared_agreement(
            staged.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap()
        .claim_id()
        .unwrap();

    assert_eq!(
        core.withdraw_shared_agreement_as_person(
            session(),
            agreement_claim_id,
            false,
            Some("现在不再适合".to_owned()),
        )
        .unwrap(),
        None
    );
    assert_eq!(core.repository().all_shared_experiences().unwrap().len(), 1);

    let withdrawal_claim_id = core
        .withdraw_shared_agreement_as_person(session(), agreement_claim_id, true, None)
        .unwrap()
        .unwrap();
    let experiences = core.repository().all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 2, "withdrawal must retain the agreement");
    let withdrawal = experiences
        .iter()
        .find(|experience| experience.claim().id() == withdrawal_claim_id)
        .unwrap()
        .agreement_withdrawal()
        .unwrap();
    assert_eq!(withdrawal.actor(), AgreementWithdrawalActor::Person);
    assert_eq!(withdrawal.agreement_claim_id(), agreement_claim_id);
    assert_eq!(withdrawal.reason(), None);
    assert_eq!(withdrawal.evidence_refs().len(), 3);
    assert!(matches!(
        core.withdraw_shared_agreement_as_person(session(), agreement_claim_id, true, None),
        Err(CoreError::SharedAgreementNotActive(id)) if id == agreement_claim_id
    ));
}

#[test]
fn counterpart_withdrawal_requires_a_verbatim_reason_and_is_immediate() {
    let agreement = RuntimeResponse::new("我同意在复盘时直接指出关键逃避。")
        .with_shared_experience(proposal(
            SharedExperienceKind::Agreement,
            "复盘时直接指出关键逃避",
            "我同意在复盘时直接指出关键逃避",
            "我同意在复盘时直接指出关键逃避",
        ));
    let empty_reason = RuntimeResponse::new("我想退出这项约定。")
        .with_agreement_withdrawal(AgreementWithdrawalProposal::new(ClaimId::from_raw(1), ""));
    let reason = "它已妨碍我诚实表达独立判断";
    let valid =
        RuntimeResponse::new(format!("我退出这项约定，因为{reason}。")).with_agreement_withdrawal(
            AgreementWithdrawalProposal::new(ClaimId::from_raw(1), reason),
        );
    let mut core = MemoryCore::new(
        ready_repository(),
        ScriptedRuntime::new(
            [
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
            ],
            [agreement, empty_reason, valid],
        ),
        IncrementingClock::new(2_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let staged = core
        .run_counterpart_turn(session(), "我同意在复盘时直接指出关键逃避。", context)
        .unwrap();
    let agreement_claim_id = core
        .resolve_shared_agreement(
            staged.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap()
        .claim_id()
        .unwrap();
    let constraint = ActiveRelationalConstraint::new(
        agreement_claim_id,
        "复盘时直接指出关键逃避",
        "双方关于重要议题的持续对话",
        Timestamp::from_millis(2_000),
        None,
    )
    .unwrap();

    let context = core
        .freeze_working_context(&[])
        .unwrap()
        .with_active_relational_constraints(vec![constraint.clone()])
        .unwrap();
    let rejected = core
        .run_counterpart_turn(session(), "你仍愿意遵守吗？", context)
        .unwrap();
    assert_eq!(
        rejected.rejected_agreement_withdrawals()[0].reason(),
        &AgreementWithdrawalRejectionReason::EmptyReason
    );
    assert_eq!(core.repository().all_shared_experiences().unwrap().len(), 1);

    let context = core
        .freeze_working_context(&[])
        .unwrap()
        .with_active_relational_constraints(vec![constraint])
        .unwrap();
    let withdrawn = core
        .run_counterpart_turn(session(), "请重新判断这项约定。", context)
        .unwrap();
    assert_eq!(withdrawn.recorded_agreement_withdrawal_ids().len(), 1);
    let experiences = core.repository().all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 2);
    let withdrawal = experiences[1].agreement_withdrawal().unwrap();
    assert_eq!(withdrawal.actor(), AgreementWithdrawalActor::Counterpart);
    assert_eq!(withdrawal.agreement_claim_id(), agreement_claim_id);
    assert_eq!(withdrawal.reason(), Some(reason));
    assert_eq!(
        withdrawal.effective_at(),
        experiences[1].claim().recorded_at()
    );
}
