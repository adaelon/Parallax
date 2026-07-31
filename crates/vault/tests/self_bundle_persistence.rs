use std::{fs, path::Path};

use eam_core::{
    ApplicableTime, Claim, ClaimOwner, EvidenceCitation, EvidenceId, IncrementingClock,
    MemoryRepository, RuntimeError, SessionId, Timestamp, Uncertainty,
};
use eam_identity::{
    IdentityFormation, IdentityProfile, InitialIdentityProposal, IntroductionAnswer,
    PresenceCoordinator, PresenceError, PresenceState, ScriptedIdentityRuntime, SelfBundleState,
    SelfIntroductionCategory, WakeTrigger, WakeWork,
};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

const VAULT_KEY_BYTES: [u8; 32] = [0x51; 32];
const PRIVATE_BUNDLE_MARKER: &str = "S05-私密自我包状态-不得出现在数据库字节中";

fn key() -> VaultKey {
    VaultKey::new(VAULT_KEY_BYTES)
}

fn introduction() -> Vec<IntroductionAnswer> {
    SelfIntroductionCategory::ALL
        .into_iter()
        .enumerate()
        .map(|(index, category)| IntroductionAnswer::new(category, format!("自述类别 {index}")))
        .collect()
}

fn proposal() -> InitialIdentityProposal {
    InitialIdentityProposal::new(
        IdentityProfile::new(
            "岚",
            "温和、直接",
            "保留独立判断",
            "可追溯性优先",
            "共同回看",
            "帮助本人形成可解释的自我理解",
        ),
        "基于六类自述形成首个身份版本",
        (1..=6).map(EvidenceId::from_raw).collect(),
    )
}

fn repository_with_identity(vault_root: &Path) -> (VaultRepository, eam_core::ClaimId) {
    let repository = VaultRepository::open(vault_root, key()).unwrap();
    let runtime = ScriptedIdentityRuntime::new([proposal()]);
    let mut formation = IdentityFormation::new(repository, runtime, IncrementingClock::new(1_000));
    formation
        .record_initial_self_introduction(&SessionId::new("onboarding"), &introduction())
        .unwrap();
    formation.form_initial_identity().unwrap();

    let (mut repository, _, _) = formation.into_parts();
    let source = repository
        .evidence(EvidenceId::from_raw(1))
        .unwrap()
        .unwrap();
    let claim_id = repository.next_claim_id();
    repository
        .append_claim(Claim::restore(
            claim_id,
            ClaimOwner::Counterpart,
            "我会保持对首个自我介绍的独立理解".to_owned(),
            vec![EvidenceCitation::new(source.id(), source.verbatim())],
            Some(Uncertainty::Low),
            ApplicableTime::At(Timestamp::from_millis(1_100)),
            Timestamp::from_millis(1_100),
        ))
        .unwrap();
    (repository, claim_id)
}

#[derive(Clone, Copy)]
enum WorkMode {
    PersistValid(eam_core::ClaimId),
    InjectMissingBelief,
}

struct PersistenceWakeWork {
    mode: WorkMode,
}

impl PersistenceWakeWork {
    fn update(
        &self,
        phase: PresenceState,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        let mut experiences = state.counterpart_experience_refs().to_vec();
        let mut beliefs = state.belief_refs().to_vec();
        let mut intentions = state.pending_intentions().to_vec();
        let mut relationship = state.relationship_state().to_owned();
        match phase {
            PresenceState::Observe => experiences.push(PRIVATE_BUNDLE_MARKER.to_owned()),
            PresenceState::Think => {
                beliefs.push(match self.mode {
                    WorkMode::PersistValid(claim_id) => claim_id,
                    WorkMode::InjectMissingBelief => eam_core::ClaimId::from_raw(999_999),
                });
                intentions.push("下次继续讨论工作与生活的边界".to_owned());
            }
            PresenceState::Respond => {
                "持续关系：已回应首次唤醒".clone_into(&mut relationship);
            }
            PresenceState::Sleeping | PresenceState::LoadSelf | PresenceState::WriteAgentMemory => {
                unreachable!("test uses bounded work phases")
            }
        }
        SelfBundleState::new(
            state.constitution_version(),
            state.identity_state_version(),
            experiences,
            beliefs,
            relationship,
            intentions,
        )
        .map_err(|error| RuntimeError::new(format!("invalid test state: {error}")))
    }
}

