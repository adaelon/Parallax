use eam_core::{
    ClaimOwner, EvidenceCitation, EvidenceId, ForgetRequest, ForgetTarget, IncrementingClock,
    MemoryCore, MemoryRepository, PersonTurnClassification, RuntimeResponse, ScriptedRuntime,
    SessionId, SharedAgreementAssent, SharedAgreementCandidateStatus, SharedAgreementDecision,
    SharedAgreementRevision, SharedExperienceKind, SharedExperienceProposal,
    SharedExperienceRepository, Timestamp,
};
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
    assert_eq!(repository.schema_version().unwrap(), 18);
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
