use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use eam_capture_browser::{BrowserCaptureReceipt, BrowserCaptureRepository, BrowserSubmission};
use eam_capture_windows::{
    ActivityTimelineRepository, CaptureCheckpoint, CaptureGapReason, CaptureMode, CaptureSpan,
    CaptureSpanKind, CaptureStateMachine, DEFAULT_IDLE_THRESHOLD, NativeCaptureSample,
    ShutdownReason, sample_foreground_activity,
};
use eam_core::{
    AgreementWithdrawalActor, ClaimId, Clock, ConversationEvidence, CounterpartReplyAttribution,
    CounterpartRuntime, EvidenceCitation, EvidenceId, IdentityEvolutionRepository,
    IdentityStateSnapshot, MemoryCore, MemoryRepository, ReflectionDecision, ReflectionImportance,
    ReflectionInvitation, ReflectionInvitationBasis, ReflectionInvitationId,
    ReflectionInvitationRepository, ReflectionInvitationState, ReflectionOpportunity,
    RuntimeErrorKind, SessionId, SharedAgreementCandidateStatus, SharedAgreementDecision,
    SharedAgreementResolution, SharedAgreementRevision, SharedExperienceKind,
    SharedExperienceRepository, Speaker, SystemClock, Timestamp, WorkingContext,
    agreement_is_active_at,
};
use eam_desktop_host::{ExitReason, HostLifecycle, HostLifecycleRepository, HostState, LaunchMode};
use eam_identity::{
    CounterpartInconsistencyReason, CounterpartReadiness, CounterpartRepository, IdentityError,
    IdentityFormation, IdentityRuntime, IntroductionAnswer, SelfIntroductionCategory,
};
use eam_ingestion::{
    ArchiveRepository, ArchiveStatus, ImportOutcome, ImportPolicy, RejectReason, UnparsedReason,
    ingest_inbox_file,
};
use eam_retrieval::{
    RetrievalQuery, RetrievalRepository, TokenBudget, freeze_working_context as freeze_retrieval,
    project_active_relational_constraints, search_terms,
};
use eam_runtime_gateway::{HttpResponsesTransport, OpenAiResponsesRuntime, RuntimeTarget};
use eam_vault::{
    PreparedVault, RuntimeProfile as VaultRuntimeProfile,
    RuntimeProfileKeyAction as VaultRuntimeProfileKeyAction,
    RuntimeProfileView as VaultRuntimeProfileView, VaultKey, VaultKeyStore, VaultRepository,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

const RUNTIME_TIMEOUT: Duration = Duration::from_secs(45);
const RUNTIME_PROFILE_TEST_INPUT: &str =
    "Synthetic runtime profile test: is the strict person-fact proposal contract available?";
const CONTINUOUS_SESSION_ID: &str = "continuous-conversation";
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_CONTEXT_TURNS: usize = 32;
const MAX_CONTEXT_BYTES: usize = 64 * 1024;
const VAULT_SETUP_INCOMPLETE: &str = "vault setup is not complete";
const INITIAL_SELF_INTRODUCTION_SESSION_ID: &str = "initial-self-introduction";

trait AppRuntimeContract: CounterpartRuntime + IdentityRuntime {}

impl<T> AppRuntimeContract for T where T: CounterpartRuntime + IdentityRuntime {}

type AppRuntime = Box<dyn AppRuntimeContract + Send>;
type AppCore = MemoryCore<VaultRepository, AppRuntime, SystemClock>;

pub struct ManagedHost {
    inner: Mutex<HostSlot>,
    vault_root: PathBuf,
    launch_mode: LaunchMode,
    updater_configured: bool,
}

#[allow(clippy::large_enum_variant)] // One mutex-owned Core is resident; boxing adds no useful boundary.
enum HostSlot {
    NeedsInitialization,
    AwaitingRecoveryConfirmation(PreparedVault),
    Ready(HostCore),
    Locked(String),
    FailedClosed(String),
    Closed,
}

struct HostCore {
    core: AppCore,
    lifecycle: HostLifecycle,
    capture: CaptureStateMachine,
    host_clock: SystemClock,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatusView {
    state: &'static str,
    vault_ready: bool,
    updater_configured: bool,
    detail: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryKeyView {
    recovery_key: String,
}

/// One command-scoped candidate for the singleton runtime profile.
///
/// This type intentionally omits `Debug`, `Clone`, and serialization so a
/// replacement key cannot be echoed by routine host diagnostics, and its
/// destructor zeroizes that replacement value.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeProfileDraft {
    base_url: String,
    model: String,
    api_key_change: RuntimeProfileApiKeyChange,
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    content = "value",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
enum RuntimeProfileApiKeyChange {
    Keep,
    Replace(String),
    Clear,
}

impl Drop for RuntimeProfileDraft {
    fn drop(&mut self) {
        if let RuntimeProfileApiKeyChange::Replace(value) = &mut self.api_key_change {
            value.zeroize();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileView {
    base_url: String,
    model: String,
    api_key_configured: bool,
    api_key_last_four: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileTestView {
    succeeded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatusView {
    state: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTimelineEntryView {
    id: u64,
    kind: &'static str,
    application: Option<String>,
    window_title: Option<String>,
    idle: Option<bool>,
    gap_reason: Option<&'static str>,
    started_at_millis: i64,
    observed_until_millis: i64,
    ended_at_millis: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnView {
    id: u64,
    speaker: &'static str,
    verbatim: String,
    recorded_at_millis: i64,
    counterpart_reply_attribution: Option<&'static str>,
    counterpart_identity_version: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnResult {
    person: ConversationTurnView,
    counterpart: ConversationTurnView,
    ceremonies: Vec<SharedExperienceCeremonyView>,
    reflection_invitations: Vec<ReflectionInvitationView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterpartReadinessView {
    state: &'static str,
    identity_version: Option<u64>,
    self_bundle_version: Option<u64>,
    inconsistency_reason: Option<&'static str>,
}

/// The fixed six-category input accepted by the initial-introduction command.
///
/// The draft is intentionally neither serializable nor debuggable: it flows
/// from the `WebView` into the trusted host but is never echoed back.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialSelfIntroductionDraft {
    basic_identity_and_address: String,
    current_life: String,
    important_people: String,
    long_term_goals: String,
    current_concerns: String,
    desired_reflection: String,
}

impl InitialSelfIntroductionDraft {
    fn into_answers(self) -> [IntroductionAnswer; 6] {
        [
            IntroductionAnswer::new(
                SelfIntroductionCategory::BasicIdentityAndAddress,
                self.basic_identity_and_address,
            ),
            IntroductionAnswer::new(SelfIntroductionCategory::CurrentLife, self.current_life),
            IntroductionAnswer::new(
                SelfIntroductionCategory::ImportantPeople,
                self.important_people,
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::LongTermGoals,
                self.long_term_goals,
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::CurrentConcerns,
                self.current_concerns,
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::DesiredReflection,
                self.desired_reflection,
            ),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionInvitationEvidenceView {
    evidence_id: u64,
    speaker: &'static str,
    quote: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionInvitationView {
    id: u64,
    topic_key: String,
    observation: String,
    why_now: String,
    importance: &'static str,
    basis: &'static str,
    defer_count: u32,
    show_mute_prompt: bool,
    evidence: Vec<ReflectionInvitationEvidenceView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectionInvitationDecisionView {
    invitation_id: u64,
    state: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedExperienceCeremonyEvidenceView {
    evidence_id: u64,
    speaker: &'static str,
    quote: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedExperienceCeremonyView {
    target_id: u64,
    target_kind: &'static str,
    experience_kind: &'static str,
    admission: &'static str,
    statement: String,
    candidate_version: Option<u64>,
    scope: Option<String>,
    effective_from_millis: Option<i64>,
    effective_until_millis: Option<i64>,
    end_condition: Option<String>,
    agreement_claim_id: Option<u64>,
    departure_reason: Option<String>,
    withdrawal_actor: Option<&'static str>,
    superseded_agreements: Vec<SupersededAgreementView>,
    evidence: Vec<SharedExperienceCeremonyEvidenceView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSharedAgreementView {
    claim_id: u64,
    statement: String,
    scope: String,
    effective_from_millis: i64,
    effective_until_millis: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityStateView {
    version: u64,
    predecessor_version: Option<u64>,
    name: String,
    expression_traits: String,
    viewpoints: String,
    value_priorities: String,
    relationship_posture: String,
    own_goals: String,
    change_reason: String,
    evidence_ids: Vec<u64>,
    formed_at_millis: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupersededAgreementView {
    claim_id: u64,
    statement: String,
    scope: String,
    effective_from_millis: i64,
    effective_until_millis: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedAgreementResolutionView {
    candidate_id: u64,
    status: &'static str,
    claim_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedAgreementRevisionView {
    candidate_id: u64,
    version: u64,
    status: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportContextFileView {
    status: &'static str,
    archive_id: Option<u64>,
    reason: Option<&'static str>,
    bytes: Option<u64>,
    object_reused: bool,
    source_version_reused: bool,
}

impl ManagedHost {
    #[must_use]
    pub fn open(vault_root: PathBuf, launch_mode: LaunchMode, updater_configured: bool) -> Self {
        let slot = match VaultKeyStore::is_initialized(&vault_root) {
            Ok(false) => HostSlot::NeedsInitialization,
            Ok(true) => HostCore::open(&vault_root, launch_mode)
                .map_or_else(HostSlot::Locked, HostSlot::Ready),
            Err(error) => HostSlot::Locked(error.to_string()),
        };
        Self {
            inner: Mutex::new(slot),
            vault_root,
            launch_mode,
            updater_configured,
        }
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(&*self.lock(), HostSlot::Ready(_))
    }

    pub fn initialize_vault(&self) -> Result<RecoveryKeyView, String> {
        let mut slot = self.lock();
        if !matches!(&*slot, HostSlot::NeedsInitialization) {
            return Err(match &*slot {
                HostSlot::AwaitingRecoveryConfirmation(_) => {
                    "vault initialization is already awaiting recovery-key confirmation".to_owned()
                }
                HostSlot::Ready(_) => "vault is already initialized".to_owned(),
                HostSlot::Locked(detail) => format!("vault is locked: {detail}"),
                HostSlot::FailedClosed(detail) => format!("Core is closed: {detail}"),
                HostSlot::Closed => "desktop host is already stopped".to_owned(),
                HostSlot::NeedsInitialization => unreachable!(),
            });
        }

        let prepared = VaultKeyStore::prepare().map_err(|error| error.to_string())?;
        let recovery_key = prepared.recovery_key().expose_secret().to_owned();
        *slot = HostSlot::AwaitingRecoveryConfirmation(prepared);
        Ok(RecoveryKeyView { recovery_key })
    }

    pub fn confirm_recovery_key_saved(&self, confirmed: bool) -> Result<HostStatusView, String> {
        if !confirmed {
            return Err("recovery-key confirmation is required".to_owned());
        }

        let mut slot = self.lock();
        let pending = match std::mem::replace(&mut *slot, HostSlot::Closed) {
            HostSlot::AwaitingRecoveryConfirmation(pending) => pending,
            other => {
                let message = match &other {
                    HostSlot::NeedsInitialization => {
                        "vault initialization has not started".to_owned()
                    }
                    HostSlot::Ready(_) => "vault is already initialized".to_owned(),
                    HostSlot::Locked(detail) => format!("vault is locked: {detail}"),
                    HostSlot::FailedClosed(detail) => format!("Core is closed: {detail}"),
                    HostSlot::Closed => "desktop host is already stopped".to_owned(),
                    HostSlot::AwaitingRecoveryConfirmation(_) => unreachable!(),
                };
                *slot = other;
                return Err(message);
            }
        };

        if let Err(error) = pending.commit(&self.vault_root) {
            *slot = HostSlot::AwaitingRecoveryConfirmation(pending);
            return Err(error.to_string());
        }
        let (vault_key, recovery_key) = pending.into_parts();
        drop(recovery_key);
        match HostCore::open_with_key(&self.vault_root, vault_key, self.launch_mode) {
            Ok(host) => *slot = HostSlot::Ready(host),
            Err(error) => {
                *slot = HostSlot::Locked(error.clone());
                return Err(error);
            }
        }
        drop(slot);
        Ok(self.status())
    }

    pub fn status(&self) -> HostStatusView {
        match &*self.lock() {
            HostSlot::NeedsInitialization => HostStatusView {
                state: "needsInitialization",
                vault_ready: false,
                updater_configured: self.updater_configured,
                detail: None,
            },
            HostSlot::AwaitingRecoveryConfirmation(_) => HostStatusView {
                state: "awaitingRecoveryConfirmation",
                vault_ready: false,
                updater_configured: self.updater_configured,
                detail: None,
            },
            HostSlot::Ready(host) => HostStatusView {
                state: encode_host_state(host.lifecycle.state()),
                vault_ready: true,
                updater_configured: self.updater_configured,
                detail: None,
            },
            HostSlot::Locked(detail) => HostStatusView {
                state: "locked",
                vault_ready: false,
                updater_configured: self.updater_configured,
                detail: Some(detail.clone()),
            },
            HostSlot::FailedClosed(detail) => HostStatusView {
                state: "failedClosed",
                vault_ready: false,
                updater_configured: self.updater_configured,
                detail: Some(detail.clone()),
            },
            HostSlot::Closed => HostStatusView {
                state: "stopped",
                vault_ready: false,
                updater_configured: self.updater_configured,
                detail: None,
            },
        }
    }

    pub fn get_runtime_profile(&self) -> Result<RuntimeProfileView, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => host
                .core
                .repository()
                .runtime_profile_view()
                .map(|view| RuntimeProfileView::from(&view))
                .map_err(|error| error.to_string()),
            slot => Err(runtime_profile_slot_error(slot)),
        }
    }

    pub fn test_runtime_profile(
        &self,
        draft: &RuntimeProfileDraft,
    ) -> Result<RuntimeProfileTestView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                let mut candidate = runtime_from_draft(host.core.repository(), draft)?;
                let evidence = ConversationEvidence::restore(
                    EvidenceId::from_raw(1),
                    SessionId::new("runtime-profile-test"),
                    Speaker::Person,
                    RUNTIME_PROFILE_TEST_INPUT.to_owned(),
                    Timestamp::from_millis(0),
                );
                candidate
                    .propose_person_facts(&evidence)
                    .map_err(|error| sanitized_runtime_test_error(error.kind()))?;
                Ok(RuntimeProfileTestView { succeeded: true })
            }
            slot => Err(runtime_profile_slot_error(slot)),
        }
    }

    pub fn save_runtime_profile(
        &self,
        draft: &RuntimeProfileDraft,
    ) -> Result<RuntimeProfileView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                save_runtime_profile_from_core(&mut host.core, draft, |repository, draft| {
                    repository
                        .update_runtime_profile(
                            &draft.base_url,
                            &draft.model,
                            draft.api_key_change.as_vault_action(),
                        )
                        .map_err(|error| error.to_string())
                })
            }
            slot => Err(runtime_profile_slot_error(slot)),
        }
    }

    pub fn mark_hidden(&self) -> Result<(), String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => host
                .lifecycle
                .hide_window()
                .map_err(|error| error.to_string()),
            HostSlot::NeedsInitialization
            | HostSlot::AwaitingRecoveryConfirmation(_)
            | HostSlot::Locked(_)
            | HostSlot::FailedClosed(_) => Ok(()),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn mark_visible(&self) -> Result<(), String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => host
                .lifecycle
                .show_window()
                .map_err(|error| error.to_string()),
            HostSlot::NeedsInitialization
            | HostSlot::AwaitingRecoveryConfirmation(_)
            | HostSlot::Locked(_)
            | HostSlot::FailedClosed(_) => Ok(()),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn heartbeat(&self) -> Result<(), String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                let session_id = host
                    .lifecycle
                    .session_id()
                    .ok_or_else(|| "running host has no lifecycle session".to_owned())?;
                let observed_at = host.host_clock.now();
                host.core
                    .repository_mut()
                    .heartbeat_host_session(session_id, observed_at)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
            HostSlot::NeedsInitialization
            | HostSlot::AwaitingRecoveryConfirmation(_)
            | HostSlot::Locked(_)
            | HostSlot::FailedClosed(_) => Ok(()),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn capture_status(&self) -> Result<CaptureStatusView, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => Ok(CaptureStatusView {
                state: encode_capture_mode(host.capture.mode()),
            }),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn list_activity_timeline(&self) -> Result<Vec<ActivityTimelineEntryView>, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => host
                .core
                .repository()
                .all_capture_spans()
                .map_err(|error| error.to_string())
                .map(|spans| spans.iter().map(ActivityTimelineEntryView::from).collect()),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn record_capture_sample(&self, sample: NativeCaptureSample) -> Result<(), String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                let at = host.host_clock.now();
                let checkpoint = match sample {
                    NativeCaptureSample::Foreground(snapshot)
                        if host.capture.mode() == CaptureMode::Locked =>
                    {
                        Some(
                            host.capture
                                .session_unlocked(snapshot, at)
                                .map_err(|error| error.to_string())?,
                        )
                    }
                    NativeCaptureSample::Foreground(snapshot) => host
                        .capture
                        .observe(snapshot, at)
                        .map_err(|error| error.to_string())?,
                    NativeCaptureSample::SessionLocked => Some(
                        host.capture
                            .session_locked(at)
                            .map_err(|error| error.to_string())?,
                    ),
                    NativeCaptureSample::SourceUnavailable => host
                        .capture
                        .source_unavailable(at)
                        .map_err(|error| error.to_string())?,
                };
                persist_capture_checkpoint(host, checkpoint.as_ref())
            }
            HostSlot::NeedsInitialization
            | HostSlot::AwaitingRecoveryConfirmation(_)
            | HostSlot::Locked(_)
            | HostSlot::FailedClosed(_) => Ok(()),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn set_capture_paused(&self, paused: bool) -> Result<CaptureStatusView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                let at = host.host_clock.now();
                let checkpoint = if paused {
                    host.capture.pause(at).map_err(|error| error.to_string())?
                } else {
                    let NativeCaptureSample::Foreground(snapshot) =
                        sample_foreground_activity(DEFAULT_IDLE_THRESHOLD)
                    else {
                        return Err(
                            "capture cannot resume until the Windows session and foreground source are available"
                                .to_owned(),
                        );
                    };
                    host.capture
                        .resume(snapshot, at)
                        .map_err(|error| error.to_string())?
                };
                persist_capture_checkpoint(host, Some(&checkpoint))?;
                Ok(CaptureStatusView {
                    state: encode_capture_mode(host.capture.mode()),
                })
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn list_conversation(&self) -> Result<Vec<ConversationTurnView>, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => list_conversation_from_core(&host.core),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn get_counterpart_readiness(&self) -> Result<CounterpartReadinessView, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => load_counterpart_readiness(host.core.repository())
                .map(CounterpartReadinessView::from),
            slot => Err(counterpart_slot_error(slot)),
        }
    }

    pub fn record_initial_self_introduction(
        &self,
        draft: InitialSelfIntroductionDraft,
    ) -> Result<CounterpartReadinessView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                match load_counterpart_readiness(host.core.repository())? {
                    CounterpartReadiness::NeedsIntroduction => {}
                    CounterpartReadiness::IntroductionRecorded
                    | CounterpartReadiness::Ready { .. } => {
                        return Err("initial self introduction is already recorded".to_owned());
                    }
                    CounterpartReadiness::Inconsistent { .. } => {
                        return Err("counterpart state is inconsistent".to_owned());
                    }
                }
                let answers = draft.into_answers();
                let (repository, runtime, clock) = host.core.parts_mut();
                let mut formation = IdentityFormation::new(repository, runtime.as_mut(), clock);
                formation
                    .record_initial_self_introduction(
                        &SessionId::new(INITIAL_SELF_INTRODUCTION_SESSION_ID),
                        &answers,
                    )
                    .map_err(sanitized_identity_error)?;
                formation
                    .counterpart_readiness()
                    .map(CounterpartReadinessView::from)
                    .map_err(sanitized_identity_error)
            }
            slot => Err(counterpart_slot_error(slot)),
        }
    }

    pub fn form_initial_counterpart(&self) -> Result<CounterpartReadinessView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                match load_counterpart_readiness(host.core.repository())? {
                    CounterpartReadiness::NeedsIntroduction => {
                        return Err("initial self introduction is required".to_owned());
                    }
                    CounterpartReadiness::IntroductionRecorded => {}
                    CounterpartReadiness::Ready { .. } => {
                        return Err("counterpart is already created".to_owned());
                    }
                    CounterpartReadiness::Inconsistent { .. } => {
                        return Err("counterpart state is inconsistent".to_owned());
                    }
                }
                let (repository, runtime, clock) = host.core.parts_mut();
                let mut formation = IdentityFormation::new(repository, runtime.as_mut(), clock);
                formation
                    .form_initial_counterpart()
                    .map(CounterpartReadinessView::from)
                    .map_err(sanitized_identity_error)
            }
            slot => Err(counterpart_slot_error(slot)),
        }
    }

    pub fn send_message(&self, verbatim: String) -> Result<ConversationTurnResult, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                require_ready_for_formal_conversation(host.core.repository())?;
                send_message_with_retrieval(&mut host.core, verbatim)
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn list_shared_experience_ceremonies(
        &self,
    ) -> Result<Vec<SharedExperienceCeremonyView>, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => list_shared_experience_ceremonies_from_core(&host.core),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn list_active_shared_agreements(&self) -> Result<Vec<ActiveSharedAgreementView>, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                let at = host.host_clock.now();
                list_active_shared_agreements_from_core(&host.core, at)
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn list_identity_history(&self) -> Result<Vec<IdentityStateView>, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => list_identity_history_from_core(&host.core),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn list_offered_reflection_invitations(
        &self,
    ) -> Result<Vec<ReflectionInvitationView>, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => list_offered_reflection_invitations_from_core(&host.core),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn decide_reflection_invitation(
        &self,
        invitation_id: u64,
        decision: &str,
    ) -> Result<ReflectionInvitationDecisionView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                decide_reflection_invitation_from_core(&mut host.core, invitation_id, decision)
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn resolve_shared_agreement(
        &self,
        candidate_id: u64,
        confirm: bool,
    ) -> Result<SharedAgreementResolutionView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                resolve_shared_agreement_from_core(&mut host.core, candidate_id, confirm)
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn withdraw_shared_agreement_as_person(
        &self,
        agreement_claim_id: u64,
        confirmed: bool,
        reason: Option<String>,
    ) -> Result<Option<u64>, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => withdraw_shared_agreement_as_person_from_core(
                &mut host.core,
                agreement_claim_id,
                confirmed,
                reason,
            ),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn revise_shared_agreement(
        &self,
        candidate_id: u64,
        statement: String,
        scope: String,
        effective_from_millis: i64,
        effective_until_millis: Option<i64>,
        end_condition: Option<String>,
        supersedes_agreement_ids: Vec<u64>,
    ) -> Result<SharedAgreementRevisionView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => revise_shared_agreement_from_core(
                &mut host.core,
                candidate_id,
                statement,
                scope,
                effective_from_millis,
                effective_until_millis,
                end_condition,
                supersedes_agreement_ids,
            ),
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn dismiss_shared_experience_ceremony(&self, claim_id: u64) -> Result<(), String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                dismiss_shared_experience_ceremony_from_core(&mut host.core, claim_id)
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn import_context_file(
        &self,
        path: &str,
        approve_oversized: bool,
    ) -> Result<ImportContextFileView, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                let archived_at_millis = host.host_clock.now().as_millis();
                import_context_file_with_policy(
                    host.core.repository_mut(),
                    Path::new(path),
                    &ImportPolicy::default(),
                    approve_oversized,
                    archived_at_millis,
                )
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn record_browser_submission(
        &self,
        submission: &BrowserSubmission,
    ) -> Result<BrowserCaptureReceipt, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => {
                let session_id = host
                    .lifecycle
                    .session_id()
                    .ok_or_else(|| "running host has no lifecycle session".to_owned())?;
                host.core
                    .repository_mut()
                    .record_browser_submission(session_id, submission)
                    .map_err(|error| error.to_string())
            }
            HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
                Err(VAULT_SETUP_INCOMPLETE.to_owned())
            }
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn shutdown(&self, reason: ExitReason) -> Result<(), Vec<String>> {
        let slot = {
            let mut guard = self.lock();
            std::mem::replace(&mut *guard, HostSlot::Closed)
        };
        let HostSlot::Ready(mut host) = slot else {
            return Ok(());
        };
        let exit_plan = match host.lifecycle.begin_exit(reason) {
            Ok(plan) => plan,
            Err(error) => {
                *self.lock() = HostSlot::Ready(host);
                return Err(vec![error.to_string()]);
            }
        };
        let ended_at = host.host_clock.now();
        let capture_result = host
            .capture
            .stop(
                match exit_plan.reason() {
                    ExitReason::Explicit => ShutdownReason::ExplicitExit,
                    ExitReason::Update => ShutdownReason::Update,
                },
                ended_at,
            )
            .map_err(|error| error.to_string())
            .and_then(|checkpoint| persist_capture_checkpoint(&mut host, Some(&checkpoint)));
        let finish_result = host
            .core
            .repository_mut()
            .finish_host_session(exit_plan.session_id(), ended_at, exit_plan.reason())
            .map(|_| ())
            .map_err(|error| error.to_string());
        let (repository, runtime, _core_clock) = host.core.into_parts();
        drop(runtime);
        let close_result = repository.close().map_err(|error| error.to_string());
        let state_result = host
            .lifecycle
            .mark_stopped()
            .map_err(|error| error.to_string());
        collect_shutdown_errors(capture_result, finish_result, close_result, state_result)
    }

    pub fn reopen_after_update_failure(&self) -> Result<(), String> {
        match HostCore::open(&self.vault_root, LaunchMode::UpdateRelaunch) {
            Ok(host) => {
                *self.lock() = HostSlot::Ready(host);
                Ok(())
            }
            Err(error) => {
                *self.lock() = HostSlot::FailedClosed(error.clone());
                Err(error)
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, HostSlot> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl HostCore {
    fn open(vault_root: &Path, launch_mode: LaunchMode) -> Result<Self, String> {
        let vault_key =
            VaultKeyStore::unlock_local(vault_root).map_err(|error| error.to_string())?;
        Self::open_with_key(vault_root, vault_key, launch_mode)
    }

    fn open_with_key(
        vault_root: &Path,
        vault_key: VaultKey,
        launch_mode: LaunchMode,
    ) -> Result<Self, String> {
        let mut repository =
            VaultRepository::open(vault_root, vault_key).map_err(|error| error.to_string())?;
        let runtime = {
            let profile = repository
                .runtime_profile()
                .map_err(|error| error.to_string())?;
            runtime_from_profile(&profile)?
        };
        let mut lifecycle = HostLifecycle::new();
        lifecycle
            .begin_recovery()
            .map_err(|error| error.to_string())?;
        let mut host_clock = SystemClock;
        let started_at = host_clock.now();
        let start = match repository.begin_host_session(started_at, launch_mode) {
            Ok(start) => start,
            Err(error) => {
                let message = error.to_string();
                let _ = repository.close();
                return Err(message);
            }
        };
        let capture_recovery = match repository.recover_capture_timeline(
            start.session().id(),
            started_at,
            start
                .recovered_gap()
                .map(eam_desktop_host::HostRuntimeGap::reason),
        ) {
            Ok(recovery) => recovery,
            Err(error) => {
                let message = error.to_string();
                let _ = repository.close();
                return Err(message);
            }
        };
        lifecycle
            .complete_recovery(start.session().id(), launch_mode)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            core: MemoryCore::new(repository, runtime, SystemClock),
            lifecycle,
            capture: CaptureStateMachine::restore(&capture_recovery),
            host_clock,
        })
    }
}

impl RuntimeProfileApiKeyChange {
    fn as_vault_action(&self) -> VaultRuntimeProfileKeyAction<'_> {
        match self {
            Self::Keep => VaultRuntimeProfileKeyAction::Keep,
            Self::Replace(value) => VaultRuntimeProfileKeyAction::Replace(value),
            Self::Clear => VaultRuntimeProfileKeyAction::Clear,
        }
    }
}

impl From<&VaultRuntimeProfileView> for RuntimeProfileView {
    fn from(value: &VaultRuntimeProfileView) -> Self {
        Self {
            base_url: value.base_url().to_owned(),
            model: value.model().to_owned(),
            api_key_configured: value.api_key_configured(),
            api_key_last_four: value.api_key_last_four().map(str::to_owned),
        }
    }
}

fn runtime_from_profile(profile: &VaultRuntimeProfile) -> Result<AppRuntime, String> {
    let target = validated_runtime_target(profile.base_url(), profile.model())?;
    runtime_from_target(target, profile.bearer_key().map(str::to_owned))
}

fn runtime_from_draft(
    repository: &VaultRepository,
    draft: &RuntimeProfileDraft,
) -> Result<AppRuntime, String> {
    let target = validated_runtime_target(&draft.base_url, &draft.model)?;
    let bearer_key = match &draft.api_key_change {
        RuntimeProfileApiKeyChange::Keep => repository
            .runtime_profile()
            .map_err(|error| error.to_string())?
            .bearer_key()
            .map(str::to_owned),
        RuntimeProfileApiKeyChange::Replace(value) => Some(value.clone()),
        RuntimeProfileApiKeyChange::Clear => None,
    };
    runtime_from_target(target, bearer_key)
}

fn validated_runtime_target(base_url: &str, model: &str) -> Result<RuntimeTarget, String> {
    RuntimeTarget::new(base_url, model.to_owned())
        .map_err(|_| "runtime profile is invalid".to_owned())
}

fn runtime_from_target(
    target: RuntimeTarget,
    bearer_key: Option<String>,
) -> Result<AppRuntime, String> {
    let transport = HttpResponsesTransport::new(bearer_key)
        .map_err(|_| "runtime transport could not be initialized".to_owned())?;
    Ok(Box::new(OpenAiResponsesRuntime::new(
        target,
        transport,
        RUNTIME_TIMEOUT,
    )))
}

fn save_runtime_profile_from_core<F>(
    core: &mut AppCore,
    draft: &RuntimeProfileDraft,
    persist: F,
) -> Result<RuntimeProfileView, String>
where
    F: FnOnce(
        &mut VaultRepository,
        &RuntimeProfileDraft,
    ) -> Result<VaultRuntimeProfileView, String>,
{
    let candidate = runtime_from_draft(core.repository(), draft)?;
    let persisted = persist(core.repository_mut(), draft)?;
    let response = RuntimeProfileView::from(&persisted);
    drop(core.replace_runtime(candidate));
    Ok(response)
}

fn runtime_profile_slot_error(slot: &HostSlot) -> String {
    match slot {
        HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
            VAULT_SETUP_INCOMPLETE.to_owned()
        }
        HostSlot::Locked(detail) => format!("vault is locked: {detail}"),
        HostSlot::FailedClosed(detail) => format!("Core is closed: {detail}"),
        HostSlot::Closed => "desktop host is already stopped".to_owned(),
        HostSlot::Ready(_) => unreachable!("ready hosts are handled by the command path"),
    }
}

fn counterpart_slot_error(slot: &HostSlot) -> String {
    match slot {
        HostSlot::NeedsInitialization | HostSlot::AwaitingRecoveryConfirmation(_) => {
            VAULT_SETUP_INCOMPLETE.to_owned()
        }
        HostSlot::Locked(detail) => format!("vault is locked: {detail}"),
        HostSlot::FailedClosed(detail) => format!("Core is closed: {detail}"),
        HostSlot::Closed => "desktop host is already stopped".to_owned(),
        HostSlot::Ready(_) => unreachable!("ready hosts are handled by the command path"),
    }
}

fn load_counterpart_readiness<R: CounterpartRepository + ?Sized>(
    repository: &R,
) -> Result<CounterpartReadiness, String> {
    repository
        .counterpart_readiness()
        .map_err(|_| "counterpart readiness could not be loaded".to_owned())
}

fn require_ready_for_formal_conversation<R: CounterpartRepository + ?Sized>(
    repository: &R,
) -> Result<(), String> {
    match load_counterpart_readiness(repository)? {
        CounterpartReadiness::Ready { .. } => Ok(()),
        CounterpartReadiness::NeedsIntroduction
        | CounterpartReadiness::IntroductionRecorded
        | CounterpartReadiness::Inconsistent { .. } => {
            Err("counterpart is not ready for formal conversation".to_owned())
        }
    }
}

fn sanitized_identity_error(error: IdentityError) -> String {
    match error {
        IdentityError::MissingCategories(_)
        | IdentityError::DuplicateCategory(_)
        | IdentityError::EmptyAnswer(_) => {
            "initial self introduction must contain six non-empty answers".to_owned()
        }
        IdentityError::IntroductionAlreadyRecorded => {
            "initial self introduction is already recorded".to_owned()
        }
        IdentityError::IntroductionNotRecorded => {
            "initial self introduction is required".to_owned()
        }
        IdentityError::IdentityAlreadyFormed | IdentityError::CounterpartAlreadyCreated => {
            "counterpart is already created".to_owned()
        }
        IdentityError::InconsistentCounterpartState(_) => {
            "counterpart state is inconsistent".to_owned()
        }
        IdentityError::InvalidProposal(_) => {
            "counterpart formation was rejected by trusted validation".to_owned()
        }
        IdentityError::Repository(_) => "counterpart storage operation failed".to_owned(),
        IdentityError::Runtime(error) => match error.kind() {
            RuntimeErrorKind::Timeout => "counterpart formation runtime timed out".to_owned(),
            RuntimeErrorKind::Unavailable => {
                "counterpart formation runtime is unavailable".to_owned()
            }
            RuntimeErrorKind::InvalidResponse => {
                "counterpart formation returned an invalid strict response".to_owned()
            }
            RuntimeErrorKind::Other => "counterpart formation runtime failed".to_owned(),
        },
    }
}

impl From<CounterpartReadiness> for CounterpartReadinessView {
    fn from(value: CounterpartReadiness) -> Self {
        match value {
            CounterpartReadiness::NeedsIntroduction => Self {
                state: "NEEDS_INTRODUCTION",
                identity_version: None,
                self_bundle_version: None,
                inconsistency_reason: None,
            },
            CounterpartReadiness::IntroductionRecorded => Self {
                state: "INTRODUCTION_RECORDED",
                identity_version: None,
                self_bundle_version: None,
                inconsistency_reason: None,
            },
            CounterpartReadiness::Ready {
                identity_version,
                self_bundle_version,
            } => Self {
                state: "READY",
                identity_version: Some(identity_version),
                self_bundle_version: Some(self_bundle_version),
                inconsistency_reason: None,
            },
            CounterpartReadiness::Inconsistent { reason } => Self {
                state: "INCONSISTENT",
                identity_version: None,
                self_bundle_version: None,
                inconsistency_reason: Some(encode_counterpart_inconsistency(&reason)),
            },
        }
    }
}

const fn encode_counterpart_inconsistency(reason: &CounterpartInconsistencyReason) -> &'static str {
    match reason {
        CounterpartInconsistencyReason::IntroductionMissing { .. } => "INTRODUCTION_MISSING",
        CounterpartInconsistencyReason::IdentityMissing { .. } => "IDENTITY_MISSING",
        CounterpartInconsistencyReason::SelfBundleMissing { .. } => "SELF_BUNDLE_MISSING",
        CounterpartInconsistencyReason::IdentityVersionMismatch { .. } => {
            "IDENTITY_VERSION_MISMATCH"
        }
    }
}

fn sanitized_runtime_test_error(kind: RuntimeErrorKind) -> String {
    match kind {
        RuntimeErrorKind::Timeout => "runtime profile test timed out".to_owned(),
        RuntimeErrorKind::Unavailable => "runtime profile is unavailable".to_owned(),
        RuntimeErrorKind::InvalidResponse => {
            "runtime profile returned an invalid strict response".to_owned()
        }
        RuntimeErrorKind::Other => "runtime profile test failed".to_owned(),
    }
}

fn list_conversation_from_core<R, T, C>(
    core: &MemoryCore<R, T, C>,
) -> Result<Vec<ConversationTurnView>, String>
where
    R: MemoryRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    core.repository()
        .all_evidence()
        .map_err(|error| error.to_string())
        .map(|evidence| {
            evidence
                .iter()
                .filter(|turn| turn.session_id().as_str() == CONTINUOUS_SESSION_ID)
                .map(ConversationTurnView::from)
                .collect()
        })
}

fn list_identity_history_from_core<R, T, C>(
    core: &MemoryCore<R, T, C>,
) -> Result<Vec<IdentityStateView>, String>
where
    R: IdentityEvolutionRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    core.repository()
        .identity_history()
        .map_err(|error| error.to_string())
        .map(|history| history.iter().map(IdentityStateView::from).collect())
}

impl From<&IdentityStateSnapshot> for IdentityStateView {
    fn from(value: &IdentityStateSnapshot) -> Self {
        Self {
            version: value.version(),
            predecessor_version: value.predecessor_version(),
            name: value.profile().name().to_owned(),
            expression_traits: value.profile().expression_traits().to_owned(),
            viewpoints: value.profile().viewpoints().to_owned(),
            value_priorities: value.profile().value_priorities().to_owned(),
            relationship_posture: value.profile().relationship_posture().to_owned(),
            own_goals: value.profile().own_goals().to_owned(),
            change_reason: value.change_reason().to_owned(),
            evidence_ids: value.evidence_refs().iter().map(|id| id.get()).collect(),
            formed_at_millis: value.formed_at().as_millis(),
        }
    }
}

#[cfg(test)]
fn send_message_with_core<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    verbatim: String,
) -> Result<ConversationTurnResult, String>
where
    R: SharedExperienceRepository + IdentityEvolutionRepository + ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    validate_message(&verbatim)?;
    let prior_turns = core
        .repository()
        .all_evidence()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|turn| turn.session_id().as_str() == CONTINUOUS_SESSION_ID)
        .collect::<Vec<_>>();
    let context_ids = select_context_ids(&prior_turns);
    let working_context = core
        .freeze_working_context(&context_ids)
        .map_err(|error| error.to_string())?;
    let working_context = if search_terms(&verbatim).is_empty() {
        working_context
    } else {
        with_relational_constraints(
            core.repository(),
            &RetrievalQuery::lexical(&verbatim),
            working_context,
        )?
    };
    let working_context =
        with_reflection_opportunity(core.repository(), &verbatim, working_context)?;
    run_message_with_context(core, verbatim, working_context)
}