impl WakeWork for PersistenceWakeWork {
    fn observe(
        &mut self,
        _trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        self.update(PresenceState::Observe, state)
    }

    fn think(
        &mut self,
        _trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        self.update(PresenceState::Think, state)
    }

    fn respond(
        &mut self,
        _trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        self.update(PresenceState::Respond, state)
    }
}

fn initial_bundle() -> SelfBundleState {
    SelfBundleState::new(1, 1, Vec::new(), Vec::new(), "forming", Vec::new()).unwrap()
}

#[test]
fn reopens_one_complete_self_bundle_with_identity_relationship_beliefs_and_intentions() {
    let directory = tempdir().unwrap();
    let (repository, belief_ref) = repository_with_identity(directory.path());
    assert_eq!(repository.schema_version().unwrap(), 20);
    let work = PersistenceWakeWork {
        mode: WorkMode::PersistValid(belief_ref),
    };
    let mut presence = PresenceCoordinator::new(repository, work, IncrementingClock::new(2_000));
    presence.initialize_self_bundle(initial_bundle()).unwrap();

    let outcome = presence.wake(WakeTrigger::ConversationStarted).unwrap();
    let expected = outcome.bundle().clone();
    assert_eq!(expected.version(), 2);
    assert_eq!(expected.state().belief_refs(), [belief_ref]);
    assert_eq!(
        expected.state().counterpart_experience_refs(),
        [PRIVATE_BUNDLE_MARKER]
    );
    assert_eq!(
        expected.state().pending_intentions(),
        ["下次继续讨论工作与生活的边界"]
    );
    assert_eq!(
        expected.state().relationship_state(),
        "持续关系：已回应首次唤醒"
    );

    let (repository, _, _) = presence.into_parts();
    let database_path = repository.database_path().to_owned();
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let reopened = PresenceCoordinator::new(
        repository,
        PersistenceWakeWork {
            mode: WorkMode::PersistValid(belief_ref),
        },
        IncrementingClock::new(3_000),
    );
    assert_eq!(reopened.current_self_bundle().unwrap(), Some(expected));
    let (repository, _, _) = reopened.into_parts();
    repository.close().unwrap();

    let encrypted_bytes = fs::read(database_path).unwrap();
    assert!(!contains_bytes(
        &encrypted_bytes,
        PRIVATE_BUNDLE_MARKER.as_bytes()
    ));
}

#[test]
fn failed_child_write_rolls_back_the_entire_self_bundle_version_across_reopen() {
    let directory = tempdir().unwrap();
    let (repository, _) = repository_with_identity(directory.path());
    let work = PersistenceWakeWork {
        mode: WorkMode::InjectMissingBelief,
    };
    let mut presence = PresenceCoordinator::new(repository, work, IncrementingClock::new(4_000));
    let before = presence.initialize_self_bundle(initial_bundle()).unwrap();

    let error = presence
        .wake(WakeTrigger::EvidenceChanged)
        .expect_err("missing belief FK must abort the complete bundle transaction");

    assert!(matches!(error, PresenceError::Repository(_)));
    assert_eq!(
        presence.current_self_bundle().unwrap(),
        Some(before.clone())
    );
    let (repository, _, _) = presence.into_parts();
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), key()).unwrap();
    let reopened = PresenceCoordinator::new(
        repository,
        PersistenceWakeWork {
            mode: WorkMode::InjectMissingBelief,
        },
        IncrementingClock::new(5_000),
    );
    assert_eq!(reopened.current_self_bundle().unwrap(), Some(before));
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
