use eam_core::{EvidenceId, IncrementingClock, SessionId};
use eam_identity::{
    IdentityFormation, IdentityProfile, InitialIdentityProposal, IntroductionAnswer,
    ScriptedIdentityRuntime, SelfIntroductionCategory,
};
use eam_vault::{VaultKey, VaultRepository};

pub fn ready_repository(path: &std::path::Path, key: [u8; 32]) -> VaultRepository {
    let repository = VaultRepository::open(path, VaultKey::new(key)).unwrap();
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
        (1..=6).map(EvidenceId::from_raw).collect(),
    );
    let mut formation = IdentityFormation::new(
        repository,
        ScriptedIdentityRuntime::new([proposal]),
        IncrementingClock::new(100),
    );
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &introduction)
        .unwrap();
    formation.form_initial_counterpart().unwrap();
    let (repository, _, _) = formation.into_parts();
    repository
}