fn send_message_with_retrieval<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    verbatim: String,
) -> Result<ConversationTurnResult, String>
where
    R: SharedExperienceRepository
        + IdentityEvolutionRepository
        + ReflectionInvitationRepository
        + RetrievalRepository,
    <R as RetrievalRepository>::Error: std::fmt::Display,
    T: CounterpartRuntime,
    C: Clock,
{
    validate_message(&verbatim)?;
    let prior_turns = core
        .repository()
        .all_evidence()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|turn| turn.session_id().as_str() == CONTINUOUS_SESSION_ID)
        .collect::<Vec<_>>();
    let context_ids = select_context_ids(&prior_turns);
    let selected = core
        .freeze_working_context(&context_ids)
        .map_err(|error| error.to_string())?;
    let frozen_at = selected.frozen_at();
    let selected_evidence = selected.evidence().to_vec();
    if search_terms(&verbatim).is_empty() {
        let selected = with_reflection_opportunity(core.repository(), &verbatim, selected)?;
        return run_message_with_context(core, verbatim, selected);
    }
    let query = RetrievalQuery::lexical(&verbatim);
    let working_context = freeze_retrieval(
        core.repository_mut(),
        &query,
        TokenBudget::default(),
        selected_evidence,
        frozen_at,
    )
    .map_err(|error| error.to_string())?;
    let working_context = with_relational_constraints(core.repository(), &query, working_context)?;
    let working_context =
        with_reflection_opportunity(core.repository(), &verbatim, working_context)?;
    run_message_with_context(core, verbatim, working_context)
}

