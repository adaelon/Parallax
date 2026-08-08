use std::{fmt::Write as _, fs};

use eam_core::{
    ClaimOwner, EvidenceCitation, EvidenceId, IdentityEvolutionRepository, IdentityProfileChanges,
    IdentityProfileSnapshot, IdentityRevisionCommit, IdentityRevisionProposal,
    IdentityStateSnapshot, IncrementingClock, MemoryCore, MemoryRepository,
    PersonTurnClassification, RuntimeResponse, ScriptedRuntime, SessionId, Speaker, Timestamp,
};
use eam_identity::{
    CounterpartReadiness, CounterpartRepository, IdentityFormation, IdentityProfile,
    IdentityRepository, IdentityStateVersion, InitialIdentityProposal, IntroductionAnswer,
    ScriptedIdentityRuntime, SelfBundleRepository, SelfBundleState, SelfBundleVersion,
    SelfIntroductionCategory,
};
use eam_vault::{VaultKey, VaultRepository};
use hkdf::Hkdf;
use rusqlite::Connection;
use sha2::Sha256;
use tempfile::tempdir;

const VAULT_KEY_BYTES: [u8; 32] = [0x73; 32];
const PRIVATE_MARKER: &str = "S04-固定自述-不得出现在数据库字节中";
const PRIVATE_IDENTITY_MARKER: &str = "S07C-2-固定身份形成理由-不得出现在数据库字节中";
const KDF_SALT: &[u8] = b"evrything-about-me/v1/vault-subkeys";
const DATABASE_INFO: &[u8] = b"database";

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
        PRIVATE_IDENTITY_MARKER,
        (1..=6).map(EvidenceId::from_raw).collect(),
    )
}

fn initial_pair(
    identity_evidence_refs: Vec<EvidenceId>,
    bundle_identity_version: u64,
) -> (IdentityStateVersion, SelfBundleVersion) {
    let proposal = proposal();
    let formed_at = Timestamp::from_millis(12_000);
    let identity = IdentityStateVersion::restore(
        1,
        None,
        proposal.profile().clone(),
        proposal.change_reason(),
        identity_evidence_refs,
        formed_at,
    );
    let bundle = SelfBundleVersion::restore(
        1,
        None,
        SelfBundleState::new(
            1,
            bundle_identity_version,
            Vec::new(),
            Vec::new(),
            identity.profile().relationship_posture(),
            Vec::new(),
        )
        .unwrap(),
        None,
        formed_at,
    );
    (identity, bundle)
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
    formation.form_initial_counterpart().unwrap();
    let (repository, _, _) = formation.into_parts();
    repository
}

#[test]
fn reopens_the_same_first_identity_with_its_person_evidence_and_facts() {
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 26);
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
    assert!(!contains_bytes(
        &encrypted_bytes,
        PRIVATE_IDENTITY_MARKER.as_bytes()
    ));
}

#[test]
fn atomically_reopens_the_first_identity_and_self_bundle_as_ready() {
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let mut formation = IdentityFormation::new(
        repository,
        ScriptedIdentityRuntime::new([proposal()]),
        IncrementingClock::new(11_000),
    );

    assert_eq!(
        formation.counterpart_readiness().unwrap(),
        CounterpartReadiness::NeedsIntroduction
    );
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &introduction())
        .unwrap();
    assert_eq!(
        formation.counterpart_readiness().unwrap(),
        CounterpartReadiness::IntroductionRecorded
    );

    let ready = formation.form_initial_counterpart().unwrap();
    assert_eq!(
        ready,
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
    assert_eq!(bundle.state().identity_state_version(), identity.version());
    assert_eq!(
        bundle.state().relationship_state(),
        identity.profile().relationship_posture()
    );
    assert!(bundle.state().counterpart_experience_refs().is_empty());
    assert!(bundle.state().belief_refs().is_empty());
    assert!(bundle.state().pending_intentions().is_empty());
    assert_eq!(bundle.committed_at(), identity.formed_at());
    assert!(formation.form_initial_counterpart().is_err());
    assert_eq!(formation.runtime().seen_requests().len(), 1);

    let (repository, _, _) = formation.into_parts();
    repository.close().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    assert_eq!(
        repository.counterpart_readiness().unwrap(),
        CounterpartReadiness::Ready {
            identity_version: 1,
            self_bundle_version: 1,
        }
    );
    assert_eq!(repository.all_identity_states().unwrap().len(), 1);
    assert_eq!(repository.current_self_bundle().unwrap(), Some(bundle));
}

