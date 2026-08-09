use std::{collections::VecDeque, sync::Arc};

use eam_core::{RuntimeError, RuntimeRequest};
use eam_identity::{IdentityRuntime, InitialIdentityRequest};

use super::*;

#[derive(Default)]
struct RuntimeObservations {
    identity_requests: usize,
    identity_evidence_ids: Vec<Vec<u64>>,
    person_fact_requests: usize,
    response_requests: usize,
}

struct RecordingDesktopRuntime {
    identity_results: VecDeque<Result<InitialIdentityProposal, RuntimeError>>,
    observations: Arc<Mutex<RuntimeObservations>>,
}

impl RecordingDesktopRuntime {
    fn new(
        identity_results: impl IntoIterator<Item = Result<InitialIdentityProposal, RuntimeError>>,
    ) -> (Self, Arc<Mutex<RuntimeObservations>>) {
        let observations = Arc::new(Mutex::new(RuntimeObservations::default()));
        (
            Self {
                identity_results: identity_results.into_iter().collect(),
                observations: Arc::clone(&observations),
            },
            observations,
        )
    }
}

impl IdentityRuntime for RecordingDesktopRuntime {
    fn form_initial_identity(
        &mut self,
        request: InitialIdentityRequest,
    ) -> Result<InitialIdentityProposal, RuntimeError> {
        let mut observations = self.observations.lock().unwrap();
        observations.identity_requests += 1;
        observations.identity_evidence_ids.push(
            request
                .introduction()
                .items()
                .iter()
                .map(|item| item.evidence_id().get())
                .collect(),
        );
        drop(observations);
        self.identity_results
            .pop_front()
            .unwrap_or_else(|| Err(RuntimeError::new("unexpected identity formation request")))
    }
}

impl CounterpartRuntime for RecordingDesktopRuntime {
    fn propose_person_facts(
        &mut self,
        _evidence: &ConversationEvidence,
    ) -> Result<eam_core::PersonFactProposalBatch, RuntimeError> {
        self.observations.lock().unwrap().person_fact_requests += 1;
        Ok(eam_core::PersonFactProposalBatch::empty())
    }