fn with_reflection_opportunity<R: ReflectionInvitationRepository>(
    repository: &R,
    verbatim: &str,
    working_context: WorkingContext,
) -> Result<WorkingContext, String> {
    let message_terms = search_terms(verbatim)
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect::<std::collections::BTreeSet<_>>();
    if message_terms.is_empty() {
        return Ok(working_context);
    }
    let mut related = repository
        .all_reflection_invitations()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(ReflectionInvitation::is_open)
        .filter(|invitation| reflection_topic_matches(&message_terms, invitation.topic_key()))
        .collect::<Vec<_>>();
    related.sort_by(|left, right| {
        right
            .importance()
            .cmp(&left.importance())
            .then_with(|| {
                left.created_at()
                    .as_millis()
                    .cmp(&right.created_at().as_millis())
            })
            .then_with(|| left.id().cmp(&right.id()))
    });
    Ok(match related.first() {
        Some(invitation) => working_context.with_reflection_opportunity(
            ReflectionOpportunity::RelatedTopic(invitation.topic_key().to_owned()),
        ),
        None => working_context,
    })
}

fn reflection_topic_matches(
    message_terms: &std::collections::BTreeSet<String>,
    topic_key: &str,
) -> bool {
    let topic_terms = search_terms(topic_key)
        .into_iter()
        .filter(|term| term.chars().count() >= 2)
        .collect::<Vec<_>>();
    let required_matches = topic_terms.len().min(2);
    required_matches > 0
        && topic_terms
            .iter()
            .filter(|term| message_terms.contains(*term))
            .take(required_matches)
            .count()
            == required_matches
}