#[test]
fn rejects_a_mismatched_initial_pair_without_writing_either_version() {
    let directory = tempdir().unwrap();
    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let mut formation = IdentityFormation::new(
        repository,
        ScriptedIdentityRuntime::new([]),
        IncrementingClock::new(12_000),
    );
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &introduction())
        .unwrap();
    let (mut repository, _, _) = formation.into_parts();
    let (identity, bundle) = initial_pair((1..=6).map(EvidenceId::from_raw).collect(), 2);

    assert!(
        repository
            .commit_initial_counterpart(identity, bundle)
            .is_err()
    );
    assert_eq!(
        repository.counterpart_readiness().unwrap(),
        CounterpartReadiness::IntroductionRecorded
    );
    assert!(repository.current_identity_state().unwrap().is_none());
    assert!(repository.current_self_bundle().unwrap().is_none());
}

#[test]
fn injected_parent_and_child_failures_reopen_without_half_counterpart_state() {
    for (stage, trigger) in [
        (
            "identity parent",
            "CREATE TRIGGER injected_initial_counterpart_failure
             AFTER INSERT ON identity_state_versions
             BEGIN SELECT RAISE(ABORT, 'injected identity parent failure'); END;",
        ),
        (
            "identity evidence child",
            "CREATE TRIGGER injected_initial_counterpart_failure
             AFTER INSERT ON identity_state_evidence WHEN NEW.ordinal = 2
             BEGIN SELECT RAISE(ABORT, 'injected identity child failure'); END;",
        ),
        (
            "Self Bundle parent",
            "CREATE TRIGGER injected_initial_counterpart_failure
             AFTER INSERT ON self_bundle_versions
             BEGIN SELECT RAISE(ABORT, 'injected Self Bundle parent failure'); END;",
        ),
    ] {
        let directory = tempdir().unwrap();
        let repository = VaultRepository::open(directory.path(), key()).unwrap();
        let mut formation = IdentityFormation::new(
            repository,
            ScriptedIdentityRuntime::new([]),
            IncrementingClock::new(13_000),
        );
        formation
            .record_initial_self_introduction(&SessionId::new("onboarding"), &introduction())
            .unwrap();
        let (repository, _, _) = formation.into_parts();
        let database_path = repository.database_path().to_owned();
        repository.close().unwrap();
        install_failure_trigger(&database_path, trigger);

        let repository = VaultRepository::open(directory.path(), key()).unwrap();
        let mut formation = IdentityFormation::new(
            repository,
            ScriptedIdentityRuntime::new([proposal()]),
            IncrementingClock::new(14_000),
        );
        let error = formation
            .form_initial_counterpart()
            .expect_err("injected write failure must abort the complete pair");
        assert!(
            matches!(error, eam_identity::IdentityError::Repository(_)),
            "unexpected {stage} error: {error:?}"
        );
        let (repository, _, _) = formation.into_parts();
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), key()).unwrap();
        assert_eq!(
            repository.counterpart_readiness().unwrap(),
            CounterpartReadiness::IntroductionRecorded,
            "{stage} left a visible half state after reopen"
        );
        assert!(repository.current_identity_state().unwrap().is_none());
        assert!(repository.current_self_bundle().unwrap().is_none());
    }
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

fn install_failure_trigger(database_path: &std::path::Path, trigger: &str) {
    let connection = Connection::open(database_path).unwrap();
    key_sqlcipher_connection(&connection);
    connection.execute_batch(trigger).unwrap();
    connection.close().unwrap();
}

fn key_sqlcipher_connection(connection: &Connection) {
    let hkdf = Hkdf::<Sha256>::new(Some(KDF_SALT), &VAULT_KEY_BYTES);
    let mut database_key = [0_u8; 32];
    hkdf.expand(DATABASE_INFO, &mut database_key).unwrap();
    let mut pragma = String::from("PRAGMA key = \"x'");
    for byte in database_key {
        write!(&mut pragma, "{byte:02x}").unwrap();
    }
    pragma.push_str("'\";");
    connection.execute_batch(&pragma).unwrap();
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
