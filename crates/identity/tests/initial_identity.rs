use eam_core::{ClaimOwner, EvidenceId, IncrementingClock, MemoryRepository, SessionId, Speaker};
use eam_identity::{
    CounterpartInconsistencyReason, CounterpartReadiness, IdentityAuthorship, IdentityError,
    IdentityFormation, IdentityProfile, IdentityProposalRejectionReason,
    InMemoryIdentityRepository, InitialIdentityProposal, IntroductionAnswer, PersonRepresentation,
    ReflectivePurposeStatus, ScriptedIdentityRuntime, SelfBundleRepository,
    SelfIntroductionCategory,
};

fn complete_introduction() -> Vec<IntroductionAnswer> {
    vec![
        IntroductionAnswer::new(
            SelfIntroductionCategory::BasicIdentityAndAddress,
            "我叫林舟，希望你称呼我为阿舟。",
        ),
        IntroductionAnswer::new(
            SelfIntroductionCategory::CurrentLife,
            "我目前住在香港，正在做一个长期个人软件项目。",
        ),
        IntroductionAnswer::new(
            SelfIntroductionCategory::ImportantPeople,
            "家人和两位老朋友是我最重要的关系。",
        ),
        IntroductionAnswer::new(
            SelfIntroductionCategory::LongTermGoals,
            "我希望建立可持续的创作和生活节奏。",
        ),
        IntroductionAnswer::new(
            SelfIntroductionCategory::CurrentConcerns,
            "我当前担心工作挤压了真实生活。",
        ),
        IntroductionAnswer::new(
            SelfIntroductionCategory::DesiredReflection,
            "请帮助我看见言行不一致之处，但不要替我做决定。",
        ),
    ]
}

fn profile() -> IdentityProfile {
    IdentityProfile::new(
        "岚",
        "温和、直接、保留不确定性",
        "不把本人的当前自述当作全部真相",
        "可追溯性高于迎合",
        "作为独立的第二自我与本人共同回看",
        "帮助本人形成更准确且可解释的自我理解",
    )
}

fn evidence_refs() -> Vec<EvidenceId> {
    (1..=6).map(EvidenceId::from_raw).collect()
}

fn proposal() -> InitialIdentityProposal {
    InitialIdentityProposal::new(
        profile(),
        "基于六类初始自述形成首个关系姿态",
        evidence_refs(),
    )
}

#[test]
fn requires_every_category_before_recording_or_forming_an_identity() {
    let mut answers = complete_introduction();
    answers.pop();
    let runtime = ScriptedIdentityRuntime::new([proposal()]);
    let mut formation = IdentityFormation::new(
        InMemoryIdentityRepository::new(),
        runtime,
        IncrementingClock::new(1_000),
    );

    let error = formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &answers)
        .expect_err("an incomplete introduction must be rejected");

    assert_eq!(
        error,
        IdentityError::MissingCategories(vec![SelfIntroductionCategory::DesiredReflection])
    );
    assert!(formation.current_identity().unwrap().is_none());
    assert!(formation.repository().all_evidence().unwrap().is_empty());
    assert!(formation.repository().all_claims().unwrap().is_empty());
}

#[test]
fn stores_timestamped_person_evidence_and_facts_before_runtime_authored_identity() {
    let runtime = ScriptedIdentityRuntime::new([proposal()]);
    let mut formation = IdentityFormation::new(
        InMemoryIdentityRepository::new(),
        runtime,
        IncrementingClock::new(2_000),
    );

    let introduction = formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &complete_introduction())
        .unwrap();
    assert_eq!(introduction.items().len(), 6);
    assert!(
        introduction
            .items()
            .iter()
            .all(|item| item.recorded_at().as_millis() == 2_000)
    );

    let identity = formation.form_initial_identity().unwrap();

    assert_eq!(identity.version(), 1);
    assert_eq!(identity.predecessor_version(), None);
    assert_eq!(identity.profile().name(), "岚");
    assert_eq!(identity.formed_at().as_millis(), 2_001);
    assert_eq!(identity.evidence_refs(), evidence_refs());
    let evidence = formation.repository().all_evidence().unwrap();
    let claims = formation.repository().all_claims().unwrap();
    assert_eq!(evidence.len(), 6);
    assert!(
        evidence
            .iter()
            .all(|item| item.speaker() == Speaker::Person)
    );
    assert_eq!(claims.len(), 6);
    assert!(
        claims
            .iter()
            .all(|claim| claim.owner() == ClaimOwner::Person)
    );
    assert!(claims.iter().zip(evidence.iter()).all(|(claim, source)| {
        claim.statement() == source.verbatim()
            && claim.support()[0].evidence_id() == source.id()
            && claim.support()[0].quote() == source.verbatim()
    }));
    assert_eq!(formation.runtime().seen_requests().len(), 1);
}

#[test]
fn rejects_person_authored_role_cards_and_preserves_no_identity() {
    let unsafe_proposal = proposal().with_authorship(IdentityAuthorship::Person);
    let runtime = ScriptedIdentityRuntime::new([unsafe_proposal]);
    let mut formation = IdentityFormation::new(
        InMemoryIdentityRepository::new(),
        runtime,
        IncrementingClock::new(3_000),
    );
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &complete_introduction())
        .unwrap();

    let error = formation.form_initial_identity().unwrap_err();

    assert_eq!(
        error,
        IdentityError::InvalidProposal(IdentityProposalRejectionReason::PersonAuthoredRoleCard)
    );
    assert!(formation.current_identity().unwrap().is_none());
}