fn with_relational_constraints<R: SharedExperienceRepository>(
    repository: &R,
    query: &RetrievalQuery,
    working_context: WorkingContext,
) -> Result<WorkingContext, String> {
    let candidates = repository
        .all_shared_agreement_candidates()
        .map_err(|error| error.to_string())?;
    let experiences = repository
        .all_shared_experiences()
        .map_err(|error| error.to_string())?;
    let constraints = project_active_relational_constraints(
        query,
        &candidates,
        &experiences,
        working_context.frozen_at(),
    );
    working_context
        .with_active_relational_constraints(constraints)
        .map_err(|error| error.to_string())
}

fn run_message_with_context<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    verbatim: String,
    working_context: WorkingContext,
) -> Result<ConversationTurnResult, String>
where
    R: SharedExperienceRepository + IdentityEvolutionRepository + ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    let outcome = core
        .run_counterpart_turn(
            SessionId::new(CONTINUOUS_SESSION_ID),
            verbatim,
            working_context,
        )
        .map_err(|error| error.to_string())?;
    let person = conversation_turn(core, outcome.person_evidence_id())?;
    let counterpart = conversation_turn(core, outcome.counterpart_evidence_id())?;
    let ceremonies = list_shared_experience_ceremonies_from_core(core)?;
    let reflection_invitations = list_offered_reflection_invitations_from_core(core)?;
    Ok(ConversationTurnResult {
        person,
        counterpart,
        ceremonies,
        reflection_invitations,
    })
}

fn list_offered_reflection_invitations_from_core<R, T, C>(
    core: &MemoryCore<R, T, C>,
) -> Result<Vec<ReflectionInvitationView>, String>
where
    R: ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    core.repository()
        .all_reflection_invitations()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|invitation| invitation.state() == ReflectionInvitationState::Offered)
        .map(|invitation| reflection_invitation_view(core.repository(), &invitation))
        .collect()
}

fn reflection_invitation_view<R: MemoryRepository>(
    repository: &R,
    invitation: &ReflectionInvitation,
) -> Result<ReflectionInvitationView, String> {
    let evidence = invitation
        .evidence_refs()
        .iter()
        .map(|citation| {
            let source = repository
                .evidence(citation.evidence_id())
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    format!(
                        "reflection evidence {} is missing",
                        citation.evidence_id().get()
                    )
                })?;
            if !source.verbatim().contains(citation.quote()) {
                return Err(format!(
                    "reflection evidence {} no longer matches its quote",
                    citation.evidence_id().get()
                ));
            }
            Ok(ReflectionInvitationEvidenceView {
                evidence_id: source.id().get(),
                speaker: encode_speaker(source.speaker()),
                quote: citation.quote().to_owned(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ReflectionInvitationView {
        id: invitation.id().get(),
        topic_key: invitation.topic_key().to_owned(),
        observation: invitation.observation().to_owned(),
        why_now: invitation.why_now().to_owned(),
        importance: encode_reflection_importance(invitation.importance()),
        basis: encode_reflection_basis(invitation.basis()),
        defer_count: invitation.defer_count(),
        show_mute_prompt: invitation.mute_prompted() && invitation.defer_count() == 1,
        evidence,
    })
}

fn decide_reflection_invitation_from_core<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    invitation_id: u64,
    decision: &str,
) -> Result<ReflectionInvitationDecisionView, String>
where
    R: SharedExperienceRepository + IdentityEvolutionRepository + ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    let decision = match decision {
        "defer" => ReflectionDecision::Defer,
        "mute" => ReflectionDecision::Mute,
        "resolve" => ReflectionDecision::Resolve,
        _ => return Err("unknown reflection invitation decision".to_owned()),
    };
    let receipt = core
        .decide_reflection_invitation(ReflectionInvitationId::from_raw(invitation_id), decision)
        .map_err(|error| error.to_string())?;
    Ok(ReflectionInvitationDecisionView {
        invitation_id: receipt.id().get(),
        state: encode_reflection_state(receipt.state()),
    })
}

fn list_shared_experience_ceremonies_from_core<R, T, C>(
    core: &MemoryCore<R, T, C>,
) -> Result<Vec<SharedExperienceCeremonyView>, String>
where
    R: SharedExperienceRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    let repository = core.repository();
    let mut ceremonies = Vec::new();
    let candidates = repository
        .all_shared_agreement_candidates()
        .map_err(|error| error.to_string())?;
    let experiences = repository
        .all_shared_experiences()
        .map_err(|error| error.to_string())?;
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.status() == SharedAgreementCandidateStatus::AwaitingPerson)
    {
        ceremonies.push(SharedExperienceCeremonyView {
            target_id: candidate.id().get(),
            target_kind: "agreementCandidate",
            experience_kind: "agreement",
            admission: "confirmationRequired",
            statement: candidate.statement().to_owned(),
            candidate_version: Some(candidate.version()),
            scope: candidate.scope().map(str::to_owned),
            effective_from_millis: candidate
                .effective_from()
                .map(eam_core::Timestamp::as_millis),
            effective_until_millis: candidate
                .effective_until()
                .map(eam_core::Timestamp::as_millis),
            end_condition: candidate.end_condition().map(str::to_owned),
            agreement_claim_id: None,
            departure_reason: None,
            withdrawal_actor: None,
            superseded_agreements: superseded_agreement_views(
                &candidates,
                &experiences,
                candidate.supersedes_agreement_ids(),
            )?,
            evidence: ceremony_evidence(repository, candidate.support())?,
        });
    }
    for experience in experiences.into_iter().filter(|experience| {
        experience.kind() != SharedExperienceKind::Agreement && !experience.ceremony_dismissed()
    }) {
        let withdrawal = experience.agreement_withdrawal();
        ceremonies.push(SharedExperienceCeremonyView {
            target_id: experience.claim().id().get(),
            target_kind: "sharedExperience",
            experience_kind: encode_shared_experience_kind(experience.kind()),
            admission: "nonVetoNotice",
            statement: experience.claim().statement().to_owned(),
            candidate_version: None,
            scope: None,
            effective_from_millis: withdrawal
                .map(|withdrawal| withdrawal.effective_at().as_millis()),
            effective_until_millis: None,
            end_condition: None,
            agreement_claim_id: experience
                .constraint_departure()
                .map(|departure| departure.agreement_claim_id().get())
                .or_else(|| withdrawal.map(|withdrawal| withdrawal.agreement_claim_id().get())),
            departure_reason: experience
                .constraint_departure()
                .map(|departure| departure.reason().to_owned())
                .or_else(|| {
                    withdrawal.and_then(|withdrawal| withdrawal.reason().map(str::to_owned))
                }),
            withdrawal_actor: withdrawal
                .map(|withdrawal| encode_agreement_withdrawal_actor(withdrawal.actor())),
            superseded_agreements: Vec::new(),
            evidence: ceremony_evidence(repository, experience.claim().support())?,
        });
    }
    Ok(ceremonies)
}

fn list_active_shared_agreements_from_core<R, T, C>(
    core: &MemoryCore<R, T, C>,
    at: eam_core::Timestamp,
) -> Result<Vec<ActiveSharedAgreementView>, String>
where
    R: SharedExperienceRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    let candidates = core
        .repository()
        .all_shared_agreement_candidates()
        .map_err(|error| error.to_string())?;
    let experiences = core
        .repository()
        .all_shared_experiences()
        .map_err(|error| error.to_string())?;
    Ok(candidates
        .iter()
        .filter_map(|candidate| {
            let claim_id = candidate.claim_id()?;
            agreement_is_active_at(claim_id, &candidates, &experiences, at).then(|| {
                ActiveSharedAgreementView {
                    claim_id: claim_id.get(),
                    statement: candidate.statement().to_owned(),
                    scope: candidate.scope().unwrap_or_default().to_owned(),
                    effective_from_millis: candidate
                        .effective_from()
                        .expect("active agreement has an effective time")
                        .as_millis(),
                    effective_until_millis: candidate
                        .effective_until()
                        .map(eam_core::Timestamp::as_millis),
                }
            })
        })
        .collect())
}

fn withdraw_shared_agreement_as_person_from_core<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    agreement_claim_id: u64,
    confirmed: bool,
    reason: Option<String>,
) -> Result<Option<u64>, String>
where
    R: SharedExperienceRepository + IdentityEvolutionRepository + ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    core.withdraw_shared_agreement_as_person(
        SessionId::new(CONTINUOUS_SESSION_ID),
        ClaimId::from_raw(agreement_claim_id),
        confirmed,
        reason,
    )
    .map(|claim_id| claim_id.map(ClaimId::get))
    .map_err(|error| error.to_string())
}

fn superseded_agreement_views(
    candidates: &[eam_core::SharedAgreementCandidate],
    experiences: &[eam_core::SharedExperience],
    claim_ids: &[ClaimId],
) -> Result<Vec<SupersededAgreementView>, String> {
    claim_ids
        .iter()
        .map(|claim_id| {
            if !experiences.iter().any(|experience| {
                experience.kind() == SharedExperienceKind::Agreement
                    && experience.claim().id() == *claim_id
            }) {
                return Err(format!(
                    "superseded agreement claim {} is missing",
                    claim_id.get()
                ));
            }
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.claim_id() == Some(*claim_id))
                .ok_or_else(|| {
                    format!(
                        "superseded agreement candidate for claim {} is missing",
                        claim_id.get()
                    )
                })?;
            Ok(SupersededAgreementView {
                claim_id: claim_id.get(),
                statement: candidate.statement().to_owned(),
                scope: candidate
                    .scope()
                    .ok_or_else(|| "superseded agreement scope is missing".to_owned())?
                    .to_owned(),
                effective_from_millis: candidate
                    .effective_from()
                    .ok_or_else(|| "superseded agreement effective time is missing".to_owned())?
                    .as_millis(),
                effective_until_millis: candidate
                    .effective_until()
                    .map(eam_core::Timestamp::as_millis),
            })
        })
        .collect()
}

