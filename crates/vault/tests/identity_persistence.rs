use std::fs;

use eam_core::{
    ClaimOwner, EvidenceCitation, EvidenceId, IdentityEvolutionRepository, IdentityProfileChanges,
    IdentityProfileSnapshot, IdentityRevisionCommit, IdentityRevisionProposal,
    IdentityStateSnapshot, IncrementingClock, MemoryCore, MemoryRepository,
    PersonTurnClassification, RuntimeResponse, ScriptedRuntime, SessionId, Speaker, Timestamp,
};
use eam_identity::{
    IdentityFormation, IdentityProfile, InitialIdentityProposal, IntroductionAnswer,
    ScriptedIdentityRuntime, SelfBundleRepository, SelfBundleState, SelfBundleVersion,
    SelfIntroductionCategory,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const VAULT_KEY_BYTES: [u8; 32] = [0x73; 32];
const PRIVATE_MARKER: &str = "S04-固定自述-不得出现在数据库字节中";

fn key() -> VaultKey {
    VaultKey::new(VAULT_KEY_BYTES)
}

fn introduction() -> Vec<IntroductionAnswer> {
    vec![
        IntroductionAnswer::new(
            SelfIntroductionCategory::BasicIdentityAndAddress,
            "我叫林舟，希望被称作阿舟；请不要把你命名成我的副本。",
        ),
        IntroductionAnswer::new(SelfIntroductionCategory::CurrentLife, PRIVATE_MARKER),
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

fn proposal() -> InitialIdentityProposal {
    InitialIdentityProposal::new(
        IdentityProfile::new(
            "岚",
            "温和、直接、保留不确定性",
            "不把本人的当前自述当作全部真相",
            "可追溯性高于迎合",
            "作为独立的第二自我与本人共同回看",
            "帮助本人形成更准确且可解释的自我理解",
        ),
        "基于六类初始自述形成首个关系姿态",
        (1..=6).map(EvidenceId::from_raw).collect(),
    )
}

fn repository_with_identity_bundle(path: &std::path::Path) -> VaultRepository {
    let repository = VaultRepository::open(path, key()).unwrap();
    let mut formation = IdentityFormation::new(
        repository,
        ScriptedIdentityRuntime::new([proposal()]),
        IncrementingClock::new(10_000),
    );
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &introduction())
        .unwrap();
    formation.form_initial_identity().unwrap();
    let (mut repository, _, _) = formation.into_parts();
    repository
        .append_self_bundle(SelfBundleVersion::restore(
            1,
            None,
            SelfBundleState::new(1, 1, Vec::new(), Vec::new(), "共同认识正在形成", Vec::new())
                .unwrap(),
            None,
            Timestamp::from_millis(10_100),
        ))
        .unwrap();
    repository
}

#[test]
fn reopens_the_same_first_identity_with_its_person_evidence_and_facts() {
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 25);
    let runtime = ScriptedIdentityRuntime::new([proposal()]);
    let mut formation = IdentityFormation::new(repository, runtime, IncrementingClock::new(10_000));

    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &introduction())
        .unwrap();
    let first_identity = formation.form_initial_identity().unwrap();
    assert_eq!(first_identity.profile().name(), "岚");
    assert_ne!(first_identity.profile().name(), "阿舟");

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
        claim.support()[0].evidence_id() == source.id()
            && claim.support()[0].quote() == source.verbatim()
    }));

    let (repository, _, _) = formation.into_parts();
    let database_path = repository.database_path().to_owned();
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let runtime = ScriptedIdentityRuntime::new([]);
    let reopened = IdentityFormation::new(repository, runtime, IncrementingClock::new(20_000));
    assert_eq!(
        reopened.current_identity().unwrap(),
        Some(first_identity.clone())
    );
    assert_eq!(first_identity.version(), 1);
    assert_eq!(first_identity.predecessor_version(), None);

    let (repository, _, _) = reopened.into_parts();
    repository.close().unwrap();
    let encrypted_bytes = fs::read(database_path).unwrap();
    assert!(!contains_bytes(&encrypted_bytes, PRIVATE_MARKER.as_bytes()));
    assert!(!contains_bytes(&encrypted_bytes, "岚".as_bytes()));
}

#[test]
fn identity_revision_and_self_bundle_reopen_as_one_immutable_chain() {
    let directory = tempdir().unwrap();
    let repository = repository_with_identity_bundle(directory.path());
    let revision = IdentityRevisionProposal::new(
        1,
        1,
        IdentityProfileChanges::new(
            None,
            Some("更直白，同时明确不确定性".to_owned()),
            None,
            None,
            None,
            None,
        ),
        "近期对话表明更直接的提醒更有帮助",
        vec![EvidenceCitation::new(
            EvidenceId::from_raw(7),
            "更直接地提醒我",
        )],
    );
    let response = RuntimeResponse::new("我会更直白，同时明确哪些只是我的判断。")
        .with_identity_revision(revision);
    let mut core = MemoryCore::new(
        repository,
        ScriptedRuntime::new([PersonTurnClassification::Question], [response]),
        IncrementingClock::new(20_000),
    );
    let context = core.freeze_working_context(&[]).unwrap();
    let outcome = core
        .run_counterpart_turn(
            SessionId::new("identity-revision"),
            "以后可以更直接地提醒我吗？",
            context,
        )
        .unwrap();
    assert_eq!(
        outcome
            .accepted_identity_revision()
            .unwrap()
            .identity_version(),
        2
    );
    let (repository, _, _) = core.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let history = repository.identity_history().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version(), 1);
    assert_eq!(history[1].predecessor_version(), Some(1));
    assert_eq!(
        history[1].profile().expression_traits(),
        "更直白，同时明确不确定性"
    );
    assert_eq!(
        history[1].evidence_refs(),
        [EvidenceId::from_raw(7), EvidenceId::from_raw(8)]
    );
    let restored = repository.current_identity_context().unwrap().unwrap();
    assert_eq!(restored.state().version(), 2);
    assert_eq!(restored.self_bundle_version(), 2);
    assert_eq!(restored.constitution_version(), 1);
    repository.close().unwrap();
}

#[test]
fn failed_identity_evidence_link_rolls_back_identity_and_self_bundle_together() {
    let directory = tempdir().unwrap();
    let mut repository = repository_with_identity_bundle(directory.path());
    let commit = IdentityRevisionCommit::new(
        1,
        1,
        1,
        IdentityStateSnapshot::restore(
            2,
            Some(1),
            IdentityProfileSnapshot::new(
                "岚",
                "更直接",
                "不把当前自述当作全部真相",
                "可追溯性高于迎合",
                "独立同行者",
                "帮助本人形成更准确且可解释的自我理解",
            ),
            "固定故障注入",
            vec![EvidenceId::from_raw(999_999)],
            Timestamp::from_millis(30_000),
        ),
    );

    assert!(repository.commit_identity_revision(commit).is_err());
    assert_eq!(repository.identity_history().unwrap().len(), 1);
    let current = repository.current_identity_context().unwrap().unwrap();
    assert_eq!(current.state().version(), 1);
    assert_eq!(current.self_bundle_version(), 1);
    repository.close().unwrap();
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