    fn respond(&mut self, _request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError> {
        self.observations.lock().unwrap().response_requests += 1;
        Ok(RuntimeResponse::new("合成第二自我回复"))
    }
}

fn initial_identity_proposal() -> InitialIdentityProposal {
    InitialIdentityProposal::new(
        IdentityProfile::new(
            "测试第二自我",
            "清晰、克制",
            "保留独立判断",
            "可信性优先",
            "共同回看的同行者",
            "帮助本人形成更准确的自我理解",
        ),
        "基于六类合成介绍形成",
        (1..=6).map(EvidenceId::from_raw).collect(),
    )
}

fn introduction_draft() -> InitialSelfIntroductionDraft {
    InitialSelfIntroductionDraft {
        basic_identity_and_address: "我是桌面创建测试中的本人。".to_owned(),
        current_life: "我正在验证第二自我创建流程。".to_owned(),
        important_people: "测试不包含真实人物资料。".to_owned(),
        long_term_goals: "保持可信、可追溯。".to_owned(),
        current_concerns: "防止未就绪对话旁路。".to_owned(),
        desired_reflection: "请保留独立判断。".to_owned(),
    }
}

fn managed_with_runtime(root: &Path, runtime: RecordingDesktopRuntime) -> ManagedHost {
    let mut repository = VaultRepository::open(root, VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let mut clock = SystemClock;
    let started_at = clock.now();
    let start = repository
        .begin_host_session(started_at, LaunchMode::Foreground)
        .unwrap();
    let recovery = repository
        .recover_capture_timeline(start.session().id(), started_at, None)
        .unwrap();
    let mut lifecycle = HostLifecycle::new();
    lifecycle.begin_recovery().unwrap();
    lifecycle
        .complete_recovery(start.session().id(), LaunchMode::Foreground)
        .unwrap();
    let runtime: AppRuntime = Box::new(runtime);
    ManagedHost {
        inner: Mutex::new(HostSlot::Ready(HostCore {
            core: MemoryCore::new(repository, runtime, SystemClock),
            lifecycle,
            capture: CaptureStateMachine::restore(&recovery),
            host_clock: SystemClock,
        })),
        vault_root: root.to_path_buf(),
        launch_mode: LaunchMode::Foreground,
        updater_configured: false,
    }
}

fn evidence_count(managed: &ManagedHost) -> usize {
    let slot = managed.lock();
    let HostSlot::Ready(host) = &*slot else {
        panic!("managed host should stay ready");
    };
    host.core.repository().all_evidence().unwrap().len()
}

#[test]
fn creation_commands_enforce_order_repetition_and_ready_send() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let (runtime, observations) = RecordingDesktopRuntime::new([Ok(initial_identity_proposal())]);
    let managed = managed_with_runtime(directory.path(), runtime);

    let initial = managed.get_counterpart_readiness().unwrap();
    assert_eq!(initial.state, "NEEDS_INTRODUCTION");
    assert_eq!(initial.identity_version, None);
    assert_eq!(initial.self_bundle_version, None);
    assert_eq!(initial.inconsistency_reason, None);
    assert_eq!(
        managed.form_initial_counterpart().unwrap_err(),
        "initial self introduction is required"
    );
    assert_eq!(observations.lock().unwrap().identity_requests, 0);

    let mut incomplete = introduction_draft();
    incomplete.current_life = "   ".to_owned();
    assert_eq!(
        managed
            .record_initial_self_introduction(incomplete)
            .unwrap_err(),
        "initial self introduction must contain six non-empty answers"
    );
    assert_eq!(evidence_count(&managed), 0);

    let recorded = managed
        .record_initial_self_introduction(introduction_draft())
        .unwrap();
    assert_eq!(recorded.state, "INTRODUCTION_RECORDED");
    assert_eq!(evidence_count(&managed), 6);
    assert_eq!(
        managed
            .record_initial_self_introduction(introduction_draft())
            .unwrap_err(),
        "initial self introduction is already recorded"
    );
    assert_eq!(evidence_count(&managed), 6);

    let ready = managed.form_initial_counterpart().unwrap();
    assert_eq!(ready.state, "READY");
    assert_eq!(ready.identity_version, Some(1));
    assert_eq!(ready.self_bundle_version, Some(1));
    assert_eq!(
        observations.lock().unwrap().identity_evidence_ids,
        [vec![1, 2, 3, 4, 5, 6]]
    );
    assert_eq!(
        managed.form_initial_counterpart().unwrap_err(),
        "counterpart is already created"
    );
    assert_eq!(observations.lock().unwrap().identity_requests, 1);

    let turn = managed
        .send_message("现在可以正式对话。".to_owned())
        .unwrap();
    assert_eq!(turn.counterpart.verbatim, "合成第二自我回复");
    let observations = observations.lock().unwrap();
    assert_eq!(observations.person_fact_requests, 1);
    assert_eq!(observations.response_requests, 1);
    drop(observations);
    managed.shutdown(ExitReason::Explicit).unwrap();
}

#[test]
fn readiness_view_uses_fixed_codes_for_every_inconsistency() {
    let cases = [
        (
            CounterpartInconsistencyReason::IntroductionMissing {
                identity_version: Some(1),
                self_bundle_version: Some(1),
            },
            "INTRODUCTION_MISSING",
        ),
        (
            CounterpartInconsistencyReason::IdentityMissing {
                self_bundle_version: 1,
                referenced_identity_version: 1,
            },
            "IDENTITY_MISSING",
        ),
        (
            CounterpartInconsistencyReason::SelfBundleMissing {
                identity_version: 1,
            },
            "SELF_BUNDLE_MISSING",
        ),
        (
            CounterpartInconsistencyReason::IdentityVersionMismatch {
                identity_version: 2,
                self_bundle_version: 3,
                referenced_identity_version: 1,
            },
            "IDENTITY_VERSION_MISMATCH",
        ),
    ];

    for (reason, expected) in cases {
        let view = CounterpartReadinessView::from(CounterpartReadiness::Inconsistent { reason });
        assert_eq!(view.state, "INCONSISTENT");
        assert_eq!(view.identity_version, None);
        assert_eq!(view.self_bundle_version, None);
        assert_eq!(view.inconsistency_reason, Some(expected));
    }
}

#[test]
fn conversation_view_projects_pre_identity_and_bound_reply_attribution() {
    let person = ConversationEvidence::restore(
        EvidenceId::from_raw(1),
        SessionId::new("s07c-6-view"),
        Speaker::Person,
        "本人原话".to_owned(),
        Timestamp::from_millis(1_000),
    );
    let legacy = ConversationEvidence::restore(
        EvidenceId::from_raw(2),
        SessionId::new("s07c-6-view"),
        Speaker::Counterpart,
        "创建前回复".to_owned(),
        Timestamp::from_millis(2_000),
    );
    let bound = ConversationEvidence::restore_counterpart(
        EvidenceId::from_raw(3),
        SessionId::new("s07c-6-view"),
        "正式回复".to_owned(),
        Timestamp::from_millis(3_000),
        CounterpartReplyAttribution::IdentityBound(7),
    );

    let person_view = ConversationTurnView::from(&person);
    assert_eq!(person_view.counterpart_reply_attribution, None);
    assert_eq!(person_view.counterpart_identity_version, None);
    let legacy_view = ConversationTurnView::from(&legacy);
    assert_eq!(
        legacy_view.counterpart_reply_attribution,
        Some("PRE_IDENTITY_UNBOUND")
    );
    assert_eq!(legacy_view.counterpart_identity_version, None);
    let bound_view = ConversationTurnView::from(&bound);
    assert_eq!(
        bound_view.counterpart_reply_attribution,
        Some("IDENTITY_BOUND")
    );
    assert_eq!(bound_view.counterpart_identity_version, Some(7));
}

#[test]
fn every_runtime_failure_category_is_sanitized() {
    let provider_secret = "provider-secret-response-body";
    let cases = [
        (
            RuntimeError::timeout(provider_secret),
            "counterpart formation runtime timed out",
        ),
        (
            RuntimeError::unavailable(provider_secret),
            "counterpart formation runtime is unavailable",
        ),
        (
            RuntimeError::invalid_response(provider_secret),
            "counterpart formation returned an invalid strict response",
        ),
        (
            RuntimeError::new(provider_secret),
            "counterpart formation runtime failed",
        ),
    ];

    for (error, expected) in cases {
        let sanitized = sanitized_identity_error(IdentityError::Runtime(error));
        assert_eq!(sanitized, expected);
        assert!(!sanitized.contains(provider_secret));
    }
}

#[test]
fn formation_failure_is_sanitized_and_retry_preserves_the_introduction() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let provider_secret = "provider-secret-response-body";
    let (runtime, observations) = RecordingDesktopRuntime::new([
        Err(RuntimeError::unavailable(provider_secret)),
        Ok(initial_identity_proposal()),
    ]);
    let managed = managed_with_runtime(directory.path(), runtime);
    managed
        .record_initial_self_introduction(introduction_draft())
        .unwrap();