fn ceremony_evidence<R: MemoryRepository>(
    repository: &R,
    support: &[EvidenceCitation],
) -> Result<Vec<SharedExperienceCeremonyEvidenceView>, String> {
    support
        .iter()
        .map(|citation| {
            let source = repository
                .evidence(citation.evidence_id())
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    format!(
                        "ceremony evidence {} is missing",
                        citation.evidence_id().get()
                    )
                })?;
            Ok(SharedExperienceCeremonyEvidenceView {
                evidence_id: source.id().get(),
                speaker: encode_speaker(source.speaker()),
                quote: citation.quote().to_owned(),
            })
        })
        .collect()
}

fn resolve_shared_agreement_from_core<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    candidate_id: u64,
    confirm: bool,
) -> Result<SharedAgreementResolutionView, String>
where
    R: SharedExperienceRepository + IdentityEvolutionRepository + ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    let resolution = core
        .resolve_shared_agreement(
            eam_core::SharedAgreementCandidateId::from_raw(candidate_id),
            if confirm {
                SharedAgreementDecision::Confirm
            } else {
                SharedAgreementDecision::Defer
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(SharedAgreementResolutionView::from(resolution))
}

#[allow(clippy::too_many_arguments)]
fn revise_shared_agreement_from_core<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    candidate_id: u64,
    statement: String,
    scope: String,
    effective_from_millis: i64,
    effective_until_millis: Option<i64>,
    end_condition: Option<String>,
    supersedes_agreement_ids: Vec<u64>,
) -> Result<SharedAgreementRevisionView, String>
where
    R: SharedExperienceRepository + IdentityEvolutionRepository + ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    let candidate_id = core
        .revise_shared_agreement(
            eam_core::SharedAgreementCandidateId::from_raw(candidate_id),
            SessionId::new(CONTINUOUS_SESSION_ID),
            SharedAgreementRevision::new(
                statement,
                scope,
                eam_core::Timestamp::from_millis(effective_from_millis),
                effective_until_millis.map(eam_core::Timestamp::from_millis),
                end_condition,
            )
            .with_superseded_agreements(
                supersedes_agreement_ids
                    .into_iter()
                    .map(ClaimId::from_raw)
                    .collect(),
            ),
        )
        .map_err(|error| error.to_string())?;
    let candidate = core
        .repository()
        .shared_agreement_candidate(candidate_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("revised candidate {} is missing", candidate_id.get()))?;
    Ok(SharedAgreementRevisionView {
        candidate_id: candidate.id().get(),
        version: candidate.version(),
        status: "awaitingCounterpart",
    })
}

fn dismiss_shared_experience_ceremony_from_core<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    claim_id: u64,
) -> Result<(), String>
where
    R: SharedExperienceRepository + IdentityEvolutionRepository + ReflectionInvitationRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    core.dismiss_shared_experience_ceremony(ClaimId::from_raw(claim_id))
        .map_err(|error| error.to_string())?
        .then_some(())
        .ok_or_else(|| format!("shared experience {claim_id} does not exist"))
}

const fn encode_shared_experience_kind(kind: SharedExperienceKind) -> &'static str {
    match kind {
        SharedExperienceKind::Agreement => "agreement",
        SharedExperienceKind::SubstantiveDisagreement => "substantiveDisagreement",
        SharedExperienceKind::RelationshipChange => "relationshipChange",
        SharedExperienceKind::SharedAchievement => "sharedAchievement",
        SharedExperienceKind::AgreementBreach => "agreementBreach",
        SharedExperienceKind::AgreementWithdrawal => "agreementWithdrawal",
    }
}

const fn encode_agreement_withdrawal_actor(actor: AgreementWithdrawalActor) -> &'static str {
    match actor {
        AgreementWithdrawalActor::Person => "person",
        AgreementWithdrawalActor::Counterpart => "counterpart",
    }
}

const fn encode_reflection_importance(importance: ReflectionImportance) -> &'static str {
    match importance {
        ReflectionImportance::Ordinary => "ordinary",
        ReflectionImportance::Important => "important",
        ReflectionImportance::ImmediateSafetyRisk => "immediateSafetyRisk",
    }
}

const fn encode_reflection_basis(basis: ReflectionInvitationBasis) -> &'static str {
    match basis {
        ReflectionInvitationBasis::ImportantSingleChange => "importantSingleChange",
        ReflectionInvitationBasis::RepeatedPattern => "repeatedPattern",
    }
}

const fn encode_reflection_state(state: ReflectionInvitationState) -> &'static str {
    match state {
        ReflectionInvitationState::Pending => "pending",
        ReflectionInvitationState::Offered => "offered",
        ReflectionInvitationState::Deferred => "deferred",
        ReflectionInvitationState::MutedByPerson => "mutedByPerson",
        ReflectionInvitationState::Resolved => "resolved",
    }
}

const fn encode_speaker(speaker: Speaker) -> &'static str {
    match speaker {
        Speaker::Person => "person",
        Speaker::Counterpart => "counterpart",
    }
}

impl From<SharedAgreementResolution> for SharedAgreementResolutionView {
    fn from(value: SharedAgreementResolution) -> Self {
        Self {
            candidate_id: value.candidate_id().get(),
            status: match value.status() {
                SharedAgreementCandidateStatus::AwaitingCounterpart => "awaitingCounterpart",
                SharedAgreementCandidateStatus::AwaitingPerson => "awaitingPerson",
                SharedAgreementCandidateStatus::Deferred => "deferred",
                SharedAgreementCandidateStatus::Confirmed => "confirmed",
            },
            claim_id: value.claim_id().map(ClaimId::get),
        }
    }
}

fn validate_message(verbatim: &str) -> Result<(), String> {
    if verbatim.trim().is_empty() {
        return Err("message cannot be empty".to_owned());
    }
    if verbatim.len() > MAX_MESSAGE_BYTES {
        return Err(format!(
            "message exceeds the {MAX_MESSAGE_BYTES}-byte limit"
        ));
    }
    Ok(())
}

fn select_context_ids(evidence: &[ConversationEvidence]) -> Vec<EvidenceId> {
    let mut bytes = 0;
    let mut selected = Vec::new();
    for turn in evidence.iter().rev().take(MAX_CONTEXT_TURNS) {
        let next_bytes = bytes + turn.verbatim().len();
        if next_bytes > MAX_CONTEXT_BYTES {
            break;
        }
        bytes = next_bytes;
        selected.push(turn.id());
    }
    selected.reverse();
    selected
}

fn conversation_turn<R, T, C>(
    core: &MemoryCore<R, T, C>,
    evidence_id: EvidenceId,
) -> Result<ConversationTurnView, String>
where
    R: MemoryRepository,
    T: CounterpartRuntime,
    C: Clock,
{
    core.repository()
        .evidence(evidence_id)
        .map_err(|error| error.to_string())?
        .as_ref()
        .map(ConversationTurnView::from)
        .ok_or_else(|| format!("conversation evidence {} is missing", evidence_id.get()))
}

fn import_context_file_with_policy<R: ArchiveRepository>(
    repository: &mut R,
    path: &Path,
    policy: &ImportPolicy,
    approve_oversized: bool,
    archived_at_millis: i64,
) -> Result<ImportContextFileView, String>
where
    R::Error: std::fmt::Display,
{
    let outcome = ingest_inbox_file(
        repository,
        path,
        policy,
        approve_oversized,
        archived_at_millis,
    )
    .map_err(|error| error.to_string())?;
    Ok(import_context_file_view(outcome))
}

const fn import_context_file_view(outcome: ImportOutcome) -> ImportContextFileView {
    match outcome {
        ImportOutcome::Discovered => ImportContextFileView {
            status: "discovered",
            archive_id: None,
            reason: None,
            bytes: None,
            object_reused: false,
            source_version_reused: false,
        },
        ImportOutcome::AwaitingApproval { bytes } => ImportContextFileView {
            status: "awaitingApproval",
            archive_id: None,
            reason: None,
            bytes: Some(bytes),
            object_reused: false,
            source_version_reused: false,
        },
        ImportOutcome::Rejected(reason) => ImportContextFileView {
            status: "rejected",
            archive_id: None,
            reason: Some(match reason {
                RejectReason::ReparsePoint => "REPARSE_POINT",
                RejectReason::UnsupportedFileType => "UNSUPPORTED_FILE_TYPE",
                RejectReason::HardLimitExceeded => "HARD_LIMIT_EXCEEDED",
            }),
            bytes: None,
            object_reused: false,
            source_version_reused: false,
        },
        ImportOutcome::Archived(receipt) => {
            let (status, reason) = match receipt.status {
                ArchiveStatus::Archived => ("archived", None),
                ArchiveStatus::ArchivedUnparsed(reason) => {
                    ("archivedUnparsed", Some(encode_unparsed_reason(reason)))
                }
                ArchiveStatus::Extracted => ("extracted", None),
            };
            ImportContextFileView {
                status,
                archive_id: Some(receipt.archive_id),
                reason,
                bytes: None,
                object_reused: receipt.object_reused,
                source_version_reused: receipt.source_version_reused,
            }
        }
    }
}

const fn encode_unparsed_reason(reason: UnparsedReason) -> &'static str {
    match reason {
        UnparsedReason::UnsupportedFormat => "UNSUPPORTED_FORMAT",
        UnparsedReason::InvalidEncoding => "INVALID_ENCODING",
        UnparsedReason::ResourceLimit(_) => "RESOURCE_LIMIT",
        UnparsedReason::InvalidStructure => "INVALID_STRUCTURE",
        UnparsedReason::ParserInterrupted => "PARSER_INTERRUPTED",
    }
}

impl From<&ConversationEvidence> for ConversationTurnView {
    fn from(value: &ConversationEvidence) -> Self {
        let (counterpart_reply_attribution, counterpart_identity_version) =
            match value.counterpart_reply_attribution() {
                Some(CounterpartReplyAttribution::PreIdentityUnbound) => {
                    (Some("PRE_IDENTITY_UNBOUND"), None)
                }
                Some(CounterpartReplyAttribution::IdentityBound(version)) => {
                    (Some("IDENTITY_BOUND"), Some(version))
                }
                None => (None, None),
            };
        Self {
            id: value.id().get(),
            speaker: match value.speaker() {
                Speaker::Person => "person",
                Speaker::Counterpart => "counterpart",
            },
            verbatim: value.verbatim().to_owned(),
            recorded_at_millis: value.recorded_at().as_millis(),
            counterpart_reply_attribution,
            counterpart_identity_version,
        }
    }
}

impl From<&CaptureSpan> for ActivityTimelineEntryView {
    fn from(value: &CaptureSpan) -> Self {
        let (application, window_title, idle, gap_reason) = match value.kind() {
            CaptureSpanKind::Activity(snapshot) => (
                Some(snapshot.application().to_owned()),
                Some(snapshot.window_title().to_owned()),
                Some(matches!(
                    snapshot.idle_state(),
                    eam_capture_windows::IdleState::Idle
                )),
                None,
            ),
            CaptureSpanKind::Gap(reason) => (None, None, None, Some(encode_capture_gap(*reason))),
        };
        Self {
            id: value.id().get(),
            kind: match value.kind() {
                CaptureSpanKind::Activity(_) => "activity",
                CaptureSpanKind::Gap(_) => "gap",
            },
            application,
            window_title,
            idle,
            gap_reason,
            started_at_millis: value.started_at().as_millis(),
            observed_until_millis: value.observed_until().as_millis(),
            ended_at_millis: value.ended_at().map(Timestamp::as_millis),
        }
    }
}

fn persist_capture_checkpoint(
    host: &mut HostCore,
    checkpoint: Option<&CaptureCheckpoint>,
) -> Result<(), String> {
    let Some(checkpoint) = checkpoint else {
        return Ok(());
    };
    let session_id = host
        .lifecycle
        .session_id()
        .ok_or_else(|| "running host has no lifecycle session".to_owned())?;
    host.core
        .repository_mut()
        .record_capture_checkpoint(session_id, checkpoint)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn collect_shutdown_errors(
    capture: Result<(), String>,
    finish: Result<(), String>,
    close: Result<(), String>,
    state: Result<(), String>,
) -> Result<(), Vec<String>> {
    let errors: Vec<_> = [capture, finish, close, state]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

const fn encode_capture_mode(mode: CaptureMode) -> &'static str {
    match mode {
        CaptureMode::Collecting => "collecting",
        CaptureMode::Paused => "paused",
        CaptureMode::Locked => "locked",
        CaptureMode::Stopped => "stopped",
    }
}

const fn encode_capture_gap(reason: CaptureGapReason) -> &'static str {
    match reason {
        CaptureGapReason::Paused => "paused",
        CaptureGapReason::SessionLocked => "sessionLocked",
        CaptureGapReason::ExplicitExit => "explicitExit",
        CaptureGapReason::Update => "update",
        CaptureGapReason::Crash => "crash",
        CaptureGapReason::SourceUnavailable => "sourceUnavailable",
    }
}

