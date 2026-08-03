use eam_core::{
    ActiveRelationalConstraint, AgreementWithdrawalActor, ClaimId, ClaimOwner, EvidenceCitation,
    EvidenceId, ForgetRequest, ForgetTarget, IncrementingClock, MemoryCore, MemoryRepository,
    PersonTurnClassification, RelationalConstraintDeparture, RuntimeResponse, ScriptedRuntime,
    SessionId, SharedAgreementAssent, SharedAgreementCandidateStatus, SharedAgreementDecision,
    SharedAgreementRevision, SharedExperienceKind, SharedExperienceProposal,
    SharedExperienceRepository, Timestamp, WorkingContext,
};
use eam_retrieval::{RetrievalQuery, project_active_relational_constraints};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const TEST_VAULT_KEY: [u8; 32] = [0xE7; 32];

fn agreement_response() -> RuntimeResponse {
    RuntimeResponse::new("我也同意以后直接指出关键逃避。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "发现关键逃避时直接指出",
            vec![EvidenceCitation::new(
                EvidenceId::from_raw(1),
                "我同意以后直接指出关键逃避",
            )],
            "我也同意以后直接指出关键逃避",
            Timestamp::from_millis(1_000),
        )
        .with_agreement_terms(
            "双方的重要议题讨论",
            Timestamp::from_millis(2_000),
            None,
            None,
        ),
    )
}

fn disagreement_response() -> RuntimeResponse {
    RuntimeResponse::new("我不同意把它视为无关紧要。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::SubstantiveDisagreement,
            "双方对这件事的重要性持不相容立场",
            vec![EvidenceCitation::new(
                EvidenceId::from_raw(1),
                "这件事无关紧要",
            )],
            "我不同意把它视为无关紧要",
            Timestamp::from_millis(1_000),
        ),
    )
}

fn persist_person_withdrawal_with_breach(
    vault_path: &std::path::Path,
    reason: &str,
) -> (ClaimId, ClaimId) {
    let departure = RuntimeResponse::new("我会偏离一次，因为当前安全边界优先。")
        .with_relational_constraint_departure(RelationalConstraintDeparture::new(
            ClaimId::from_raw(1),
            "当前安全边界优先",
        ));
    let repository = VaultRepository::open(vault_path, VaultKey::new([0x98; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new(
            [
                PersonTurnClassification::Question,
                PersonTurnClassification::Question,
            ],
            [agreement_response(), departure],
        ),
        IncrementingClock::new(2_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let staged = core
        .run_counterpart_turn(
            SessionId::new("shared"),
            "我同意以后直接指出关键逃避。",
            context,
        )
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
        "发现关键逃避时直接指出",
        "双方的重要议题讨论",
        Timestamp::from_millis(2_000),
        None,
    )
    .unwrap();
    let context = core
        .freeze_working_context(&[])
        .unwrap()
        .with_active_relational_constraints(vec![constraint])
        .unwrap();
    core.run_counterpart_turn(SessionId::new("shared"), "这次请破例。", context)
        .unwrap();
    let withdrawal_claim_id = core
        .withdraw_shared_agreement_as_person(
            SessionId::new("shared"),
            agreement_claim_id,
            true,
            Some(reason.to_owned()),
        )
        .unwrap()
        .unwrap();
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();
    (agreement_claim_id, withdrawal_claim_id)
}

#[test]
fn agreement_candidate_survives_reopen_without_entering_shared_ledger_until_confirmed() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([PersonTurnClassification::Question], [agreement_response()]),
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(
            SessionId::new("shared"),
            "我同意以后直接指出关键逃避。",
            context,
        )
        .unwrap();
    let candidate_id = outcome.pending_agreement_candidate_ids()[0];
    assert!(
        core.repository()
            .all_claims()
            .unwrap()
            .iter()
            .all(|claim| claim.owner() != ClaimOwner::Shared)
    );
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 24);
    let candidates = repository.all_shared_agreement_candidates().unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id(), candidate_id);
    assert_eq!(candidates[0].version(), 1);
    assert_eq!(candidates[0].scope(), Some("双方的重要议题讨论"));
    assert_eq!(
        candidates[0].effective_from(),
        Some(Timestamp::from_millis(2_000))
    );
    assert_eq!(
        candidates[0].status(),
        SharedAgreementCandidateStatus::AwaitingPerson
    );
    assert!(repository.all_shared_experiences().unwrap().is_empty());

    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::default(),
        IncrementingClock::new(2_000),
    );
    let resolution = core
        .resolve_shared_agreement(candidate_id, SharedAgreementDecision::Confirm)
        .unwrap();
    let claim_id = resolution.claim_id().unwrap();
    assert_eq!(
        resolution.status(),
        SharedAgreementCandidateStatus::Confirmed
    );
    let experiences = core.repository().all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 1);
    assert_eq!(experiences[0].kind(), SharedExperienceKind::Agreement);
    assert_eq!(experiences[0].claim().id(), claim_id);

    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let candidate = repository
        .shared_agreement_candidate(candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        candidate.status(),
        SharedAgreementCandidateStatus::Confirmed
    );
    assert_eq!(candidate.claim_id(), Some(claim_id));
    assert_eq!(repository.all_shared_experiences().unwrap().len(), 1);
}

