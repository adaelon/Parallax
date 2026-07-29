use eam_core::{EvidenceId, IncrementingClock, RuntimeError, Timestamp};
use eam_identity::{
    IdentityProfile, IdentityRepository, IdentityStateVersion, InMemoryIdentityRepository,
    PresenceCoordinator, PresenceError, PresenceState, SelfBundleState, WakeExit, WakeTrigger,
    WakeWork,
};

#[derive(Clone, Copy)]
enum Mutation {
    Normal,
    ChangeConstitution,
}

struct DeterministicWakeWork {
    fail_at: Option<PresenceState>,
    mutation: Mutation,
}

impl DeterministicWakeWork {
    const fn completes() -> Self {
        Self {
            fail_at: None,
            mutation: Mutation::Normal,
        }
    }

    const fn fails_at(phase: PresenceState) -> Self {
        Self {
            fail_at: Some(phase),
            mutation: Mutation::Normal,
        }
    }

    fn run_phase(
        &self,
        phase: PresenceState,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        if self.fail_at == Some(phase) {
            return Err(RuntimeError::new(format!("{phase:?} failed")));
        }

        let mut experiences = state.counterpart_experience_refs().to_vec();
        let mut intentions = state.pending_intentions().to_vec();
        let mut relationship = state.relationship_state().to_owned();
        match phase {
            PresenceState::Observe => experiences.push("experience:conversation-1".to_owned()),
            PresenceState::Think => intentions.push("continue:conversation-1".to_owned()),
            PresenceState::Respond => "engaged".clone_into(&mut relationship),
            PresenceState::Sleeping | PresenceState::LoadSelf | PresenceState::WriteAgentMemory => {
                unreachable!("test work uses bounded phases")
            }
        }

        let constitution_version = match (phase, self.mutation) {
            (PresenceState::Think, Mutation::ChangeConstitution) => {
                state.constitution_version() + 1
            }
            _ => state.constitution_version(),
        };
        SelfBundleState::new(
            constitution_version,
            state.identity_state_version(),
            experiences,
            state.belief_refs().to_vec(),
            relationship,
            intentions,
        )
        .map_err(|error| RuntimeError::new(format!("invalid scripted state: {error:?}")))
    }
}

impl WakeWork for DeterministicWakeWork {
    fn observe(
        &mut self,
        _trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        self.run_phase(PresenceState::Observe, state)
    }

    fn think(
        &mut self,
        _trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        self.run_phase(PresenceState::Think, state)
    }

    fn respond(
        &mut self,
        _trigger: WakeTrigger,
        state: &SelfBundleState,
    ) -> Result<SelfBundleState, RuntimeError> {
        self.run_phase(PresenceState::Respond, state)
    }
}

fn identity() -> IdentityStateVersion {
    IdentityStateVersion::restore(
        1,
        None,
        IdentityProfile::new(
            "岚",
            "温和、直接",
            "保留独立判断",
            "可追溯性优先",
            "共同回看",
            "帮助本人形成可解释的自我理解",
        ),
        "首个身份版本",
        vec![EvidenceId::from_raw(1)],
        Timestamp::from_millis(10),
    )
}

fn initial_bundle(identity_version: u64) -> SelfBundleState {
    SelfBundleState::new(
        1,
        identity_version,
        Vec::new(),
        Vec::new(),
        "forming",
        Vec::new(),
    )
    .unwrap()
}

fn coordinator(
    work: DeterministicWakeWork,
) -> PresenceCoordinator<InMemoryIdentityRepository, DeterministicWakeWork, IncrementingClock> {
    let mut repository = InMemoryIdentityRepository::new();
    repository.append_identity_state(identity()).unwrap();
    PresenceCoordinator::new(repository, work, IncrementingClock::new(100))
}

