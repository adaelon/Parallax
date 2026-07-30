use eam_core::{
    CoreError, ForgetRequest, ForgetTarget, InMemoryRepository, IncrementingClock, MemoryCore,
    MemoryRepository, PersonTurnClassification, ScriptedRuntime, SessionId,
};

fn core_with_person_fact() -> MemoryCore<InMemoryRepository, ScriptedRuntime, IncrementingClock> {
    let mut core = MemoryCore::new(
        InMemoryRepository::new(),
        ScriptedRuntime::new([PersonTurnClassification::DirectSelfReport], []),
        IncrementingClock::new(1_000),
    );
    core.record_person_turn(SessionId::new("forget"), "我住在深圳。")
        .expect("seed person fact");
    core
}

#[test]
fn forget_requires_explicit_person_confirmation_without_mutating_state() {
    let mut core = core_with_person_fact();
    let evidence_id = core.repository().all_evidence().unwrap()[0].id();

    assert_eq!(
        core.forget(ForgetRequest::new(
            ForgetTarget::ConversationEvidence(evidence_id),
            false,
        )),
        Err(CoreError::ForgetNotConfirmed)
    );
    assert_eq!(core.repository().all_evidence().unwrap().len(), 1);
    assert_eq!(core.repository().all_claims().unwrap().len(), 1);
}

#[test]
fn confirmed_forget_removes_conversation_evidence_and_its_claim_closure_idempotently() {
    let mut core = core_with_person_fact();
    let evidence_id = core.repository().all_evidence().unwrap()[0].id();
    let request = ForgetRequest::new(ForgetTarget::ConversationEvidence(evidence_id), true);

    let first = core.forget(request).expect("confirmed forget");
    let repeated = core.forget(request).expect("repeated forget is idempotent");

    assert_eq!(first, repeated);
    assert_eq!(first.deletion_intent_id(), 1);
    assert_eq!(first.removed_authority_records(), 2);
    assert!(core.repository().all_evidence().unwrap().is_empty());
    assert!(core.repository().all_claims().unwrap().is_empty());
}

#[test]
fn forget_rejects_a_target_that_never_existed() {
    let mut core = core_with_person_fact();

    assert_eq!(
        core.forget(ForgetRequest::new(ForgetTarget::ArchivedEvidence(99), true,)),
        Err(CoreError::ForgetTargetNotFound)
    );
    assert_eq!(core.repository().all_evidence().unwrap().len(), 1);
}