#[test]
fn rejects_abandoned_reflective_purpose_and_person_impersonation() {
    for (unsafe_proposal, expected) in [
        (
            proposal().with_reflective_purpose(ReflectivePurposeStatus::Abandoned),
            IdentityProposalRejectionReason::ReflectivePurposeAbandoned,
        ),
        (
            proposal().with_person_representation(PersonRepresentation::ImpersonatesPerson),
            IdentityProposalRejectionReason::ImpersonatesPerson,
        ),
    ] {
        let runtime = ScriptedIdentityRuntime::new([unsafe_proposal]);
        let mut formation = IdentityFormation::new(
            InMemoryIdentityRepository::new(),
            runtime,
            IncrementingClock::new(4_000),
        );
        formation
            .record_initial_self_introduction(
                &SessionId::new("onboarding"),
                &complete_introduction(),
            )
            .unwrap();

        assert_eq!(
            formation.form_initial_identity().unwrap_err(),
            IdentityError::InvalidProposal(expected)
        );
        assert!(formation.current_identity().unwrap().is_none());
    }
}

#[test]
fn first_identity_is_immutable_and_cannot_be_formed_twice() {
    let runtime = ScriptedIdentityRuntime::new([proposal(), proposal()]);
    let mut formation = IdentityFormation::new(
        InMemoryIdentityRepository::new(),
        runtime,
        IncrementingClock::new(5_000),
    );
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &complete_introduction())
        .unwrap();
    let first = formation.form_initial_identity().unwrap();

    assert_eq!(
        formation.form_initial_identity().unwrap_err(),
        IdentityError::IdentityAlreadyFormed
    );
    assert_eq!(formation.current_identity().unwrap(), Some(first));
    assert_eq!(formation.runtime().seen_requests().len(), 1);
}

#[test]
fn forms_identity_and_empty_self_bundle_as_one_ready_counterpart() {
    let runtime = ScriptedIdentityRuntime::new([proposal()]);
    let mut formation = IdentityFormation::new(
        InMemoryIdentityRepository::new(),
        runtime,
        IncrementingClock::new(6_000),
    );

    assert_eq!(
        formation.counterpart_readiness().unwrap(),
        CounterpartReadiness::NeedsIntroduction
    );
    assert_eq!(
        formation.form_initial_counterpart().unwrap_err(),
        IdentityError::IntroductionNotRecorded
    );
    assert!(formation.runtime().seen_requests().is_empty());

    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &complete_introduction())
        .unwrap();
    assert_eq!(
        formation.counterpart_readiness().unwrap(),
        CounterpartReadiness::IntroductionRecorded
    );

    assert_eq!(
        formation.form_initial_counterpart().unwrap(),
        CounterpartReadiness::Ready {
            identity_version: 1,
            self_bundle_version: 1,
        }
    );
    let identity = formation.current_identity().unwrap().unwrap();
    let bundle = formation
        .repository()
        .current_self_bundle()
        .unwrap()
        .unwrap();
    assert_eq!(bundle.version(), 1);
    assert_eq!(bundle.predecessor_version(), None);
    assert_eq!(bundle.wake_commit(), None);
    assert_eq!(bundle.state().constitution_version(), 1);
    assert_eq!(bundle.state().identity_state_version(), identity.version());
    assert_eq!(
        bundle.state().relationship_state(),
        identity.profile().relationship_posture()
    );
    assert!(bundle.state().counterpart_experience_refs().is_empty());
    assert!(bundle.state().belief_refs().is_empty());
    assert!(bundle.state().pending_intentions().is_empty());
    assert_eq!(bundle.committed_at(), identity.formed_at());
    assert_eq!(
        formation.counterpart_readiness().unwrap(),
        CounterpartReadiness::Ready {
            identity_version: 1,
            self_bundle_version: 1,
        }
    );
    assert_eq!(
        formation.form_initial_counterpart().unwrap_err(),
        IdentityError::CounterpartAlreadyCreated
    );
    assert_eq!(formation.runtime().seen_requests().len(), 1);
}

#[test]
fn refuses_to_create_over_an_existing_identity_half_state() {
    let runtime = ScriptedIdentityRuntime::new([proposal(), proposal()]);
    let mut formation = IdentityFormation::new(
        InMemoryIdentityRepository::new(),
        runtime,
        IncrementingClock::new(7_000),
    );
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &complete_introduction())
        .unwrap();
    formation.form_initial_identity().unwrap();

    let reason = CounterpartInconsistencyReason::SelfBundleMissing {
        identity_version: 1,
    };
    assert_eq!(
        formation.counterpart_readiness().unwrap(),
        CounterpartReadiness::Inconsistent {
            reason: reason.clone(),
        }
    );
    assert_eq!(
        formation.form_initial_counterpart().unwrap_err(),
        IdentityError::InconsistentCounterpartState(reason)
    );
    assert_eq!(formation.runtime().seen_requests().len(), 1);
}