    let error = managed.form_initial_counterpart().unwrap_err();
    assert_eq!(error, "counterpart formation runtime is unavailable");
    assert!(!error.contains(provider_secret));
    assert_eq!(
        managed.get_counterpart_readiness().unwrap().state,
        "INTRODUCTION_RECORDED"
    );
    assert_eq!(evidence_count(&managed), 6);

    assert_eq!(managed.form_initial_counterpart().unwrap().state, "READY");
    assert_eq!(observations.lock().unwrap().identity_requests, 2);
    assert_eq!(evidence_count(&managed), 6);
    managed.shutdown(ExitReason::Explicit).unwrap();
}

#[test]
fn readiness_and_creation_resume_across_host_restarts() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let (first_runtime, _) = RecordingDesktopRuntime::new([]);
    let first = managed_with_runtime(directory.path(), first_runtime);
    first
        .record_initial_self_introduction(introduction_draft())
        .unwrap();
    first.shutdown(ExitReason::Explicit).unwrap();

    let (second_runtime, observations) =
        RecordingDesktopRuntime::new([Ok(initial_identity_proposal())]);
    let second = managed_with_runtime(directory.path(), second_runtime);
    assert_eq!(
        second.get_counterpart_readiness().unwrap().state,
        "INTRODUCTION_RECORDED"
    );
    assert_eq!(second.form_initial_counterpart().unwrap().state, "READY");
    assert_eq!(observations.lock().unwrap().identity_requests, 1);
    second.shutdown(ExitReason::Explicit).unwrap();

    let (third_runtime, observations) = RecordingDesktopRuntime::new([]);
    let third = managed_with_runtime(directory.path(), third_runtime);
    let ready = third.get_counterpart_readiness().unwrap();
    assert_eq!(ready.state, "READY");
    assert_eq!(ready.identity_version, Some(1));
    assert_eq!(ready.self_bundle_version, Some(1));
    assert_eq!(observations.lock().unwrap().identity_requests, 0);
    third.shutdown(ExitReason::Explicit).unwrap();
}

#[test]
fn inconsistent_state_blocks_creation_and_host_send_without_runtime_calls() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let repository =
        VaultRepository::open(directory.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
    let mut formation = IdentityFormation::new(
        repository,
        ScriptedIdentityRuntime::new([initial_identity_proposal()]),
        IncrementingClock::new(1_000),
    );
    formation
        .record_initial_self_introduction(
            &SessionId::new("inconsistent-desktop-onboarding"),
            &introduction_draft().into_answers(),
        )
        .unwrap();
    formation.form_initial_identity().unwrap();
    let (repository, _, _) = formation.into_parts();
    repository.close().unwrap();

    let (runtime, observations) = RecordingDesktopRuntime::new([]);
    let managed = managed_with_runtime(directory.path(), runtime);
    let inconsistent = managed.get_counterpart_readiness().unwrap();
    assert_eq!(inconsistent.state, "INCONSISTENT");
    assert_eq!(
        inconsistent.inconsistency_reason,
        Some("SELF_BUNDLE_MISSING")
    );
    assert_eq!(
        managed
            .record_initial_self_introduction(introduction_draft())
            .unwrap_err(),
        "counterpart state is inconsistent"
    );
    assert_eq!(
        managed.form_initial_counterpart().unwrap_err(),
        "counterpart state is inconsistent"
    );
    assert_eq!(
        managed
            .send_message("不得越过宿主门禁。".to_owned())
            .unwrap_err(),
        "counterpart is not ready for formal conversation"
    );
    let observations = observations.lock().unwrap();
    assert_eq!(observations.identity_requests, 0);
    assert_eq!(observations.person_fact_requests, 0);
    assert_eq!(observations.response_requests, 0);
    drop(observations);
    assert_eq!(evidence_count(&managed), 6);
    managed.shutdown(ExitReason::Explicit).unwrap();
}
