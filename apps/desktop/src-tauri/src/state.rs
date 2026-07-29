use std::{
    env,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use eam_core::{
    Clock, ConversationEvidence, CounterpartRuntime, EvidenceId, MemoryCore, MemoryRepository,
    SessionId, Speaker, SystemClock,
};
use eam_desktop_host::{ExitReason, HostLifecycle, HostLifecycleRepository, HostState, LaunchMode};
use eam_runtime_gateway::{
    FallbackRuntime, HttpResponsesTransport, OpenAiResponsesRuntime, RuntimeTarget,
};
use eam_vault::{VaultKeyStore, VaultRepository};
use serde::Serialize;

const LOCAL_RESPONSES_ENDPOINT: &str = "http://127.0.0.1:11434/v1/responses";
const CLOUD_RESPONSES_ENDPOINT: &str = "https://api.openai.com/v1/responses";
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(45);
const CONTINUOUS_SESSION_ID: &str = "continuous-conversation";
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_CONTEXT_TURNS: usize = 32;
const MAX_CONTEXT_BYTES: usize = 64 * 1024;

type AppRuntime = Box<dyn CounterpartRuntime + Send>;
type AppCore = MemoryCore<VaultRepository, AppRuntime, SystemClock>;

pub struct ManagedHost {
    inner: Mutex<HostSlot>,
    vault_root: PathBuf,
    updater_configured: bool,
}

enum HostSlot {
    Ready(HostCore),
    Locked(String),
    FailedClosed(String),
    Closed,
}

struct HostCore {
    core: AppCore,
    lifecycle: HostLifecycle,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnView {
    id: u64,
    speaker: &'static str,
    verbatim: String,
    recorded_at_millis: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnResult {
    person: ConversationTurnView,
    counterpart: ConversationTurnView,
}

impl ManagedHost {
    #[must_use]
    pub fn open(vault_root: PathBuf, launch_mode: LaunchMode, updater_configured: bool) -> Self {
        let slot =
            HostCore::open(&vault_root, launch_mode).map_or_else(HostSlot::Locked, HostSlot::Ready);
        Self {
            inner: Mutex::new(slot),
            vault_root,
            updater_configured,
        }
    }

    pub fn status(&self) -> HostStatusView {
        match &*self.lock() {
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

    pub fn mark_hidden(&self) -> Result<(), String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => host
                .lifecycle
                .hide_window()
                .map_err(|error| error.to_string()),
            HostSlot::Locked(_) | HostSlot::FailedClosed(_) => Ok(()),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn mark_visible(&self) -> Result<(), String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => host
                .lifecycle
                .show_window()
                .map_err(|error| error.to_string()),
            HostSlot::Locked(_) | HostSlot::FailedClosed(_) => Ok(()),
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
            HostSlot::Locked(_) | HostSlot::FailedClosed(_) => Ok(()),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn list_conversation(&self) -> Result<Vec<ConversationTurnView>, String> {
        match &*self.lock() {
            HostSlot::Ready(host) => list_conversation_from_core(&host.core),
            HostSlot::Locked(detail) => Err(format!("vault is locked: {detail}")),
            HostSlot::FailedClosed(detail) => Err(format!("Core is closed: {detail}")),
            HostSlot::Closed => Err("desktop host is already stopped".to_owned()),
        }
    }

    pub fn send_message(&self, verbatim: String) -> Result<ConversationTurnResult, String> {
        match &mut *self.lock() {
            HostSlot::Ready(host) => send_message_with_core(&mut host.core, verbatim),
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
        collect_shutdown_errors(finish_result, close_result, state_result)
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
        let runtime = configured_runtime()?;
        let vault_key =
            VaultKeyStore::unlock_local(vault_root).map_err(|error| error.to_string())?;
        let mut repository =
            VaultRepository::open(vault_root, vault_key).map_err(|error| error.to_string())?;
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
        lifecycle
            .complete_recovery(start.session().id(), launch_mode)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            core: MemoryCore::new(repository, runtime, SystemClock),
            lifecycle,
            host_clock,
        })
    }
}

fn configured_runtime() -> Result<AppRuntime, String> {
    let local_endpoint = env::var("EAM_LOCAL_RESPONSES_ENDPOINT")
        .unwrap_or_else(|_| LOCAL_RESPONSES_ENDPOINT.to_owned());
    let local = OpenAiResponsesRuntime::new(
        RuntimeTarget::openai_local(local_endpoint),
        HttpResponsesTransport::openai_local().map_err(|error| error.to_string())?,
        RUNTIME_TIMEOUT,
    );

    let token = env::var("OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty());
    match token {
        None => Ok(Box::new(local)),
        Some(token) => {
            let cloud_endpoint = env::var("EAM_CLOUD_RESPONSES_ENDPOINT")
                .unwrap_or_else(|_| CLOUD_RESPONSES_ENDPOINT.to_owned());
            let cloud = OpenAiResponsesRuntime::new(
                RuntimeTarget::openai_cloud(cloud_endpoint),
                HttpResponsesTransport::openai_cloud(token).map_err(|error| error.to_string())?,
                RUNTIME_TIMEOUT,
            );
            Ok(Box::new(FallbackRuntime::new(cloud, local)))
        }
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

fn send_message_with_core<R, T, C>(
    core: &mut MemoryCore<R, T, C>,
    verbatim: String,
) -> Result<ConversationTurnResult, String>
where
    R: MemoryRepository,
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
    let outcome = core
        .run_counterpart_turn(
            SessionId::new(CONTINUOUS_SESSION_ID),
            verbatim,
            working_context,
        )
        .map_err(|error| error.to_string())?;
    let person = conversation_turn(core, outcome.person_evidence_id())?;
    let counterpart = conversation_turn(core, outcome.counterpart_evidence_id())?;
    Ok(ConversationTurnResult {
        person,
        counterpart,
    })
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

impl From<&ConversationEvidence> for ConversationTurnView {
    fn from(value: &ConversationEvidence) -> Self {
        Self {
            id: value.id().get(),
            speaker: match value.speaker() {
                Speaker::Person => "person",
                Speaker::Counterpart => "counterpart",
            },
            verbatim: value.verbatim().to_owned(),
            recorded_at_millis: value.recorded_at().as_millis(),
        }
    }
}

fn collect_shutdown_errors(
    finish: Result<(), String>,
    close: Result<(), String>,
    state: Result<(), String>,
) -> Result<(), Vec<String>> {
    let errors: Vec<_> = [finish, close, state]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
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
    use std::sync::{Mutex, MutexGuard};

    use eam_core::{
        InMemoryRepository, IncrementingClock, PersonTurnClassification, RuntimeResponse,
        ScriptedRuntime,
    };
    use eam_vault::VaultKey;
    use tempfile::tempdir;

    use super::*;

    const TEST_VAULT_KEY: [u8; 32] = [0x73; 32];
    static SQLCIPHER_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn sqlcipher_test_lock() -> MutexGuard<'static, ()> {
        SQLCIPHER_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn shutdown_collects_every_stage_failure_in_order() {
        let result = collect_shutdown_errors(
            Err("finish failed".to_owned()),
            Err("close failed".to_owned()),
            Err("state failed".to_owned()),
        );
        assert_eq!(
            result,
            Err(vec![
                "finish failed".to_owned(),
                "close failed".to_owned(),
                "state failed".to_owned(),
            ])
        );
    }

    #[test]
    fn shutdown_success_requires_all_stages() {
        assert_eq!(collect_shutdown_errors(Ok(()), Ok(()), Ok(())), Ok(()));
    }

    #[test]
    fn ordinary_conversation_survives_sqlcipher_reopen_without_claims() {
        let _guard = sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let repository =
            VaultRepository::open(directory.path(), VaultKey::new(TEST_VAULT_KEY)).unwrap();
        let runtime = ScriptedRuntime::new(
            [PersonTurnClassification::Question],
            [RuntimeResponse::new("我会记得这段原话。")],
        );
        let mut core = MemoryCore::new(repository, runtime, IncrementingClock::new(10_000));

        let result = send_message_with_core(&mut core, "你会记得这句话吗？".to_owned()).unwrap();
        assert_eq!(result.person.verbatim, "你会记得这句话吗？");
        assert_eq!(result.counterpart.verbatim, "我会记得这段原话。");
        assert!(core.repository().all_claims().unwrap().is_empty());

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
        assert!(core.repository().all_claims().unwrap().is_empty());
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
    fn later_turn_receives_prior_continuous_conversation_as_frozen_context() {
        let runtime = ScriptedRuntime::new(
            [
                PersonTurnClassification::Question,
                PersonTurnClassification::Question,
            ],
            [
                RuntimeResponse::new("第一答"),
                RuntimeResponse::new("第二答"),
            ],
        );
        let mut core = MemoryCore::new(
            InMemoryRepository::new(),
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
}
