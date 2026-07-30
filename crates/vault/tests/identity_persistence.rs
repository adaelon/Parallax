use std::fs;

use eam_core::{ClaimOwner, EvidenceId, IncrementingClock, MemoryRepository, SessionId, Speaker};
use eam_identity::{
    IdentityFormation, IdentityProfile, InitialIdentityProposal, IntroductionAnswer,
    ScriptedIdentityRuntime, SelfIntroductionCategory,
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

#[test]
fn reopens_the_same_first_identity_with_its_person_evidence_and_facts() {
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 10);
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

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