#[test]
fn follows_every_success_transition_and_commits_the_complete_next_bundle() {
    let mut presence = coordinator(DeterministicWakeWork::completes());
    let initial = presence.initialize_self_bundle(initial_bundle(1)).unwrap();
    assert_eq!(initial.version(), 1);

    let outcome = presence.wake(WakeTrigger::ConversationStarted).unwrap();

    assert_eq!(
        outcome.trace(),
        [
            PresenceState::Sleeping,
            PresenceState::LoadSelf,
            PresenceState::Observe,
            PresenceState::Think,
            PresenceState::Respond,
            PresenceState::WriteAgentMemory,
            PresenceState::Sleeping,
        ]
    );
    assert!(outcome.interruption().is_none());
    assert_eq!(outcome.bundle().version(), 2);
    assert_eq!(outcome.bundle().predecessor_version(), Some(1));
    assert_eq!(
        outcome.bundle().wake_commit().unwrap().exit(),
        WakeExit::Completed
    );
    assert_eq!(
        outcome.bundle().state().counterpart_experience_refs(),
        ["experience:conversation-1"]
    );
    assert_eq!(
        outcome.bundle().state().pending_intentions(),
        ["continue:conversation-1"]
    );
    assert_eq!(outcome.bundle().state().relationship_state(), "engaged");
    assert_eq!(
        presence.current_self_bundle().unwrap(),
        Some(outcome.bundle().clone())
    );
}

#[test]
fn every_declared_trigger_enters_the_same_bounded_wake_machine() {
    for trigger in [
        WakeTrigger::ConversationStarted,
        WakeTrigger::EvidenceChanged,
        WakeTrigger::ScheduledReflection,
        WakeTrigger::ImportantChange,
    ] {
        let mut presence = coordinator(DeterministicWakeWork::completes());
        presence.initialize_self_bundle(initial_bundle(1)).unwrap();

        let outcome = presence.wake(trigger).unwrap();

        assert_eq!(outcome.bundle().wake_commit().unwrap().trigger(), trigger);
        assert_eq!(outcome.trace().first(), Some(&PresenceState::Sleeping));
        assert_eq!(outcome.trace().last(), Some(&PresenceState::Sleeping));
    }
}

#[test]
fn every_work_exit_commits_the_last_valid_state_before_sleeping() {
    for phase in [
        PresenceState::Observe,
        PresenceState::Think,
        PresenceState::Respond,
    ] {
        let mut presence = coordinator(DeterministicWakeWork::fails_at(phase));
        presence.initialize_self_bundle(initial_bundle(1)).unwrap();

        let outcome = presence.wake(WakeTrigger::EvidenceChanged).unwrap();

        let interruption = outcome
            .interruption()
            .expect("work failure must be visible");
        assert_eq!(interruption.phase(), phase);
        assert_eq!(
            outcome.bundle().wake_commit().unwrap().exit(),
            WakeExit::InterruptedAt(phase)
        );
        assert_eq!(outcome.trace().last(), Some(&PresenceState::Sleeping));
        assert_eq!(
            outcome.trace()[outcome.trace().len() - 2],
            PresenceState::WriteAgentMemory
        );
        assert_eq!(outcome.bundle().version(), 2);
        assert_eq!(
            presence.current_self_bundle().unwrap(),
            Some(outcome.bundle().clone())
        );
    }
}

#[test]
fn rejects_constitution_mutation_but_still_commits_a_safe_sleep_version() {
    let work = DeterministicWakeWork {
        fail_at: None,
        mutation: Mutation::ChangeConstitution,
    };
    let mut presence = coordinator(work);
    presence.initialize_self_bundle(initial_bundle(1)).unwrap();

    let outcome = presence.wake(WakeTrigger::ImportantChange).unwrap();

    assert_eq!(
        outcome.interruption().unwrap().phase(),
        PresenceState::Think
    );
    assert_eq!(outcome.bundle().state().constitution_version(), 1);
    assert_eq!(
        outcome.bundle().state().counterpart_experience_refs(),
        ["experience:conversation-1"]
    );
    assert!(outcome.bundle().state().pending_intentions().is_empty());
    assert_eq!(outcome.trace().last(), Some(&PresenceState::Sleeping));
}

#[test]
fn requires_the_current_identity_before_initializing_a_self_bundle() {
    let mut without_identity = PresenceCoordinator::new(
        InMemoryIdentityRepository::new(),
        DeterministicWakeWork::completes(),
        IncrementingClock::new(200),
    );
    assert_eq!(
        without_identity
            .initialize_self_bundle(initial_bundle(1))
            .unwrap_err(),
        PresenceError::IdentityNotFormed
    );

    let mut wrong_version = coordinator(DeterministicWakeWork::completes());
    assert_eq!(
        wrong_version
            .initialize_self_bundle(initial_bundle(2))
            .unwrap_err(),
        PresenceError::IdentityStateVersionMismatch {
            expected: 1,
            proposed: 2,
        }
    );
}