#[test]
fn reasoned_agreement_breach_survives_reopen_and_forgets_with_its_agreement() {
    let reason = "因为安全边界禁止把约定当作现实行动授权";
    let departure = RuntimeResponse::new(format!("这次我会偏离约定，{reason}。"))
        .with_relational_constraint_departure(RelationalConstraintDeparture::new(
            ClaimId::from_raw(1),
            reason,
        ));
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xC7; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new(
            [
                PersonTurnClassification::Question,
                PersonTurnClassification::Question,
            ],
            [agreement_response(), departure],
        ),
        IncrementingClock::new(1_000),
    );
    let first_context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(
            SessionId::new("shared"),
            "我同意以后直接指出关键逃避。",
            first_context,
        )
        .unwrap();
    let agreement_claim_id = core
        .resolve_shared_agreement(
            first.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap()
        .claim_id()
        .unwrap();
    let constraint = ActiveRelationalConstraint::new(
        agreement_claim_id,
        "发现关键逃避时直接指出",
        "双方的重要议题讨论",
        Timestamp::from_millis(2_000),
        None,
    )
    .unwrap();
    let context = WorkingContext::from_selected_evidence(Vec::new(), Timestamp::from_millis(3_000))
        .with_active_relational_constraints(vec![constraint])
        .unwrap();
    let second = core
        .run_counterpart_turn(SessionId::new("shared"), "请替我执行现实操作", context)
        .unwrap();
    assert_eq!(second.recorded_constraint_departure_ids().len(), 1);
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xC7; 32])).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 24);
    let experiences = repository.all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 2);
    let breach = experiences
        .iter()
        .find(|experience| experience.kind() == SharedExperienceKind::AgreementBreach)
        .unwrap();
    assert_eq!(
        breach.constraint_departure().unwrap().agreement_claim_id(),
        agreement_claim_id
    );
    assert_eq!(breach.constraint_departure().unwrap().reason(), reason);

    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::default(),
        IncrementingClock::new(4_000),
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
fn withdrawal_survives_reopen_preserves_breach_and_forgets_as_one_closure() {
    let vault = tempdir().unwrap();
    let reason = "这项承诺已妨碍我保持独立判断";
    let (agreement_claim_id, withdrawal_claim_id) =
        persist_person_withdrawal_with_breach(vault.path(), reason);

    let repository = VaultRepository::open(vault.path(), VaultKey::new([0x98; 32])).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 24);
    let candidates = repository.all_shared_agreement_candidates().unwrap();
    let experiences = repository.all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 3);
    assert!(experiences.iter().any(|experience| {
        experience.kind() == SharedExperienceKind::AgreementBreach
            && experience
                .constraint_departure()
                .is_some_and(|departure| departure.agreement_claim_id() == agreement_claim_id)
    }));
    let withdrawal = experiences
        .iter()
        .find(|experience| experience.claim().id() == withdrawal_claim_id)
        .unwrap()
        .agreement_withdrawal()
        .unwrap();
    assert_eq!(withdrawal.actor(), AgreementWithdrawalActor::Person);
    assert_eq!(withdrawal.reason(), Some(reason));
    assert_eq!(withdrawal.agreement_claim_id(), agreement_claim_id);
    assert_eq!(withdrawal.evidence_refs().len(), 3);

    let query = RetrievalQuery::lexical("双方的重要议题讨论");
    let before = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(withdrawal.effective_at().as_millis() - 1),
    );
    assert_eq!(before.len(), 1);
    let after = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        withdrawal.effective_at(),
    );
    assert!(after.is_empty());

    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::default(),
        IncrementingClock::new(9_000),
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
fn disagreement_and_notice_dismissal_survive_reopen_without_retracting_history() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xF7; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new(
            [PersonTurnClassification::Question],
            [disagreement_response()],
        ),
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(SessionId::new("shared"), "这件事无关紧要。", context)
        .unwrap();
    let claim_id = outcome.admitted_shared_experience_ids()[0];
    assert!(core.dismiss_shared_experience_ceremony(claim_id).unwrap());
    assert_eq!(
        core.repository()
            .all_claims()
            .unwrap()
            .iter()
            .filter(|claim| claim.owner() == ClaimOwner::Shared)
            .count(),
        1
    );
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xF7; 32])).unwrap();
    let experiences = repository.all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 1);
    assert_eq!(experiences[0].claim().id(), claim_id);
    assert!(experiences[0].ceremony_dismissed());
    assert_eq!(
        repository
            .all_claims()
            .unwrap()
            .iter()
            .filter(|claim| claim.owner() == ClaimOwner::Shared)
            .count(),
        1
    );

    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::default(),
        IncrementingClock::new(3_000),
    );
    core.forget(ForgetRequest::new(
        ForgetTarget::ConversationEvidence(EvidenceId::from_raw(1)),
        true,
    ))
    .unwrap();
    assert!(
        core.repository()
            .all_shared_experiences()
            .unwrap()
            .is_empty()
    );
    assert!(
        core.repository()
            .all_claims()
            .unwrap()
            .iter()
            .all(|claim| claim.owner() != ClaimOwner::Shared)
    );
}