const fn encode_host_state(state: HostState) -> &'static str {
    match state {
        HostState::Starting => "starting",
        HostState::Recovering => "recovering",
        HostState::BackgroundRunning => "backgroundRunning",
        HostState::ForegroundRunning => "foregroundRunning",
        HostState::ExitingExplicit => "exitingExplicit",
        HostState::ExitingUpdate => "exitingUpdate",
        HostState::FailedClosed => "failedClosed",
        HostState::Stopped => "stopped",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Mutex, MutexGuard},
    };

    use eam_capture_browser::BrowserSubmissionPayload;
    use eam_core::{
        ClaimOwner, IdentityProfileSnapshot, IdentityRuntimeContext, IdentityStateSnapshot,
        InMemoryRepository, IncrementingClock, RelationalConstraintDeparture, RuntimeResponse,
        ScriptedPersonFactResponse, ScriptedRuntime, SharedAgreementAssent,
        SharedExperienceProposal, Timestamp,
    };
    use eam_identity::{
        IdentityFormation, IdentityProfile, InitialIdentityProposal, IntroductionAnswer,
        ScriptedIdentityRuntime, SelfIntroductionCategory,
    };
    use eam_ingestion::{ArchiveInput, ArchiveReceipt};
    use eam_vault::RecoveryKey;
    use tempfile::tempdir;

    use super::*;

    const TEST_VAULT_KEY: [u8; 32] = [0x73; 32];
    static SQLCIPHER_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn sqlcipher_test_lock() -> MutexGuard<'static, ()> {
        SQLCIPHER_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn ready_identity_context() -> IdentityRuntimeContext {
        IdentityRuntimeContext::new(
            1,
            1,
            IdentityStateSnapshot::restore(
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
                "桌面测试夹具",
                Vec::new(),
                Timestamp::from_millis(1),
            ),
        )
    }

    fn ready_in_memory_repository() -> InMemoryRepository {
        InMemoryRepository::new()
            .with_identity_context(ready_identity_context())
            .unwrap()
    }

    fn seed_ready_counterpart(repository: VaultRepository) -> VaultRepository {
        let answers = [
            IntroductionAnswer::new(
                SelfIntroductionCategory::BasicIdentityAndAddress,
                "我是桌面测试中的本人。",
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::CurrentLife,
                "我正在验证持续对话。",
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::ImportantPeople,
                "测试不包含真实人物资料。",
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::LongTermGoals,
                "保持可信且可追溯。",
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::CurrentConcerns,
                "防止未就绪对话旁路。",
            ),
            IntroductionAnswer::new(
                SelfIntroductionCategory::DesiredReflection,
                "请保留独立判断。",
            ),
        ];
        let proposal = InitialIdentityProposal::new(
            IdentityProfile::new(
                "测试第二自我",
                "清晰表达",
                "保留独立判断",
                "可追溯性优先",
                "共同回看的同行者",
                "帮助本人形成更准确的自我理解",
            ),
            "基于合成介绍形成",
            (1..=6).map(EvidenceId::from_raw).collect(),
        );
        let mut formation = IdentityFormation::new(
            repository,
            ScriptedIdentityRuntime::new([proposal]),
            IncrementingClock::new(1_000),
        );
        formation
            .record_initial_self_introduction(&SessionId::new("desktop-test-onboarding"), &answers)
            .unwrap();
        formation.form_initial_counterpart().unwrap();
        let (repository, _, _) = formation.into_parts();
        repository
    }

    fn ready_vault_repository(path: &Path) -> VaultRepository {
        seed_ready_counterpart(VaultRepository::open(path, VaultKey::new(TEST_VAULT_KEY)).unwrap())
    }

    #[test]
    fn shutdown_collects_every_stage_failure_in_order() {
        let result = collect_shutdown_errors(
            Err("capture failed".to_owned()),
            Err("finish failed".to_owned()),
            Err("close failed".to_owned()),
            Err("state failed".to_owned()),
        );
        assert_eq!(
            result,
            Err(vec![
                "capture failed".to_owned(),
                "finish failed".to_owned(),
                "close failed".to_owned(),
                "state failed".to_owned(),
            ])
        );
    }

    #[test]
    fn shutdown_success_requires_all_stages() {
        assert_eq!(
            collect_shutdown_errors(Ok(()), Ok(()), Ok(()), Ok(())),
            Ok(())
        );
    }

    #[cfg(windows)]
    #[test]
    fn first_run_commits_nothing_until_the_recovery_key_is_confirmed() {
        let _guard = sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let vault_root = directory.path().join("first-run-vault");
        let interrupted = ManagedHost::open(vault_root.clone(), LaunchMode::Foreground, false);

        assert_eq!(interrupted.status().state, "needsInitialization");
        assert!(!interrupted.status().vault_ready);
        let first_view = interrupted.initialize_vault().unwrap();
        assert!(first_view.recovery_key.starts_with("eamrecovery1"));
        assert_eq!(interrupted.status().state, "awaitingRecoveryConfirmation");
        assert!(!vault_root.exists());
        assert!(interrupted.initialize_vault().is_err());
        assert!(interrupted.confirm_recovery_key_saved(false).is_err());
        assert!(!vault_root.exists());
        interrupted.shutdown(ExitReason::Explicit).unwrap();
        assert!(!vault_root.exists());

        let managed = ManagedHost::open(vault_root.clone(), LaunchMode::Foreground, false);
        let recovery_view = managed.initialize_vault().unwrap();
        let recovery_key = RecoveryKey::parse(&recovery_view.recovery_key).unwrap();
        let ready = managed.confirm_recovery_key_saved(true).unwrap();

        assert!(ready.vault_ready);
        assert_eq!(ready.state, "foregroundRunning");
        assert!(vault_root.join("bundle.meta").is_file());
        assert!(vault_root.join("self.db").is_file());
        assert!(managed.list_conversation().unwrap().is_empty());
        managed.shutdown(ExitReason::Explicit).unwrap();

        let recovered_key = VaultKeyStore::unlock_recovery(&vault_root, &recovery_key).unwrap();
        VaultRepository::open(&vault_root, recovered_key)
            .unwrap()
            .close()
            .unwrap();
        let reopened = ManagedHost::open(vault_root, LaunchMode::Foreground, false);
        assert!(reopened.is_ready());
        assert!(reopened.initialize_vault().is_err());
        reopened.shutdown(ExitReason::Explicit).unwrap();
    }

    #[test]
    fn hiding_the_window_keeps_the_same_capture_interval_open() {
        let _guard = sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let mut repository =
            VaultRepository::open(directory.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
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
        let runtime = runtime_from_target(
            RuntimeTarget::new("http://127.0.0.1:1/v1", "unused-test-runtime").unwrap(),
            None,
        )
        .unwrap();
        let managed = ManagedHost {
            inner: Mutex::new(HostSlot::Ready(HostCore {
                core: MemoryCore::new(repository, runtime, SystemClock),
                lifecycle,
                capture: CaptureStateMachine::restore(&recovery),
                host_clock: SystemClock,
            })),
            vault_root: directory.path().to_path_buf(),
            launch_mode: LaunchMode::Foreground,
            updater_configured: false,
        };
        let snapshot = eam_capture_windows::ActivitySnapshot::new(
            "code.exe",
            "S28",
            eam_capture_windows::IdleState::Active,
        )
        .unwrap();

        managed
            .record_capture_sample(NativeCaptureSample::Foreground(snapshot.clone()))
            .unwrap();
        managed.mark_hidden().unwrap();
        managed
            .record_capture_sample(NativeCaptureSample::Foreground(snapshot))
            .unwrap();

        assert_eq!(managed.status().state, "backgroundRunning");
        let timeline = managed.list_activity_timeline().unwrap();
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].kind, "activity");
        assert_eq!(timeline[0].ended_at_millis, None);
    }

    #[test]
    fn browser_submission_uses_only_the_managed_current_host_session() {
        let _guard = sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let mut repository =
            VaultRepository::open(directory.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
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
        let runtime = runtime_from_target(
            RuntimeTarget::new("http://127.0.0.1:1/v1", "unused-test-runtime").unwrap(),
            None,
        )
        .unwrap();
        let managed = ManagedHost {
            inner: Mutex::new(HostSlot::Ready(HostCore {
                core: MemoryCore::new(repository, runtime, SystemClock),
                lifecycle,
                capture: CaptureStateMachine::restore(&recovery),
                host_clock: SystemClock,
            })),
            vault_root: directory.path().to_path_buf(),
            launch_mode: LaunchMode::Foreground,
            updater_configured: false,
        };
        let submission = BrowserSubmission::from_payload(BrowserSubmissionPayload {
            submission_id: "desktop-host-browser-event".to_owned(),
            url: "https://example.test/article".to_owned(),
            title: "Desktop host".to_owned(),
            visited_at_millis: started_at.as_millis(),
            dwell_millis: 0,
            page_content: None,
        })
        .unwrap();

        let first = managed.record_browser_submission(&submission).unwrap();
        let retried = managed.record_browser_submission(&submission).unwrap();

        assert!(!first.reused());
        assert!(retried.reused());
        assert_eq!(first.visit_id(), retried.visit_id());
        managed.shutdown(ExitReason::Explicit).unwrap();
        assert_eq!(
            managed.record_browser_submission(&submission).unwrap_err(),
            "desktop host is already stopped"
        );
    }

    #[test]
    fn identity_history_is_a_fixed_trusted_read_only_projection() {
        let state = IdentityStateSnapshot::restore(
            1,
            None,
            IdentityProfileSnapshot::new(
                "岚",
                "温和、直接",
                "保留分歧",
                "准确高于迎合",
                "同行者",
                "帮助本人看见长期变化",
            ),
            "基于初始自述形成",
            vec![EvidenceId::from_raw(7)],
            Timestamp::from_millis(9_000),
        );
        let repository = InMemoryRepository::new()
            .with_identity_context(IdentityRuntimeContext::new(1, 1, state))
            .unwrap();
        let core = MemoryCore::new(
            repository,
            ScriptedRuntime::default(),
            IncrementingClock::new(10_000),
        );

        let history = list_identity_history_from_core(&core).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].version, 1);
        assert_eq!(history[0].predecessor_version, None);
        assert_eq!(history[0].name, "岚");
        assert_eq!(history[0].change_reason, "基于初始自述形成");
        assert_eq!(history[0].evidence_ids, [7]);
    }

    #[test]
    fn offered_reflection_projection_requires_a_whitelisted_core_decision() {
        let mut repository = InMemoryRepository::new();
        let evidence_id = repository.next_evidence_id();
        repository
            .append_evidence(ConversationEvidence::restore(
                evidence_id,
                SessionId::new(CONTINUOUS_SESSION_ID),
                Speaker::Person,
                "最近我第一次把工作节奏提快了。".to_owned(),
                Timestamp::from_millis(1_000),
            ))
            .unwrap();
        let invitation_id = repository.next_reflection_invitation_id();
        let pending = ReflectionInvitation::restore(
            invitation_id,
            "work-rhythm",
            "这次工作节奏变化值得一起看。",
            vec![EvidenceCitation::new(evidence_id, "第一次把工作节奏提快了")],
            "这是刚发生且有直接依据的重要变化。",
            ReflectionImportance::Important,
            ReflectionInvitationBasis::ImportantSingleChange,
            ReflectionInvitationState::Pending,
            Timestamp::from_millis(2_000),
            Timestamp::from_millis(2_000),
            None,
            None,
            0,
            false,
        );
        repository
            .commit_reflection_invitation(pending.clone())
            .unwrap();
        let first_offer =
            eam_core::offer_reflection_invitation(&pending, Timestamp::from_millis(3_000)).unwrap();
        repository
            .transition_reflection_invitation(
                ReflectionInvitationState::Pending,
                first_offer.clone(),
            )
            .unwrap();
        let first_defer = eam_core::decide_reflection_invitation(
            &first_offer,
            ReflectionDecision::Defer,
            Timestamp::from_millis(4_000),
        )
        .unwrap();
        repository
            .transition_reflection_invitation(
                ReflectionInvitationState::Offered,
                first_defer.clone(),
            )
            .unwrap();
        let second_offer = eam_core::offer_reflection_invitation(
            &first_defer,
            Timestamp::from_millis(4_000 + eam_core::REFLECTION_DEFER_MILLIS),
        )
        .unwrap();
        repository
            .transition_reflection_invitation(ReflectionInvitationState::Deferred, second_offer)
            .unwrap();
        let mut core = MemoryCore::new(
            repository,
            ScriptedRuntime::default(),
            IncrementingClock::new(5_000 + eam_core::REFLECTION_DEFER_MILLIS),
        );

        let invitations = list_offered_reflection_invitations_from_core(&core).unwrap();
        assert_eq!(invitations.len(), 1);
        assert_eq!(invitations[0].observation, "这次工作节奏变化值得一起看。");
        assert_eq!(invitations[0].evidence[0].evidence_id, evidence_id.get());
        assert!(invitations[0].show_mute_prompt);

        assert!(
            decide_reflection_invitation_from_core(&mut core, invitation_id.get(), "delete")
                .is_err()
        );
        assert_eq!(
            core.repository()
                .reflection_invitation(invitation_id)
                .unwrap()
                .unwrap()
                .state(),
            ReflectionInvitationState::Offered
        );

        let decision =
            decide_reflection_invitation_from_core(&mut core, invitation_id.get(), "mute").unwrap();
        assert_eq!(decision.state, "mutedByPerson");
        assert!(
            list_offered_reflection_invitations_from_core(&core)
                .unwrap()
                .is_empty()
        );
        let retained = core
            .repository()
            .reflection_invitation(invitation_id)
            .unwrap()
            .unwrap();
        assert_eq!(retained.observation(), "这次工作节奏变化值得一起看。");
        assert_eq!(retained.evidence_refs()[0].evidence_id(), evidence_id);
    }

    #[test]
    fn person_topic_reentry_reaches_muted_reflection_as_discuss_only() {
        let mut repository = ready_in_memory_repository();
        let evidence_id = repository.next_evidence_id();
        repository
            .append_evidence(ConversationEvidence::restore(
                evidence_id,
                SessionId::new(CONTINUOUS_SESSION_ID),
                Speaker::Person,
                "工作节奏最近发生了重要变化。".to_owned(),
                Timestamp::from_millis(1_000),
            ))
            .unwrap();
        let invitation_id = repository.next_reflection_invitation_id();
        let pending = ReflectionInvitation::restore(
            invitation_id,
            "工作节奏",
            "这次工作节奏变化值得一起看。",
            vec![EvidenceCitation::new(
                evidence_id,
                "工作节奏最近发生了重要变化",
            )],
            "这是一项有直接依据的重要变化。",
            ReflectionImportance::Important,
            ReflectionInvitationBasis::ImportantSingleChange,
            ReflectionInvitationState::Pending,
            Timestamp::from_millis(2_000),
            Timestamp::from_millis(2_000),
            None,
            None,
            0,
            false,
        );
        repository
            .commit_reflection_invitation(pending.clone())
            .unwrap();
        let offered =
            eam_core::offer_reflection_invitation(&pending, Timestamp::from_millis(3_000)).unwrap();
        repository
            .transition_reflection_invitation(ReflectionInvitationState::Pending, offered.clone())
            .unwrap();
        let muted = eam_core::decide_reflection_invitation(
            &offered,
            ReflectionDecision::Mute,
            Timestamp::from_millis(4_000),
        )
        .unwrap();
        repository
            .transition_reflection_invitation(ReflectionInvitationState::Offered, muted)
            .unwrap();
        let runtime = ScriptedRuntime::new(
            [ScriptedPersonFactResponse::NoFacts],
            [RuntimeResponse::new(
                "可以，我们按你现在主动提起的方向继续谈。",
            )],
        );
        let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(5_000));

        let result =
            send_message_with_core(&mut core, "我想继续聊聊工作节奏。".to_owned()).unwrap();

        assert!(result.reflection_invitations.is_empty());
        assert_eq!(
            core.runtime().seen_requests()[0]
                .reflection()
                .unwrap()
                .disposition(),
            eam_core::ReflectionRuntimeDisposition::DiscussOnly
        );
        assert_eq!(
            core.repository()
                .reflection_invitation(invitation_id)
                .unwrap()
                .unwrap()
                .state(),
            ReflectionInvitationState::MutedByPerson
        );
    }

    #[test]
    fn ordinary_conversation_survives_sqlcipher_reopen_without_ordinary_claims() {
        let _guard = sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let repository = ready_vault_repository(directory.path());
        let runtime = ScriptedRuntime::new(
            [ScriptedPersonFactResponse::NoFacts],
            [RuntimeResponse::new("我会记得这段原话。")],
        );
        let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(10_000));
        let baseline_claim_count = core.repository().all_claims().unwrap().len();

        let result =
            send_message_with_retrieval(&mut core, "你会记得这句话吗？".to_owned()).unwrap();
        assert_eq!(result.person.verbatim, "你会记得这句话吗？");
        assert_eq!(result.counterpart.verbatim, "我会记得这段原话。");
        assert_eq!(
            core.repository().all_claims().unwrap().len(),
            baseline_claim_count
        );
        assert_eq!(
            core.runtime().seen_requests()[0]
                .working_context()
                .retrieval_snapshot()
                .unwrap()
                .retrieval_contract_version(),
            eam_retrieval::RETRIEVAL_INDEX_VERSION
        );

        let (repository, _, _) = core.into_parts();
        repository.close().unwrap();
        let repository =
            VaultRepository::open(directory.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
        let core = MemoryCore::new(
            repository,
            ScriptedRuntime::default(),
            IncrementingClock::new(20_000),
        );

        let restored = list_conversation_from_core(&core).unwrap();
        assert_eq!(restored, vec![result.person, result.counterpart]);
        assert_eq!(
            core.repository().all_claims().unwrap().len(),
            baseline_claim_count
        );
        let (repository, _, _) = core.into_parts();
        repository.close().unwrap();
    }

    #[test]
    fn non_searchable_message_still_reaches_the_runtime_without_retrieval_failure() {
        let _guard = sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let repository = ready_vault_repository(directory.path());
        let runtime = ScriptedRuntime::new(
            [ScriptedPersonFactResponse::NoFacts],
            [RuntimeResponse::new("🙂")],
        );
        let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(25_000));

        let result = send_message_with_retrieval(&mut core, "😊".to_owned()).unwrap();
        assert_eq!(result.person.verbatim, "😊");
        assert_eq!(result.counterpart.verbatim, "🙂");
        assert!(
            core.runtime().seen_requests()[0]
                .working_context()
                .retrieval_snapshot()
                .is_none()
        );
        let (repository, _, _) = core.into_parts();
        repository.close().unwrap();
    }

    #[test]
    fn rejects_blank_and_oversized_messages_before_persistence() {
        let mut core = MemoryCore::new(
            InMemoryRepository::new(),
            ScriptedRuntime::default(),
            IncrementingClock::new(30_000),
        );

        assert!(send_message_with_core(&mut core, "   ".to_owned()).is_err());
        assert!(send_message_with_core(&mut core, "x".repeat(MAX_MESSAGE_BYTES + 1)).is_err());
        assert!(core.repository().all_evidence().unwrap().is_empty());
    }

    #[test]
    fn send_message_fails_closed_before_counterpart_creation() {
        let mut core = MemoryCore::new(
            InMemoryRepository::new(),
            ScriptedRuntime::new(
                [ScriptedPersonFactResponse::NoFacts],
                [RuntimeResponse::new("这条回复不应被调用。")],
            ),
            IncrementingClock::new(35_000),
        );

        send_message_with_core(&mut core, "这条本人消息不应落盘。".to_owned())
            .expect_err("desktop send must fail closed before counterpart creation");

        assert!(core.repository().all_evidence().unwrap().is_empty());
        assert!(core.repository().all_claims().unwrap().is_empty());
        assert!(core.runtime().seen_person_fact_inputs().is_empty());
        assert!(core.runtime().seen_requests().is_empty());
    }

    #[test]
    fn later_turn_receives_prior_continuous_conversation_as_frozen_context() {
        let runtime = ScriptedRuntime::new(
            [
                ScriptedPersonFactResponse::NoFacts,
                ScriptedPersonFactResponse::NoFacts,
            ],
            [
                RuntimeResponse::new("第一答"),
                RuntimeResponse::new("第二答"),
            ],
        );
        let mut core = MemoryCore::new(
            ready_in_memory_repository(),
            runtime,
            IncrementingClock::new(40_000),
        );

        send_message_with_core(&mut core, "第一问".to_owned()).unwrap();
        send_message_with_core(&mut core, "第二问".to_owned()).unwrap();

        let requests = core.runtime().seen_requests();
        assert!(requests[0].working_context().evidence().is_empty());
        assert_eq!(
            requests[1]
                .working_context()
                .evidence()
                .iter()
                .map(ConversationEvidence::verbatim)
                .collect::<Vec<_>>(),
            vec!["第一问", "第一答"]
        );
    }

    #[test]
    fn agreement_ceremony_is_trusted_state_and_confirmation_writes_shared_claim() {
        let response = RuntimeResponse::new("我也同意直接指出关键逃避。").with_shared_experience(
            SharedExperienceProposal::new(
                SharedExperienceKind::Agreement,
                "发现关键逃避时直接指出",
                vec![EvidenceCitation::new(
                    EvidenceId::from_raw(1),
                    "我同意直接指出关键逃避",
                )],
                "我也同意直接指出关键逃避",
                Timestamp::from_millis(50_000),
            )
            .with_agreement_terms(
                "双方的重要议题讨论",
                Timestamp::from_millis(51_000),
                None,
                None,
            ),
        );
        let mut core = MemoryCore::new(
            ready_in_memory_repository(),
            ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [response]),
            IncrementingClock::new(50_000),
        );

        let result =
            send_message_with_core(&mut core, "我同意直接指出关键逃避。".to_owned()).unwrap();
        assert_eq!(result.ceremonies.len(), 1);
        let ceremony = &result.ceremonies[0];
        assert_eq!(ceremony.target_kind, "agreementCandidate");
        assert_eq!(ceremony.admission, "confirmationRequired");
        assert_eq!(ceremony.candidate_version, Some(1));
        assert_eq!(ceremony.scope.as_deref(), Some("双方的重要议题讨论"));
        assert_eq!(ceremony.effective_from_millis, Some(51_000));
        assert_eq!(ceremony.effective_until_millis, None);
        assert_eq!(ceremony.end_condition, None);
        assert_eq!(ceremony.evidence.len(), 2);
        assert!(core.repository().all_claims().unwrap().is_empty());

        let resolution =
            resolve_shared_agreement_from_core(&mut core, ceremony.target_id, true).unwrap();
        assert_eq!(resolution.status, "confirmed");
        assert_eq!(
            core.repository()
                .all_claims()
                .unwrap()
                .iter()
                .filter(|claim| claim.owner() == ClaimOwner::Shared)
                .count(),
            1
        );
        assert!(
            list_shared_experience_ceremonies_from_core(&core)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn superseding_agreement_ceremony_lists_every_displaced_agreement_boundary() {
        let original = RuntimeResponse::new("我同意复盘时直接指出关键逃避。")
            .with_shared_experience(
                SharedExperienceProposal::new(
                    SharedExperienceKind::Agreement,
                    "复盘时直接指出关键逃避",
                    vec![EvidenceCitation::new(
                        EvidenceId::from_raw(1),
                        "我同意复盘时直接指出关键逃避",
                    )],
                    "我同意复盘时直接指出关键逃避",
                    Timestamp::from_millis(50_000),
                )
                .with_agreement_terms(
                    "双方共同项目复盘",
                    Timestamp::from_millis(50_000),
                    None,
                    None,
                ),
            );
        let replacement = RuntimeResponse::new("我同意新约定整份取代旧约定。")
            .with_shared_experience(
                SharedExperienceProposal::new(
                    SharedExperienceKind::Agreement,
                    "复盘时不要直接指出关键逃避",
                    vec![EvidenceCitation::new(
                        EvidenceId::from_raw(3),
                        "我同意新约定整份取代旧约定",
                    )],
                    "我同意新约定整份取代旧约定",
                    Timestamp::from_millis(53_000),
                )
                .with_agreement_terms(
                    "双方共同项目复盘",
                    Timestamp::from_millis(55_000),
                    None,
                    None,
                )
                .with_superseded_agreements(vec![ClaimId::from_raw(1)]),
            );
        let mut core = MemoryCore::new(
            ready_in_memory_repository(),
            ScriptedRuntime::new(
                [
                    ScriptedPersonFactResponse::NoFacts,
                    ScriptedPersonFactResponse::NoFacts,
                ],
                [original, replacement],
            ),
            IncrementingClock::new(50_000),
        );

        let first =
            send_message_with_core(&mut core, "我同意复盘时直接指出关键逃避。".to_owned()).unwrap();
        resolve_shared_agreement_from_core(&mut core, first.ceremonies[0].target_id, true).unwrap();
        let second = send_message_with_core(
            &mut core,
            "共同项目复盘时，我同意新约定整份取代旧约定。".to_owned(),
        )
        .unwrap();

        assert_eq!(second.ceremonies.len(), 1);
        let ceremony = &second.ceremonies[0];
        assert_eq!(ceremony.superseded_agreements.len(), 1);
        let displaced = &ceremony.superseded_agreements[0];
        assert_eq!(displaced.claim_id, 1);
        assert_eq!(displaced.statement, "复盘时直接指出关键逃避");
        assert_eq!(displaced.scope, "双方共同项目复盘");
        assert_eq!(displaced.effective_from_millis, 50_000);
        assert_eq!(displaced.effective_until_millis, None);
    }

    #[test]
    fn relevant_agreement_is_sent_and_its_departure_returns_a_trusted_notice() {
        let agreement = RuntimeResponse::new("我也同意复盘时直接指出关键逃避。")
            .with_shared_experience(
                SharedExperienceProposal::new(
                    SharedExperienceKind::Agreement,
                    "复盘时直接指出关键逃避",
                    vec![EvidenceCitation::new(
                        EvidenceId::from_raw(1),
                        "我同意复盘时直接指出关键逃避",
                    )],
                    "我也同意复盘时直接指出关键逃避",
                    Timestamp::from_millis(80_000),
                )
                .with_agreement_terms(
                    "双方共同项目复盘",
                    Timestamp::from_millis(80_000),
                    None,
                    None,
                ),
            );
        let reason = "因为安全边界禁止把约定当作现实行动授权";
        let departure = RuntimeResponse::new(format!("这次会偏离约定，{reason}。"))
            .with_relational_constraint_departure(RelationalConstraintDeparture::new(
                ClaimId::from_raw(1),
                reason,
            ));
        let mut core = MemoryCore::new(
            ready_in_memory_repository(),
            ScriptedRuntime::new(
                [
                    ScriptedPersonFactResponse::NoFacts,
                    ScriptedPersonFactResponse::NoFacts,
                ],
                [agreement, departure],
            ),
            IncrementingClock::new(80_000),
        );

        let first =
            send_message_with_core(&mut core, "我同意复盘时直接指出关键逃避。".to_owned()).unwrap();
        resolve_shared_agreement_from_core(&mut core, first.ceremonies[0].target_id, true).unwrap();
        let second =
            send_message_with_core(&mut core, "这次共同项目复盘请替我执行现实操作。".to_owned())
                .unwrap();

        assert_eq!(
            core.runtime().seen_requests()[1]
                .working_context()
                .active_relational_constraints()
                .len(),
            1
        );
        assert_eq!(second.ceremonies.len(), 1);
        let ceremony = &second.ceremonies[0];
        assert_eq!(ceremony.experience_kind, "agreementBreach");
        assert_eq!(ceremony.admission, "nonVetoNotice");
        assert_eq!(ceremony.agreement_claim_id, Some(1));
        assert_eq!(ceremony.departure_reason.as_deref(), Some(reason));
    }

    #[test]
    fn person_withdrawal_requires_confirmation_and_allows_no_reason() {
        let agreement = RuntimeResponse::new("我也同意复盘时直接指出关键逃避。")
            .with_shared_experience(
                SharedExperienceProposal::new(
                    SharedExperienceKind::Agreement,
                    "复盘时直接指出关键逃避",
                    vec![EvidenceCitation::new(
                        EvidenceId::from_raw(1),
                        "我同意复盘时直接指出关键逃避",
                    )],
                    "我也同意复盘时直接指出关键逃避",
                    Timestamp::from_millis(90_000),
                )
                .with_agreement_terms(
                    "双方共同项目复盘",
                    Timestamp::from_millis(90_000),
                    None,
                    None,
                ),
            );
        let mut core = MemoryCore::new(
            ready_in_memory_repository(),
            ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [agreement]),
            IncrementingClock::new(90_000),
        );
        let first =
            send_message_with_core(&mut core, "我同意复盘时直接指出关键逃避。".to_owned()).unwrap();
        let agreement_claim_id =
            resolve_shared_agreement_from_core(&mut core, first.ceremonies[0].target_id, true)
                .unwrap()
                .claim_id
                .unwrap();

        assert_eq!(
            withdraw_shared_agreement_as_person_from_core(
                &mut core,
                agreement_claim_id,
                false,
                None,
            )
            .unwrap(),
            None
        );
        assert_eq!(
            list_active_shared_agreements_from_core(&core, Timestamp::from_millis(100_000),)
                .unwrap()
                .len(),
            1
        );

        assert!(
            withdraw_shared_agreement_as_person_from_core(
                &mut core,
                agreement_claim_id,
                true,
                Some("   ".to_owned()),
            )
            .unwrap()
            .is_some()
        );
        let ceremonies = list_shared_experience_ceremonies_from_core(&core).unwrap();
        assert_eq!(ceremonies.len(), 1);
        assert_eq!(ceremonies[0].experience_kind, "agreementWithdrawal");
        assert_eq!(ceremonies[0].admission, "nonVetoNotice");
        assert_eq!(ceremonies[0].agreement_claim_id, Some(agreement_claim_id));
        assert_eq!(ceremonies[0].departure_reason, None);
        assert_eq!(ceremonies[0].withdrawal_actor, Some("person"));
        assert!(
            list_active_shared_agreements_from_core(&core, Timestamp::from_millis(100_000),)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn person_revision_waits_for_counterpart_before_returning_as_a_signable_ceremony() {
        let initial = RuntimeResponse::new("我同意第一版。").with_shared_experience(
            SharedExperienceProposal::new(
                SharedExperienceKind::Agreement,
                "复盘时直接指出关键逃避",
                vec![EvidenceCitation::new(
                    EvidenceId::from_raw(1),
                    "我同意第一版",
                )],
                "我同意第一版",
                Timestamp::from_millis(70_000),
            )
            .with_agreement_terms(
                "共同项目复盘",
                Timestamp::from_millis(71_000),
                None,
                None,
            ),
        );
        let assent = RuntimeResponse::new("我明确接受第二版全部边界。")
            .with_shared_agreement_assent(SharedAgreementAssent::new(
                eam_core::SharedAgreementCandidateId::from_raw(2),
                2,
                "我明确接受第二版全部边界",
            ));
        let mut core = MemoryCore::new(
            ready_in_memory_repository(),
            ScriptedRuntime::new(
                [
                    ScriptedPersonFactResponse::NoFacts,
                    ScriptedPersonFactResponse::NoFacts,
                ],
                [initial, assent],
            ),
            IncrementingClock::new(70_000),
        );

        let first = send_message_with_core(&mut core, "我同意第一版。".to_owned()).unwrap();
        let first_id = first.ceremonies[0].target_id;
        let revision = revise_shared_agreement_from_core(
            &mut core,
            first_id,
            "只在正式复盘时直接指出关键逃避".to_owned(),
            "双方共同项目的正式复盘".to_owned(),
            72_000,
            None,
            None,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(revision.candidate_id, 2);
        assert_eq!(revision.version, 2);
        assert_eq!(revision.status, "awaitingCounterpart");
        assert!(
            list_shared_experience_ceremonies_from_core(&core)
                .unwrap()
                .is_empty()
        );

        let second = send_message_with_core(&mut core, "请核对第二版。".to_owned()).unwrap();
        assert_eq!(second.ceremonies.len(), 1);
        let ceremony = &second.ceremonies[0];
        assert_eq!(ceremony.target_id, 2);
        assert_eq!(ceremony.candidate_version, Some(2));
        assert_eq!(ceremony.scope.as_deref(), Some("双方共同项目的正式复盘"));
        assert_eq!(ceremony.effective_from_millis, Some(72_000));
        assert_eq!(ceremony.evidence.len(), 2);
    }

    #[test]
    fn disagreement_notice_can_close_without_removing_shared_history() {
        let response = RuntimeResponse::new("我不同意把它视为无关紧要。").with_shared_experience(
            SharedExperienceProposal::new(
                SharedExperienceKind::SubstantiveDisagreement,
                "双方对这件事的重要性持不相容立场",
                vec![EvidenceCitation::new(
                    EvidenceId::from_raw(1),
                    "这件事无关紧要",
                )],
                "我不同意把它视为无关紧要",
                Timestamp::from_millis(60_000),
            ),
        );
        let mut core = MemoryCore::new(
            ready_in_memory_repository(),
            ScriptedRuntime::new([ScriptedPersonFactResponse::NoFacts], [response]),
            IncrementingClock::new(60_000),
        );

        let result = send_message_with_core(&mut core, "这件事无关紧要。".to_owned()).unwrap();
        assert_eq!(result.ceremonies.len(), 1);
        let ceremony = &result.ceremonies[0];
        assert_eq!(ceremony.experience_kind, "substantiveDisagreement");
        assert_eq!(ceremony.admission, "nonVetoNotice");
        dismiss_shared_experience_ceremony_from_core(&mut core, ceremony.target_id).unwrap();

        assert!(
            list_shared_experience_ceremonies_from_core(&core)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            core.repository()
                .all_claims()
                .unwrap()
                .iter()
                .filter(|claim| claim.owner() == ClaimOwner::Shared)
                .count(),
            1
        );
    }

    #[derive(Default)]
    struct RecordingArchiveRepository {
        statuses: Vec<ArchiveStatus>,
    }

    impl ArchiveRepository for RecordingArchiveRepository {
        type Error = std::io::Error;

        fn archive(&mut self, input: ArchiveInput<'_>) -> Result<ArchiveReceipt, Self::Error> {
            self.statuses.push(input.status);
            Ok(ArchiveReceipt {
                archive_id: u64::try_from(self.statuses.len()).unwrap(),
                status: input.status,
                object_reused: false,
                source_version_reused: false,
            })
        }
    }

    fn zero_window_policy(auto_limit: u64, hard_limit: u64) -> ImportPolicy {
        ImportPolicy {
            stability_window: Duration::ZERO,
            auto_import_limit_bytes: auto_limit,
            hard_import_limit_bytes: hard_limit,
        }
    }

    #[test]
    fn bounded_import_view_distinguishes_archived_and_unsupported_files() {
        let directory = tempdir().unwrap();
        let markdown = directory.path().join("context.md");
        let binary = directory.path().join("context.bin");
        fs::write(&markdown, b"# context").unwrap();
        fs::write(&binary, b"opaque").unwrap();
        let mut repository = RecordingArchiveRepository::default();
        let policy = zero_window_policy(1024, 2048);

        let archived =
            import_context_file_with_policy(&mut repository, &markdown, &policy, false, 1).unwrap();
        let unsupported =
            import_context_file_with_policy(&mut repository, &binary, &policy, false, 2).unwrap();

        assert_eq!(archived.status, "archived");
        assert_eq!(archived.archive_id, Some(1));
        assert_eq!(archived.reason, None);
        assert_eq!(unsupported.status, "archivedUnparsed");
        assert_eq!(unsupported.archive_id, Some(2));
        assert_eq!(unsupported.reason, Some("UNSUPPORTED_FORMAT"));
    }

    #[test]
    fn import_view_maps_s09_terminal_states_without_exposing_content() {
        let extracted = import_context_file_view(ImportOutcome::Archived(ArchiveReceipt {
            archive_id: 7,
            status: ArchiveStatus::Extracted,
            object_reused: false,
            source_version_reused: false,
        }));
        let rejected = import_context_file_view(ImportOutcome::Archived(ArchiveReceipt {
            archive_id: 8,
            status: ArchiveStatus::ArchivedUnparsed(UnparsedReason::ParserInterrupted),
            object_reused: false,
            source_version_reused: false,
        }));

        assert_eq!(extracted.status, "extracted");
        assert_eq!(extracted.reason, None);
        assert_eq!(rejected.status, "archivedUnparsed");
        assert_eq!(rejected.reason, Some("PARSER_INTERRUPTED"));
    }

    #[test]
    fn bounded_import_view_exposes_wait_and_rejection_without_archiving() {
        let directory = tempdir().unwrap();
        let oversized = directory.path().join("large.md");
        fs::write(&oversized, vec![0_u8; 5]).unwrap();
        let mut repository = RecordingArchiveRepository::default();
        let policy = zero_window_policy(4, 8);

        let waiting =
            import_context_file_with_policy(&mut repository, &oversized, &policy, false, 3)
                .unwrap();
        let rejected =
            import_context_file_with_policy(&mut repository, directory.path(), &policy, false, 4)
                .unwrap();

        assert_eq!(waiting.status, "awaitingApproval");
        assert_eq!(waiting.bytes, Some(5));
        assert_eq!(rejected.status, "rejected");
        assert_eq!(rejected.reason, Some("UNSUPPORTED_FILE_TYPE"));
        assert!(repository.statuses.is_empty());
    }

    mod counterpart_creation;
    mod runtime_profile;
}
