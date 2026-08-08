use eam_core::{
    EvidenceId, IdentityProfileSnapshot, IdentityRuntimeContext, IdentityStateSnapshot,
    InMemoryRepository, IncrementingClock, MemoryRepository, SessionId, Timestamp,
};
use eam_identity::{
    IdentityFormation, IdentityProfile, InitialIdentityProposal, IntroductionAnswer,
    ScriptedIdentityRuntime, SelfIntroductionCategory,
};
use eam_vault::VaultRepository;

pub fn ready_in_memory_repository() -> InMemoryRepository {
    let identity = IdentityStateSnapshot::restore(
        1,
        None,
        IdentityProfileSnapshot::new(
            "测试第二自我",
            "清晰表达",
            "保留独立判断",
            "可追溯性优先",
            "共同回看的同行者",
            "帮助本人形成更准确的自我理解",
        ),
        "确定性测试夹具",
        Vec::new(),
        Timestamp::from_millis(1),
    );
    InMemoryRepository::new()
        .with_identity_context(IdentityRuntimeContext::new(1, 1, identity))
        .expect("the runtime contract counterpart fixture is internally consistent")
}

pub fn make_vault_ready(repository: VaultRepository) -> VaultRepository {
    let introduction = vec![
        IntroductionAnswer::new(
            SelfIntroductionCategory::BasicIdentityAndAddress,
            "我叫林舟，希望被称作阿舟；请不要把你命名成我的副本。",
        ),
        IntroductionAnswer::new(
            SelfIntroductionCategory::CurrentLife,
            "我正在整理长期资料。",
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
    ];
    let first_evidence_id = repository
        .all_evidence()
        .expect("the runtime contract fixture evidence must remain readable")
        .into_iter()
        .map(|evidence| evidence.id().get())
        .max()
        .unwrap_or(0)
        + 1;
    let proposal = InitialIdentityProposal::new(
        IdentityProfile::new(
            "岚",
            "温和、直接、保留不确定性",
            "不把本人的当前自述当作全部真相",
            "可追溯性高于迎合",
            "作为独立的第二自我与本人共同回看",
            "帮助本人形成更准确且可解释的自我理解",
        ),
        "由六类初始介绍形成首版身份",
        (first_evidence_id..first_evidence_id + 6)
            .map(EvidenceId::from_raw)
            .collect(),
    );
    let mut formation = IdentityFormation::new(
        repository,
        ScriptedIdentityRuntime::new([proposal]),
        IncrementingClock::new(2_000),
    );
    formation
        .record_initial_self_introduction(
            &SessionId::new("runtime-contract-onboarding"),
            &introduction,
        )
        .expect("the runtime contract introduction fixture must be valid");
    formation
        .form_initial_counterpart()
        .expect("the runtime contract counterpart fixture must become ready");
    let (repository, _, _) = formation.into_parts();
    repository
}