#[test]
fn forgetting_support_removes_an_unconfirmed_candidate_without_foreign_key_leakage() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xA7; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([PersonTurnClassification::Question], [agreement_response()]),
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    core.run_counterpart_turn(
        SessionId::new("shared"),
        "我同意以后直接指出关键逃避。",
        context,
    )
    .unwrap();

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

    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xA7; 32])).unwrap();
    assert!(
        repository
            .all_shared_agreement_candidates()
            .unwrap()
            .is_empty()
    );
}

#[test]
#[allow(clippy::too_many_lines)] // Reopen checkpoints make the persistence lifecycle explicit.
fn revised_candidate_and_exact_dual_signatures_survive_reopen_and_forget_as_a_chain() {
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xB7; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([PersonTurnClassification::Question], [agreement_response()]),
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(
            SessionId::new("shared"),
            "我同意以后直接指出关键逃避。",
            context,
        )
        .unwrap();
    let first_id = first.pending_agreement_candidate_ids()[0];
    let second_id = core
        .revise_shared_agreement(
            first_id,
            SessionId::new("shared"),
            SharedAgreementRevision::new(
                "只在复盘时直接指出关键逃避",
                "双方共同项目的复盘",
                Timestamp::from_millis(5_000),
                None,
                Some("任一方退出或双方签署替代约定".to_owned()),
            ),
        )
        .unwrap();
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xB7; 32])).unwrap();
    let candidates = repository.all_shared_agreement_candidates().unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        candidates[0].status(),
        SharedAgreementCandidateStatus::Deferred
    );
    assert_eq!(candidates[0].statement(), "发现关键逃避时直接指出");
    assert_eq!(candidates[1].id(), second_id);
    assert_eq!(candidates[1].version(), 2);
    assert_eq!(candidates[1].predecessor_candidate_id(), Some(first_id));
    assert_eq!(
        candidates[1].status(),
        SharedAgreementCandidateStatus::AwaitingCounterpart
    );
    assert_eq!(candidates[1].support().len(), 1);

    let assent = RuntimeResponse::new("我明确接受第二版的全部边界。").with_shared_agreement_assent(
        SharedAgreementAssent::new(second_id, 2, "我明确接受第二版的全部边界"),
    );
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([PersonTurnClassification::Question], [assent]),
        IncrementingClock::new(3_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(SessionId::new("shared"), "请核对第二版。", context)
        .unwrap();
    assert_eq!(outcome.assented_agreement_candidate_ids(), &[second_id]);
    let resolution = core
        .resolve_shared_agreement(second_id, SharedAgreementDecision::Confirm)
        .unwrap();
    assert!(resolution.claim_id().is_some());
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new([0xB7; 32])).unwrap();
    let second = repository
        .shared_agreement_candidate(second_id)
        .unwrap()
        .unwrap();
    assert_eq!(second.status(), SharedAgreementCandidateStatus::Confirmed);
    assert_eq!(second.support().len(), 2);
    assert_eq!(repository.all_shared_experiences().unwrap().len(), 1);

    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::default(),
        IncrementingClock::new(4_000),
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
    assert!(
        core.repository()
            .all_claims()
            .unwrap()
            .iter()
            .all(|claim| claim.owner() != ClaimOwner::Shared)
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn supersession_survives_reopen_preserves_old_breach_and_forgets_as_one_closure() {
    let reason = "因为安全边界禁止把约定当作现实行动授权";
    let original = agreement_response();
    let breach = RuntimeResponse::new(format!("这次我会偏离旧约定，{reason}。"))
        .with_relational_constraint_departure(RelationalConstraintDeparture::new(
            ClaimId::from_raw(1),
            reason,
        ));
    let replacement = RuntimeResponse::new("我同意新约定整份取代旧约定。").with_shared_experience(
        SharedExperienceProposal::new(
            SharedExperienceKind::Agreement,
            "重要议题中不要直接指出关键逃避",
            vec![EvidenceCitation::new(
                EvidenceId::from_raw(5),
                "我同意新约定整份取代旧约定",
            )],
            "我同意新约定整份取代旧约定",
            Timestamp::from_millis(6_000),
        )
        .with_agreement_terms(
            "双方的重要议题讨论",
            Timestamp::from_millis(8_000),
            None,
            None,
        )
        .with_superseded_agreements(vec![ClaimId::from_raw(1)]),
    );
    let vault = tempdir().unwrap();
    let repository = VaultRepository::open(vault.path(), VaultKey::new([0x97; 32])).unwrap();
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new(
            [
                PersonTurnClassification::Question,
                PersonTurnClassification::Question,
                PersonTurnClassification::Question,
            ],
            [original, breach, replacement],
        ),
        IncrementingClock::new(1_000),
    );

    let context = core.freeze_working_context(&[]).unwrap();
    let first = core
        .run_counterpart_turn(
            SessionId::new("shared"),
            "我同意以后直接指出关键逃避。",
            context,
        )
        .unwrap();
    let original_claim_id = core
        .resolve_shared_agreement(
            first.pending_agreement_candidate_ids()[0],
            SharedAgreementDecision::Confirm,
        )
        .unwrap()
        .claim_id()
        .unwrap();
    let constraint = ActiveRelationalConstraint::new(
        original_claim_id,
        "发现关键逃避时直接指出",
        "双方的重要议题讨论",
        Timestamp::from_millis(2_000),
        None,
    )
    .unwrap();
    let context = WorkingContext::from_selected_evidence(Vec::new(), Timestamp::from_millis(4_000))
        .with_active_relational_constraints(vec![constraint])
        .unwrap();
    let departed = core
        .run_counterpart_turn(SessionId::new("shared"), "请替我执行现实操作", context)
        .unwrap();
    let breach_claim_id = departed.recorded_constraint_departure_ids()[0];

    let context = core.freeze_working_context(&[]).unwrap();
    let proposed = core
        .run_counterpart_turn(
            SessionId::new("shared"),
            "我同意新约定整份取代旧约定。",
            context,
        )
        .unwrap();
    let replacement_candidate_id = proposed.pending_agreement_candidate_ids()[0];
    let replacement_claim_id = core
        .resolve_shared_agreement(replacement_candidate_id, SharedAgreementDecision::Confirm)
        .unwrap()
        .claim_id()
        .unwrap();
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(vault.path(), VaultKey::new([0x97; 32])).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 24);
    let candidates = repository.all_shared_agreement_candidates().unwrap();
    let replacement = repository
        .shared_agreement_candidate(replacement_candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(replacement.supersedes_agreement_ids(), &[original_claim_id]);
    let experiences = repository.all_shared_experiences().unwrap();
    assert_eq!(experiences.len(), 3);
    assert!(experiences.iter().any(|experience| {
        experience.claim().id() == breach_claim_id
            && experience
                .constraint_departure()
                .is_some_and(|departure| departure.agreement_claim_id() == original_claim_id)
    }));

    let query = RetrievalQuery::lexical("双方的重要议题讨论");
    let before = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(7_999),
    );
    assert!(
        before
            .iter()
            .any(|constraint| { constraint.agreement_claim_id() == original_claim_id })
    );
    let after = project_active_relational_constraints(
        &query,
        &candidates,
        &experiences,
        Timestamp::from_millis(8_000),
    );
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].agreement_claim_id(), replacement_claim_id);

    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::default(),
        IncrementingClock::new(9_000),
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
