use eam_core::{
    EvidenceCitation, EvidenceId, IdentityEvolutionRepository, IdentityPersonRepresentation,
    IdentityProfileChanges, IdentityProfileSnapshot, IdentityReflectivePurposeStatus,
    IdentityRevisionAuthorship, IdentityRevisionProposal, IdentityRevisionRejectionReason,
    IdentityRuntimeContext, IdentityStateSnapshot, InMemoryRepository, IncrementingClock,
    MemoryCore, PersonTurnClassification, RuntimeResponse, ScriptedRuntime, SessionId, Timestamp,
};

const PERSON_WORDS: &str = "最近我更需要直白但不武断的提醒。";

fn initial_identity() -> IdentityStateSnapshot {
    IdentityStateSnapshot::restore(
        1,
        None,
        IdentityProfileSnapshot::new(
            "岚",
            "温和、克制",
            "保留分歧",
            "准确高于迎合",
            "同行者",
            "帮助本人看见长期变化",
        ),
        "基于初始自我介绍形成",
        Vec::new(),
        Timestamp::from_millis(10),
    )
}

fn repository_with_identity() -> InMemoryRepository {
    InMemoryRepository::new()
        .with_identity_context(IdentityRuntimeContext::new(7, 1, initial_identity()))
        .unwrap()
}

fn revision(from_version: u64, constitution_version: u64) -> IdentityRevisionProposal {
    IdentityRevisionProposal::new(
        from_version,
        constitution_version,
        IdentityProfileChanges::new(
            None,
            Some("直白、审慎、不武断".to_owned()),
            None,
            None,
            None,
            None,
        ),
        "这更能保持独立判断，同时让提醒可被质疑。",
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(1),
            "直白但不武断",
        )],
    )
}

fn run_with(
    proposal: IdentityRevisionProposal,
) -> (
    MemoryCore<InMemoryRepository, ScriptedRuntime, IncrementingClock>,
    eam_core::TurnOutcome,
) {
    let response = RuntimeResponse::new("我会更直白，但会把判断依据和不确定性说清楚。")
        .with_identity_revision(proposal);
    let mut core = MemoryCore::new(
        repository_with_identity(),
        ScriptedRuntime::new([PersonTurnClassification::Question], [response]),
        IncrementingClock::new(1_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(SessionId::new("identity"), PERSON_WORDS, context)
        .unwrap();
    (core, outcome)
}

#[test]
fn counterpart_revision_appends_one_immutable_version_and_advances_self_bundle() {
    let (core, outcome) = run_with(revision(1, 7));

    let receipt = outcome
        .accepted_identity_revision()
        .expect("valid counterpart revision should be committed");
    assert_eq!(receipt.identity_version(), 2);
    assert_eq!(receipt.self_bundle_version(), 2);
    assert!(outcome.rejected_identity_revisions().is_empty());

    let history = core.repository().identity_history().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0], initial_identity());
    assert_eq!(history[1].predecessor_version(), Some(1));
    assert_eq!(
        history[1].profile().expression_traits(),
        "直白、审慎、不武断"
    );
    assert_eq!(history[1].profile().name(), "岚");
    assert_eq!(
        history[1].evidence_refs(),
        [EvidenceId::from_raw(1), EvidenceId::from_raw(2)]
    );

    let request_identity = core.runtime().seen_requests()[0].identity();
    assert_eq!(request_identity.state().version(), 1);
    assert_eq!(request_identity.constitution_version(), 7);
}

#[test]
fn only_the_current_counterpart_authored_constitution_preserving_revision_is_accepted() {
    let cases = [
        (
            revision(0, 7),
            IdentityRevisionRejectionReason::StalePredecessor {
                expected: 1,
                proposed: 0,
            },
        ),
        (
            revision(1, 7).with_authorship(IdentityRevisionAuthorship::Person),
            IdentityRevisionRejectionReason::PersonAuthoredRoleCard,
        ),
        (
            revision(1, 8),
            IdentityRevisionRejectionReason::ConstitutionVersionChanged {
                expected: 7,
                proposed: 8,
            },
        ),
        (
            revision(1, 7).with_reflective_purpose(IdentityReflectivePurposeStatus::Abandoned),
            IdentityRevisionRejectionReason::ReflectivePurposeAbandoned,
        ),
        (
            revision(1, 7)
                .with_person_representation(IdentityPersonRepresentation::ImpersonatesPerson),
            IdentityRevisionRejectionReason::ImpersonatesPerson,
        ),
        (
            IdentityRevisionProposal::new(
                1,
                7,
                IdentityProfileChanges::new(
                    Some("新名字".to_owned()),
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                "缺少来源",
                Vec::new(),
            ),
            IdentityRevisionRejectionReason::MissingEvidence,
        ),
    ];

    for (proposal, expected) in cases {
        let (core, outcome) = run_with(proposal);
        assert_eq!(outcome.accepted_identity_revision(), None);
        assert_eq!(outcome.rejected_identity_revisions()[0].reason(), &expected);
        assert_eq!(
            core.repository().identity_history().unwrap(),
            [initial_identity()]
        );
    }
}

#[test]
fn a_model_switch_reads_the_committed_identity_chain_instead_of_owning_it() {
    let (first_core, first_outcome) = run_with(revision(1, 7));
    assert_eq!(
        first_outcome
            .accepted_identity_revision()
            .unwrap()
            .identity_version(),
        2
    );
    let (repository, _first_runtime, _clock) = first_core.into_parts();

    let second_proposal = IdentityRevisionProposal::new(
        2,
        7,
        IdentityProfileChanges::new(
            None,
            None,
            Some("更明确地区分事实、推断与分歧".to_owned()),
            None,
            None,
            None,
        ),
        "模型变化不改变我的版本前驱。",
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(3),
            "继续沿用同一身份吗",
        )],
    );
    let mut second_core = MemoryCore::new(
        repository,
        ScriptedRuntime::new(
            [PersonTurnClassification::Question],
            [RuntimeResponse::new("会，我从当前版本继续。")
                .with_identity_revision(second_proposal)],
        ),
        IncrementingClock::new(2_000),
    );
    let context = second_core.freeze_working_context(&[]).unwrap();
    let outcome = second_core
        .run_counterpart_turn(
            SessionId::new("identity-model-switch"),
            "换模型后还会继续沿用同一身份吗？",
            context,
        )
        .unwrap();

    assert_eq!(
        second_core.runtime().seen_requests()[0]
            .identity()
            .state()
            .version(),
        2
    );
    assert_eq!(
        outcome
            .accepted_identity_revision()
            .unwrap()
            .identity_version(),
        3
    );
    assert_eq!(
        second_core.repository().identity_history().unwrap().len(),
        3
    );
}
