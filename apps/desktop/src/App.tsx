import { invoke } from "@tauri-apps/api/core";
import { FormEvent, useEffect, useRef, useState } from "react";

type Speaker = "person" | "counterpart";

export interface ConversationTurn {
  id: number;
  speaker: Speaker;
  verbatim: string;
  recordedAtMillis: number;
}

interface ConversationTurnResult {
  person: ConversationTurn;
  counterpart: ConversationTurn;
  ceremonies: SharedExperienceCeremony[];
  reflectionInvitations?: ReflectionInvitationCeremony[];
}

type ReflectionInvitationDecision = "defer" | "mute" | "resolve";

export interface ReflectionInvitationCeremony {
  id: number;
  topicKey: string;
  observation: string;
  whyNow: string;
  importance: "ordinary" | "important" | "immediateSafetyRisk";
  basis: "importantSingleChange" | "repeatedPattern";
  deferCount: number;
  showMutePrompt: boolean;
  evidence: Array<{
    evidenceId: number;
    speaker: Speaker;
    quote: string;
  }>;
}

type SharedExperienceKind =
  | "agreement"
  | "substantiveDisagreement"
  | "relationshipChange"
  | "sharedAchievement"
  | "agreementBreach"
  | "agreementWithdrawal";

type AgreementWithdrawalActor = "person" | "counterpart";

export interface SharedExperienceCeremony {
  targetId: number;
  targetKind: "agreementCandidate" | "sharedExperience";
  experienceKind: SharedExperienceKind;
  admission: "confirmationRequired" | "nonVetoNotice";
  statement: string;
  candidateVersion: number | null;
  scope: string | null;
  effectiveFromMillis: number | null;
  effectiveUntilMillis: number | null;
  endCondition: string | null;
  agreementClaimId: number | null;
  departureReason: string | null;
  withdrawalActor: AgreementWithdrawalActor | null;
  supersededAgreements: Array<{
    claimId: number;
    statement: string;
    scope: string;
    effectiveFromMillis: number;
    effectiveUntilMillis: number | null;
  }>;
  evidence: Array<{
    evidenceId: number;
    speaker: Speaker;
    quote: string;
  }>;
}

interface ActiveSharedAgreement {
  claimId: number;
  statement: string;
  scope: string;
  effectiveFromMillis: number;
  effectiveUntilMillis: number | null;
}

export interface IdentityStateVersion {
  version: number;
  predecessorVersion: number | null;
  name: string;
  expressionTraits: string;
  viewpoints: string;
  valuePriorities: string;
  relationshipPosture: string;
  ownGoals: string;
  changeReason: string;
  evidenceIds: number[];
  formedAtMillis: number;
}

type CaptureState = "collecting" | "paused" | "locked" | "stopped";

interface CaptureStatus {
  state: CaptureState;
}

interface HostStatus {
  state: string;
  vaultReady: boolean;
  updaterConfigured: boolean;
  detail: string | null;
}

interface RecoveryKeyView {
  recoveryKey: string;
}

interface RuntimeProfileView {
  baseUrl: string;
  model: string;
  apiKeyConfigured: boolean;
  apiKeyLastFour: string | null;
}

type RuntimeProfileApiKeyChange =
  | { action: "KEEP" }
  | { action: "REPLACE"; value: string }
  | { action: "CLEAR" };

interface RuntimeProfileDraft {
  baseUrl: string;
  model: string;
  apiKeyChange: RuntimeProfileApiKeyChange;
}

interface VaultProjection {
  turns: ConversationTurn[];
  ceremonies: SharedExperienceCeremony[];
  identityHistory: IdentityStateVersion[];
  reflectionInvitations: ReflectionInvitationCeremony[];
  captureStatus: CaptureStatus;
  activityTimeline: ActivityTimelineEntry[];
}

export interface ActivityTimelineEntry {
  id: number;
  kind: "activity" | "gap";
  application: string | null;
  windowTitle: string | null;
  idle: boolean | null;
  gapReason:
    | "paused"
    | "sessionLocked"
    | "explicitExit"
    | "update"
    | "crash"
    | "sourceUnavailable"
    | null;
  startedAtMillis: number;
  observedUntilMillis: number;
  endedAtMillis: number | null;
}

interface WithdrawalDraft {
  agreement: ActiveSharedAgreement;
  reason: string;
}

interface AgreementRevisionDraft {
  statement: string;
  scope: string;
  effectiveFrom: string;
  effectiveUntil: string;
  endCondition: string;
}

const timeFormatter = new Intl.DateTimeFormat("zh-CN", {
  hour: "2-digit",
  minute: "2-digit",
});

async function loadVaultProjection(): Promise<VaultProjection> {
  const [
    turns,
    ceremonies,
    identityHistory,
    reflectionInvitations,
    captureStatus,
    activityTimeline,
  ] = await Promise.all([
    invoke<ConversationTurn[]>("list_conversation"),
    invoke<SharedExperienceCeremony[]>("list_shared_experience_ceremonies"),
    invoke<IdentityStateVersion[]>("list_identity_history").catch(() => []),
    invoke<ReflectionInvitationCeremony[]>(
      "list_offered_reflection_invitations",
    ).catch(() => []),
    invoke<CaptureStatus>("get_capture_status").catch(() => ({
      state: "collecting" as const,
    })),
    invoke<ActivityTimelineEntry[]>("list_activity_timeline").catch(() => []),
  ]);
  return {
    turns,
    ceremonies,
    identityHistory,
    reflectionInvitations,
    captureStatus,
    activityTimeline,
  };
}

export function App() {
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [ceremonies, setCeremonies] = useState<SharedExperienceCeremony[]>([]);
  const [reflectionInvitations, setReflectionInvitations] = useState<
    ReflectionInvitationCeremony[]
  >([]);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [hostStatus, setHostStatus] = useState<HostStatus | null>(null);
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [recoveryKeySaved, setRecoveryKeySaved] = useState(false);
  const [recoveryKeyCopied, setRecoveryKeyCopied] = useState(false);
  const [setupAction, setSetupAction] = useState(false);
  const [sending, setSending] = useState(false);
  const [ceremonyAction, setCeremonyAction] = useState<string | null>(null);
  const [activeAgreements, setActiveAgreements] = useState<ActiveSharedAgreement[]>([]);
  const [identityHistory, setIdentityHistory] = useState<IdentityStateVersion[]>([]);
  const [identityHistoryOpen, setIdentityHistoryOpen] = useState(false);
  const [captureStatus, setCaptureStatus] = useState<CaptureStatus>({
    state: "collecting",
  });
  const [activityTimeline, setActivityTimeline] = useState<ActivityTimelineEntry[]>([]);
  const [activityTimelineOpen, setActivityTimelineOpen] = useState(false);
  const [captureAction, setCaptureAction] = useState(false);
  const [runtimeSettingsOpen, setRuntimeSettingsOpen] = useState(false);
  const [runtimeProfile, setRuntimeProfile] = useState<RuntimeProfileView | null>(null);
  const [runtimeBaseUrl, setRuntimeBaseUrl] = useState("");
  const [runtimeModel, setRuntimeModel] = useState("");
  const [runtimeApiKey, setRuntimeApiKey] = useState("");
  const [runtimeClearKey, setRuntimeClearKey] = useState(false);
  const [runtimeProfileLoading, setRuntimeProfileLoading] = useState(false);
  const [runtimeProfileAction, setRuntimeProfileAction] = useState<
    "test" | "save" | null
  >(null);
  const [runtimeProfileError, setRuntimeProfileError] = useState<string | null>(null);
  const [runtimeProfileNotice, setRuntimeProfileNotice] = useState<string | null>(null);
  const [agreementManagerOpen, setAgreementManagerOpen] = useState(false);
  const [agreementLoading, setAgreementLoading] = useState(false);
  const [withdrawalDraft, setWithdrawalDraft] = useState<WithdrawalDraft | null>(null);
  const [withdrawalSubmitting, setWithdrawalSubmitting] = useState(false);
  const [revisionDraft, setRevisionDraft] = useState<AgreementRevisionDraft | null>(null);
  const [revisionNotice, setRevisionNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const conversationEnd = useRef<HTMLDivElement>(null);
  const messageInput = useRef<HTMLTextAreaElement>(null);
  const runtimeSettingsTrigger = useRef<HTMLButtonElement>(null);
  const runtimeSettingsBaseUrl = useRef<HTMLInputElement>(null);
  const runtimeSettingsClose = useRef<HTMLButtonElement>(null);
  const runtimeSettingsWasOpen = useRef(false);
  const runtimeSettingsShouldFocus = useRef(false);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const status = await invoke<HostStatus>("get_host_status");
        if (active) {
          setHostStatus(status);
        }
        if (!status.vaultReady) {
          return;
        }
        const projection = await loadVaultProjection();
        if (active) {
          applyVaultProjection(projection);
          setError(null);
        }
      } catch (reason: unknown) {
        if (active) {
          setError(errorMessage(reason));
        }
      } finally {
        if (active) {
          setLoading(false);
        }
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    conversationEnd.current?.scrollIntoView?.({ block: "end" });
  }, [turns, sending]);

  useEffect(() => {
    if (runtimeSettingsOpen) {
      runtimeSettingsWasOpen.current = true;
      if (!runtimeProfileLoading && runtimeSettingsShouldFocus.current) {
        runtimeSettingsShouldFocus.current = false;
        (runtimeSettingsBaseUrl.current ?? runtimeSettingsClose.current)?.focus();
      }
      return;
    }
    if (runtimeSettingsWasOpen.current) {
      runtimeSettingsWasOpen.current = false;
      runtimeSettingsTrigger.current?.focus();
    }
  }, [runtimeProfileLoading, runtimeSettingsOpen]);

  useEffect(() => {
    if (!runtimeSettingsOpen) {
      return;
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape" && runtimeProfileAction === null) {
        event.preventDefault();
        closeRuntimeSettings();
      }
    }
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [runtimeProfileAction, runtimeSettingsOpen]);

  function applyVaultProjection(projection: VaultProjection) {
    setTurns(projection.turns);
    setCeremonies(projection.ceremonies);
    setIdentityHistory(projection.identityHistory);
    setReflectionInvitations(projection.reflectionInvitations);
    setCaptureStatus(projection.captureStatus);
    setActivityTimeline(projection.activityTimeline);
  }

  async function openRuntimeSettings() {
    if (runtimeProfileAction !== null) {
      return;
    }
    runtimeSettingsShouldFocus.current = true;
    setRuntimeSettingsOpen(true);
    setRuntimeProfileLoading(true);
    setRuntimeProfile(null);
    setRuntimeBaseUrl("");
    setRuntimeModel("");
    setRuntimeApiKey("");
    setRuntimeClearKey(false);
    setRuntimeProfileError(null);
    setRuntimeProfileNotice(null);
    try {
      const profile = redactRuntimeProfileView(
        await invoke<RuntimeProfileView>("get_runtime_profile"),
      );
      setRuntimeProfile(profile);
      setRuntimeBaseUrl(profile.baseUrl);
      setRuntimeModel(profile.model);
    } catch {
      setRuntimeProfileError("运行时设置暂时无法读取；未显示任何认证信息。");
    } finally {
      setRuntimeProfileLoading(false);
    }
  }

  function closeRuntimeSettings() {
    if (runtimeProfileAction !== null) {
      return;
    }
    setRuntimeSettingsOpen(false);
    setRuntimeProfile(null);
    setRuntimeBaseUrl("");
    setRuntimeModel("");
    setRuntimeApiKey("");
    setRuntimeClearKey(false);
    setRuntimeProfileError(null);
    setRuntimeProfileNotice(null);
  }

  function runtimeProfileDraft(): RuntimeProfileDraft {
    const apiKeyChange: RuntimeProfileApiKeyChange = runtimeClearKey
      ? { action: "CLEAR" }
      : runtimeApiKey.length > 0
        ? { action: "REPLACE", value: runtimeApiKey }
        : { action: "KEEP" };
    return {
      baseUrl: runtimeBaseUrl,
      model: runtimeModel,
      apiKeyChange,
    };
  }

  async function testRuntimeProfile() {
    if (
      runtimeProfile === null ||
      runtimeProfileAction !== null ||
      runtimeBaseUrl.length === 0 ||
      runtimeModel.length === 0
    ) {
      return;
    }
    setRuntimeProfileAction("test");
    setRuntimeProfileError(null);
    setRuntimeProfileNotice(null);
    try {
      await invoke("test_runtime_profile", { draft: runtimeProfileDraft() });
      setRuntimeProfileNotice("连接测试成功；草稿尚未保存，当前运行时未切换。");
    } catch {
      setRuntimeProfileError("连接测试失败；草稿与当前运行时均未改变。");
    } finally {
      setRuntimeProfileAction(null);
    }
  }

  async function saveRuntimeProfile() {
    if (
      runtimeProfile === null ||
      runtimeProfileAction !== null ||
      runtimeBaseUrl.length === 0 ||
      runtimeModel.length === 0
    ) {
      return;
    }
    setRuntimeProfileAction("save");
    setRuntimeProfileError(null);
    setRuntimeProfileNotice(null);
    try {
      const saved = redactRuntimeProfileView(
        await invoke<RuntimeProfileView>("save_runtime_profile", {
          draft: runtimeProfileDraft(),
        }),
      );
      setRuntimeProfile(saved);
      setRuntimeBaseUrl(saved.baseUrl);
      setRuntimeModel(saved.model);
      setRuntimeApiKey("");
      setRuntimeClearKey(false);
      setRuntimeProfileNotice("运行时档案已保存并切换；Key 输入已清空。");
    } catch {
      setRuntimeProfileError("保存并切换失败；草稿和当前运行时均保持不变。");
    } finally {
      setRuntimeProfileAction(null);
    }
  }

  async function beginVaultInitialization() {
    if (setupAction) {
      return;
    }
    setSetupAction(true);
    setError(null);
    try {
      const result = await invoke<RecoveryKeyView>("initialize_vault");
      setRecoveryKey(result.recoveryKey);
      setRecoveryKeySaved(false);
      setRecoveryKeyCopied(false);
      setHostStatus((current) => ({
        state: "awaitingRecoveryConfirmation",
        vaultReady: false,
        updaterConfigured: current?.updaterConfigured ?? false,
        detail: null,
      }));
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setSetupAction(false);
    }
  }

  async function copyRecoveryKey() {
    if (recoveryKey === null) {
      return;
    }
    try {
      await navigator.clipboard.writeText(recoveryKey);
      setRecoveryKeyCopied(true);
      setError(null);
    } catch {
      setError("无法自动复制；请选中恢复密钥并手动复制。");
    }
  }

  async function confirmRecoveryKeySaved() {
    if (!recoveryKeySaved || recoveryKey === null || setupAction) {
      return;
    }
    setSetupAction(true);
    setError(null);
    try {
      const status = await invoke<HostStatus>("confirm_recovery_key_saved", {
        confirmed: true,
      });
      setHostStatus(status);
      setRecoveryKey(null);
      setRecoveryKeySaved(false);
      setRecoveryKeyCopied(false);
      const projection = await loadVaultProjection();
      applyVaultProjection(projection);
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setSetupAction(false);
    }
  }

  async function exitInterruptedInitialization() {
    setSetupAction(true);
    try {
      await invoke("exit_application");
    } catch (reason: unknown) {
      setError(errorMessage(reason));
      setSetupAction(false);
    }
  }

  async function submitMessage(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (sending || draft.trim().length === 0) {
      return;
    }

    const verbatim = draft;
    setSending(true);
    setError(null);
    try {
      const result = await invoke<ConversationTurnResult>("send_message", {
        verbatim,
      });
      setTurns((current) => mergeTurns(current, result.person, result.counterpart));
      setCeremonies((current) => mergeCeremonies(current, result.ceremonies));
      setReflectionInvitations((current) =>
        mergeReflectionInvitations(current, result.reflectionInvitations ?? []),
      );
      try {
        setIdentityHistory(
          await invoke<IdentityStateVersion[]>("list_identity_history"),
        );
      } catch {
        setError("回应已保存，但身份版本历史暂时无法刷新。");
      }
      setDraft("");
    } catch (reason: unknown) {
      setError(errorMessage(reason));
      try {
        const [restored, restoredCeremonies] = await Promise.all([
          invoke<ConversationTurn[]>("list_conversation"),
          invoke<SharedExperienceCeremony[]>("list_shared_experience_ceremonies"),
        ]);
        setTurns(restored);
        setCeremonies(restoredCeremonies);
      } catch {
        // Keep the last readable trusted state when recovery itself is unavailable.
      }
    } finally {
      setSending(false);
    }
  }

  async function resolveCeremony(
    ceremony: SharedExperienceCeremony,
    confirm?: boolean,
  ): Promise<boolean> {
    const key = ceremonyKey(ceremony);
    if (ceremonyAction !== null) {
      return false;
    }
    setCeremonyAction(key);
    setError(null);
    try {
      if (ceremony.admission === "confirmationRequired") {
        await invoke("resolve_shared_agreement", {
          candidateId: ceremony.targetId,
          confirm: confirm === true,
        });
      } else {
        await invoke("dismiss_shared_experience_ceremony", {
          claimId: ceremony.targetId,
        });
      }
      setCeremonies((current) =>
        current.filter((item) => ceremonyKey(item) !== key),
      );
      return true;
    } catch (reason: unknown) {
      setError(errorMessage(reason));
      return false;
    } finally {
      setCeremonyAction(null);
    }
  }

  async function acknowledgeCounterpartWithdrawal(
    ceremony: SharedExperienceCeremony,
    continueResponding: boolean,
  ) {
    const dismissed = await resolveCeremony(ceremony);
    if (dismissed && continueResponding) {
      messageInput.current?.focus();
    }
  }

  async function decideReflectionInvitation(
    invitation: ReflectionInvitationCeremony,
    decision: ReflectionInvitationDecision,
  ) {
    const key = `reflection:${invitation.id}`;
    if (ceremonyAction !== null) {
      return;
    }
    setCeremonyAction(key);
    setError(null);
    try {
      await invoke("decide_reflection_invitation", {
        invitationId: invitation.id,
        decision,
      });
      setReflectionInvitations((current) =>
        current.filter((item) => item.id !== invitation.id),
      );
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setCeremonyAction(null);
    }
  }

  async function openActivityTimeline() {
    setActivityTimelineOpen(true);
    setError(null);
    try {
      const [status, timeline] = await Promise.all([
        invoke<CaptureStatus>("get_capture_status"),
        invoke<ActivityTimelineEntry[]>("list_activity_timeline"),
      ]);
      setCaptureStatus(status);
      setActivityTimeline(timeline);
    } catch (reason: unknown) {
      setError(errorMessage(reason));
      setActivityTimelineOpen(false);
    }
  }

  async function toggleCapturePause() {
    if (captureAction || captureStatus.state === "locked") {
      return;
    }
    setCaptureAction(true);
    setError(null);
    const paused = captureStatus.state !== "paused";
    try {
      const status = await invoke<CaptureStatus>("set_capture_paused", {
        paused,
      });
      setCaptureStatus(status);
      setActivityTimeline(
        await invoke<ActivityTimelineEntry[]>("list_activity_timeline"),
      );
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setCaptureAction(false);
    }
  }

  async function openAgreementManager() {
    if (agreementLoading) {
      return;
    }
    setAgreementManagerOpen(true);
    setAgreementLoading(true);
    setError(null);
    try {
      const agreements = await invoke<ActiveSharedAgreement[]>(
        "list_active_shared_agreements",
      );
      setActiveAgreements(agreements);
    } catch (reason: unknown) {
      setError(errorMessage(reason));
      setAgreementManagerOpen(false);
    } finally {
      setAgreementLoading(false);
    }
  }

  async function confirmPersonWithdrawal() {
    if (withdrawalDraft === null || withdrawalSubmitting) {
      return;
    }
    setWithdrawalSubmitting(true);
    setError(null);
    const agreementClaimId = withdrawalDraft.agreement.claimId;
    const reason = withdrawalDraft.reason.trim() || null;
    try {
      await invoke<number | null>("withdraw_shared_agreement_as_person", {
        agreementClaimId,
        confirmed: true,
        reason,
      });
      setActiveAgreements((current) =>
        current.filter((agreement) => agreement.claimId !== agreementClaimId),
      );
      setWithdrawalDraft(null);
      setAgreementManagerOpen(false);
      try {
        const notices = await invoke<SharedExperienceCeremony[]>(
          "list_shared_experience_ceremonies",
        );
        setCeremonies((current) => mergeCeremonies(current, notices));
      } catch {
        setError("退出已生效，但共同历史通知暂时无法刷新。");
      }
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setWithdrawalSubmitting(false);
    }
  }

  function beginRevision(ceremony: SharedExperienceCeremony) {
    setRevisionDraft({
      statement: ceremony.statement,
      scope: ceremony.scope ?? "",
      effectiveFrom: toDateTimeInput(ceremony.effectiveFromMillis),
      effectiveUntil: toDateTimeInput(ceremony.effectiveUntilMillis),
      endCondition: ceremony.endCondition ?? "",
    });
  }

  async function submitRevision(
    event: FormEvent<HTMLFormElement>,
    ceremony: SharedExperienceCeremony,
  ) {
    event.preventDefault();
    if (revisionDraft === null || ceremonyAction !== null) {
      return;
    }
    const effectiveFromMillis = Date.parse(revisionDraft.effectiveFrom);
    const effectiveUntilMillis = revisionDraft.effectiveUntil
      ? Date.parse(revisionDraft.effectiveUntil)
      : null;
    if (
      revisionDraft.statement.trim().length === 0 ||
      revisionDraft.scope.trim().length === 0 ||
      !Number.isFinite(effectiveFromMillis) ||
      (effectiveUntilMillis !== null && !Number.isFinite(effectiveUntilMillis))
    ) {
      setError("候选表述、适用范围和生效时间不能为空。");
      return;
    }

    const key = ceremonyKey(ceremony);
    setCeremonyAction(key);
    setError(null);
    try {
      const revised = await invoke<{ candidateId: number; version: number; status: string }>(
        "revise_shared_agreement",
        {
          candidateId: ceremony.targetId,
          statement: revisionDraft.statement,
          scope: revisionDraft.scope,
          effectiveFromMillis,
          effectiveUntilMillis,
          endCondition: revisionDraft.endCondition.trim() || null,
          supersedesAgreementIds: ceremony.supersededAgreements.map(
            (agreement) => agreement.claimId,
          ),
        },
      );
      setCeremonies((current) =>
        current.filter((item) => ceremonyKey(item) !== key),
      );
      setRevisionDraft(null);
      setRevisionNotice(
        `候选 v${revised.version} 已生成，等待第二自我明确同意该精确版本后再由你最终签署。`,
      );
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setCeremonyAction(null);
    }
  }

  const activeCeremony = ceremonies[0];
  const activeReflection = activeCeremony === undefined ? reflectionInvitations[0] : undefined;

  if (hostStatus === null || !hostStatus.vaultReady) {
    return (
      <VaultSetup
        error={error}
        loading={loading}
        onBegin={() => void beginVaultInitialization()}
        onConfirm={() => void confirmRecoveryKeySaved()}
        onCopy={() => void copyRecoveryKey()}
        onExitInterrupted={() => void exitInterruptedInitialization()}
        recoveryKey={recoveryKey}
        recoveryKeyCopied={recoveryKeyCopied}
        recoveryKeySaved={recoveryKeySaved}
        setRecoveryKeySaved={setRecoveryKeySaved}
        setupAction={setupAction}
        status={hostStatus}
      />
    );
  }

  return (
    <main className="conversation-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">evrything-about-me</p>
          <h1>同一个第二自我</h1>
        </div>
        <div className="presence" aria-label="本地 Core 边界">
          <span className="presence-dot" />
          加密本地 Core
        </div>
        <div className="topbar-actions">
          <button
            className="agreement-manager-trigger"
            onClick={() => void openRuntimeSettings()}
            ref={runtimeSettingsTrigger}
            type="button"
          >
            运行时设置
          </button>
          <button
            className="agreement-manager-trigger"
            onClick={() => void openActivityTimeline()}
            type="button"
          >
            活动时间线
          </button>
          <button
            className="agreement-manager-trigger"
            onClick={() => setIdentityHistoryOpen(true)}
            type="button"
          >
            身份版本
          </button>
          <button
            className="agreement-manager-trigger"
            onClick={() => void openAgreementManager()}
            type="button"
          >
            管理共同约定
          </button>
        </div>
      </header>

      <section className="conversation" aria-label="持续对话">
        {loading ? (
          <p className="conversation-state">正在从加密保险库恢复对话…</p>
        ) : turns.length === 0 ? (
          <div className="empty-state">
            <p className="eyebrow">A quiet beginning</p>
            <h2>从此刻继续认识彼此。</h2>
            <p>你的原话与回应会作为可追溯的对话证据保留，但不会自动变成事实或长期记忆。</p>
          </div>
        ) : (
          <div className="turn-list">
            {turns.map((turn) => (
              <article className={`turn turn-${turn.speaker}`} key={turn.id}>
                <div className="turn-meta">
                  <span>{turn.speaker === "person" ? "你" : "第二自我"}</span>
                  <time dateTime={new Date(turn.recordedAtMillis).toISOString()}>
                    {timeFormatter.format(turn.recordedAtMillis)}
                  </time>
                </div>
                <p>{turn.verbatim}</p>
              </article>
            ))}
          </div>
        )}

        {sending ? (
          <div className="thinking" role="status">
            <span />
            <span />
            <span />
            正在回应
          </div>
        ) : null}
        <div ref={conversationEnd} />
      </section>

      {runtimeSettingsOpen ? (
        <div className="ceremony-layer runtime-settings-layer">
          <article
            aria-labelledby="runtime-settings-title"
            aria-modal="true"
            className="ceremony-card runtime-settings-card"
            role="dialog"
          >
            <p className="eyebrow">Vault 单档案</p>
            <h2 id="runtime-settings-title">运行时设置</h2>
            <p className="ceremony-note runtime-settings-intro">
              配置兼容 Responses 的 Base URL、模型和可选 Bearer Key。完整 Key 只写入本次草稿，读取时不会回显。
            </p>
            {runtimeProfileLoading ? (
              <>
                <p className="runtime-settings-state" role="status">
                  正在读取脱敏运行时档案…
                </p>
                <div className="ceremony-actions">
                  <button
                    className="secondary-action"
                    onClick={closeRuntimeSettings}
                    ref={runtimeSettingsClose}
                    type="button"
                  >
                    关闭
                  </button>
                </div>
              </>
            ) : runtimeProfile === null ? (
              <>
                {runtimeProfileError ? (
                  <p className="runtime-settings-error" role="alert">
                    {runtimeProfileError}
                  </p>
                ) : null}
                <div className="ceremony-actions">
                  <button
                    className="secondary-action"
                    onClick={() => void openRuntimeSettings()}
                    type="button"
                  >
                    重新读取
                  </button>
                  <button
                    className="secondary-action"
                    onClick={closeRuntimeSettings}
                    ref={runtimeSettingsClose}
                    type="button"
                  >
                    关闭
                  </button>
                </div>
              </>
            ) : (
              <form
                className="runtime-settings-form"
                onSubmit={(event) => {
                  event.preventDefault();
                  void saveRuntimeProfile();
                }}
              >
                <label htmlFor="runtime-base-url">Responses Base URL</label>
                <input
                  autoComplete="url"
                  disabled={runtimeProfileAction !== null}
                  id="runtime-base-url"
                  onChange={(event) => setRuntimeBaseUrl(event.target.value)}
                  ref={runtimeSettingsBaseUrl}
                  spellCheck={false}
                  type="url"
                  value={runtimeBaseUrl}
                />
                <p className="runtime-field-note">
                  应用会规范化地址，并只追加一个 <code>/responses</code>。
                </p>

                <label htmlFor="runtime-model">模型 ID</label>
                <input
                  autoComplete="off"
                  disabled={runtimeProfileAction !== null}
                  id="runtime-model"
                  onChange={(event) => setRuntimeModel(event.target.value)}
                  spellCheck={false}
                  value={runtimeModel}
                />

                <label htmlFor="runtime-api-key">Bearer Key（可选）</label>
                <input
                  aria-describedby="runtime-key-status"
                  autoComplete="new-password"
                  disabled={runtimeProfileAction !== null || runtimeClearKey}
                  id="runtime-api-key"
                  onChange={(event) => {
                    setRuntimeApiKey(event.target.value);
                    if (event.target.value.length > 0) {
                      setRuntimeClearKey(false);
                    }
                  }}
                  placeholder="留空以保持当前 Key"
                  spellCheck={false}
                  type="password"
                  value={runtimeApiKey}
                />
                <p className="runtime-field-note" id="runtime-key-status">
                  当前状态：{runtimeKeyStatus(runtimeProfile)}。输入新值会替换；留空会保持。
                </p>
                <label className="runtime-clear-confirmation" htmlFor="runtime-clear-key">
                  <input
                    checked={runtimeClearKey}
                    disabled={
                      runtimeProfileAction !== null || !runtimeProfile.apiKeyConfigured
                    }
                    id="runtime-clear-key"
                    onChange={(event) => {
                      setRuntimeClearKey(event.target.checked);
                      if (event.target.checked) {
                        setRuntimeApiKey("");
                      }
                    }}
                    type="checkbox"
                  />
                  <span>确认清除当前已保存的 Key</span>
                </label>

                {runtimeProfileError ? (
                  <p className="runtime-settings-error" role="alert">
                    {runtimeProfileError}
                  </p>
                ) : null}
                {runtimeProfileNotice ? (
                  <p className="runtime-settings-notice" role="status">
                    {runtimeProfileNotice}
                  </p>
                ) : null}

                <div className="ceremony-actions runtime-settings-actions">
                  <button
                    className="secondary-action"
                    disabled={
                      runtimeProfileAction !== null ||
                      runtimeBaseUrl.length === 0 ||
                      runtimeModel.length === 0
                    }
                    onClick={() => void testRuntimeProfile()}
                    type="button"
                  >
                    {runtimeProfileAction === "test" ? "正在测试…" : "测试连接"}
                  </button>
                  <button
                    className="secondary-action"
                    disabled={runtimeProfileAction !== null}
                    onClick={closeRuntimeSettings}
                    ref={runtimeSettingsClose}
                    type="button"
                  >
                    关闭
                  </button>
                  <button
                    disabled={
                      runtimeProfileAction !== null ||
                      runtimeBaseUrl.length === 0 ||
                      runtimeModel.length === 0
                    }
                    type="submit"
                  >
                    {runtimeProfileAction === "save" ? "正在保存…" : "保存并切换"}
                  </button>
                </div>
              </form>
            )}
          </article>
        </div>
      ) : null}

      {activityTimelineOpen ? (
        <div className="ceremony-layer">
          <article
            aria-labelledby="activity-timeline-title"
            aria-modal="true"
            className="ceremony-card"
            role="dialog"
          >
            <p className="eyebrow">Windows 最小元数据</p>
            <h2 id="activity-timeline-title">活动时间线</h2>
            <p className="ceremony-note">
              当前状态：{captureStateLabel(captureStatus.state)}。这里只记录前台应用、窗口标题、空闲状态和有原因空缺，不记录屏幕或键盘内容。
            </p>
            {activityTimeline.length === 0 ? (
              <p className="ceremony-note">尚无已确认的活动区间。</p>
            ) : (
              <div className="agreement-list activity-timeline-list">
                {[...activityTimeline].reverse().slice(0, 50).map((entry) => (
                  <section className="agreement-list-item" key={entry.id}>
                    {entry.kind === "activity" ? (
                      <>
                        <strong>
                          {entry.application} · {entry.idle ? "空闲" : "活跃"}
                        </strong>
                        <p>{entry.windowTitle || "无窗口标题"}</p>
                      </>
                    ) : (
                      <>
                        <strong>采集空缺 · {gapReasonLabel(entry.gapReason)}</strong>
                        <p>该区间没有活动元数据，系统不会推测或填补。</p>
                      </>
                    )}
                    <p className="ceremony-note">
                      {new Date(entry.startedAtMillis).toLocaleString("zh-CN")}
                      {" → "}
                      {new Date(
                        entry.endedAtMillis ?? entry.observedUntilMillis,
                      ).toLocaleString("zh-CN")}
                      {entry.endedAtMillis === null ? "（开放）" : ""}
                    </p>
                  </section>
                ))}
              </div>
            )}
            <div className="ceremony-actions">
              <button
                className="primary-action"
                disabled={
                  captureAction ||
                  captureStatus.state === "locked" ||
                  captureStatus.state === "stopped"
                }
                onClick={() => void toggleCapturePause()}
                type="button"
              >
                {captureStatus.state === "paused"
                  ? "继续记录"
                  : captureStatus.state === "locked"
                    ? "会话锁定中"
                    : "暂停记录"}
              </button>
              <button
                className="secondary-action"
                onClick={() => setActivityTimelineOpen(false)}
                type="button"
              >
                关闭
              </button>
            </div>
          </article>
        </div>
      ) : null}

      {identityHistoryOpen ? (
        <div className="ceremony-layer">
          <article
            aria-labelledby="identity-history-title"
            aria-modal="true"
            className="ceremony-card"
            role="dialog"
          >
            <p className="eyebrow">不可改写身份链</p>
            <h2 id="identity-history-title">第二自我的身份版本</h2>
            <p className="ceremony-note">
              这里是 Core 从加密保险库投影的只读历史。你可以通过对话影响第二自我，但不能直接编辑这些版本。
            </p>
            {identityHistory.length === 0 ? (
              <p className="ceremony-note">尚未形成身份版本。</p>
            ) : (
              <div className="agreement-list identity-history-list">
                {[...identityHistory].reverse().map((identity) => (
                  <section className="agreement-list-item" key={identity.version}>
                    <div className="identity-version-heading">
                      <strong>v{identity.version} · {identity.name}</strong>
                      <time dateTime={new Date(identity.formedAtMillis).toISOString()}>
                        {new Date(identity.formedAtMillis).toLocaleString("zh-CN")}
                      </time>
                    </div>
                    <dl className="ceremony-boundaries">
                      <div>
                        <dt>前驱版本</dt>
                        <dd>{identity.predecessorVersion ?? "首个版本"}</dd>
                      </div>
                      <div>
                        <dt>表达方式</dt>
                        <dd>{identity.expressionTraits}</dd>
                      </div>
                      <div>
                        <dt>观点</dt>
                        <dd>{identity.viewpoints}</dd>
                      </div>
                      <div>
                        <dt>价值排序</dt>
                        <dd>{identity.valuePriorities}</dd>
                      </div>
                      <div>
                        <dt>关系姿态</dt>
                        <dd>{identity.relationshipPosture}</dd>
                      </div>
                      <div>
                        <dt>自身目标</dt>
                        <dd>{identity.ownGoals}</dd>
                      </div>
                      <div>
                        <dt>变化理由</dt>
                        <dd>{identity.changeReason}</dd>
                      </div>
                      <div>
                        <dt>证据</dt>
                        <dd>{identity.evidenceIds.map((id) => `#${id}`).join("、")}</dd>
                      </div>
                    </dl>
                  </section>
                ))}
              </div>
            )}
            <div className="ceremony-actions">
              <button
                className="secondary-action"
                onClick={() => setIdentityHistoryOpen(false)}
                type="button"
              >
                关闭
              </button>
            </div>
          </article>
        </div>
      ) : null}

      {agreementManagerOpen && withdrawalDraft === null ? (
        <div className="ceremony-layer">
          <article
            aria-labelledby="agreement-manager-title"
            aria-modal="true"
            className="ceremony-card"
            role="dialog"
          >
            <p className="eyebrow">当前关系约束</p>
            <h2 id="agreement-manager-title">管理共同约定</h2>
            {agreementLoading ? (
              <p className="ceremony-note">正在读取当前有效约定…</p>
            ) : activeAgreements.length === 0 ? (
              <p className="ceremony-note">当前没有可退出的共同约定。</p>
            ) : (
              <div className="agreement-list">
                {activeAgreements.map((agreement) => (
                  <section className="agreement-list-item" key={agreement.claimId}>
                    <p>{agreement.statement}</p>
                    <dl className="ceremony-boundaries">
                      <div>
                        <dt>Claim</dt>
                        <dd>#{agreement.claimId}</dd>
                      </div>
                      <div>
                        <dt>适用范围</dt>
                        <dd>{agreement.scope}</dd>
                      </div>
                    </dl>
                    <button
                      className="secondary-action"
                      onClick={() => setWithdrawalDraft({ agreement, reason: "" })}
                      type="button"
                    >
                      退出这项约定
                    </button>
                  </section>
                ))}
              </div>
            )}
            <div className="ceremony-actions">
              <button
                className="secondary-action"
                disabled={agreementLoading}
                onClick={() => setAgreementManagerOpen(false)}
                type="button"
              >
                关闭
              </button>
            </div>
          </article>
        </div>
      ) : null}

      {withdrawalDraft !== null ? (
        <div className="ceremony-layer">
          <article
            aria-labelledby="withdrawal-title"
            aria-modal="true"
            className="ceremony-card"
            role="dialog"
          >
            <p className="eyebrow">防误触确认</p>
            <h2 id="withdrawal-title">确认退出共同约定</h2>
            <p className="ceremony-statement">{withdrawalDraft.agreement.statement}</p>
            <dl className="ceremony-boundaries">
              <div>
                <dt>共同约定</dt>
                <dd>Claim #{withdrawalDraft.agreement.claimId}</dd>
              </div>
              <div>
                <dt>影响</dt>
                <dd>确认后立即停止未来关系约束，原约定及历史不会删除。</dd>
              </div>
            </dl>
            <label className="withdrawal-reason-label" htmlFor="withdrawal-reason">
              理由（可选）
            </label>
            <textarea
              id="withdrawal-reason"
              onChange={(event) =>
                setWithdrawalDraft({ ...withdrawalDraft, reason: event.target.value })
              }
              value={withdrawalDraft.reason}
            />
            <div className="ceremony-actions">
              <button
                className="secondary-action"
                disabled={withdrawalSubmitting}
                onClick={() => setWithdrawalDraft(null)}
                type="button"
              >
                取消
              </button>
              <button
                disabled={withdrawalSubmitting}
                onClick={() => void confirmPersonWithdrawal()}
                type="button"
              >
                确认退出
              </button>
            </div>
          </article>
        </div>
      ) : null}

      {activeReflection ? (
        <div className="ceremony-layer">
          <article
            aria-labelledby="reflection-invitation-title"
            aria-modal="true"
            className="ceremony-card"
            role="dialog"
          >
            <p className="eyebrow">反思邀请</p>
            <h2 id="reflection-invitation-title">想和你一起看一件事</h2>
            <p className="ceremony-statement">{activeReflection.observation}</p>
            <dl className="ceremony-boundaries">
              <div>
                <dt>为何现在提出</dt>
                <dd>{activeReflection.whyNow}</dd>
              </div>
              <div>
                <dt>依据类型</dt>
                <dd>{reflectionBasisText(activeReflection.basis)}</dd>
              </div>
              <div>
                <dt>重要性</dt>
                <dd>{reflectionImportanceText(activeReflection.importance)}</dd>
              </div>
            </dl>
            <div className="ceremony-evidence" aria-label="反思邀请的逐字依据">
              {activeReflection.evidence.map((item) => (
                <blockquote key={`${item.evidenceId}-${item.quote}`}>
                  <span>{item.speaker === "person" ? "你" : "第二自我"}</span>
                  {item.quote}
                </blockquote>
              ))}
            </div>
            <p className="ceremony-note">
              {activeReflection.showMutePrompt
                ? "这项邀请已经延后过一次。你可以继续延后，或只停止它今后的主动出现；观察与证据不会被删除。"
                : "你可以稍后再谈；只有明确选择已谈完，才会把这项邀请标记为完成。"}
            </p>
            <div className="ceremony-actions">
              <button
                className="secondary-action"
                disabled={ceremonyAction !== null}
                onClick={() =>
                  void decideReflectionInvitation(activeReflection, "defer")
                }
                type="button"
              >
                {activeReflection.showMutePrompt ? "继续延后" : "稍后再说"}
              </button>
              {activeReflection.showMutePrompt ? (
                <button
                  className="secondary-action"
                  disabled={ceremonyAction !== null}
                  onClick={() =>
                    void decideReflectionInvitation(activeReflection, "mute")
                  }
                  type="button"
                >
                  不再主动提起
                </button>
              ) : null}
              <button
                disabled={ceremonyAction !== null}
                onClick={() =>
                  void decideReflectionInvitation(activeReflection, "resolve")
                }
                type="button"
              >
                这次已谈完
              </button>
            </div>
          </article>
        </div>
      ) : null}

      {activeCeremony ? (
        <div className="ceremony-layer">
          <article
            aria-labelledby="ceremony-title"
            aria-modal="true"
            className="ceremony-card"
            role="dialog"
          >
            <p className="eyebrow">共同历史仪式</p>
            <h2 id="ceremony-title">{ceremonyTitle(activeCeremony)}</h2>
            {activeCeremony.candidateVersion !== null ? (
              <p className="ceremony-version">候选 v{activeCeremony.candidateVersion}</p>
            ) : null}
            <p className="ceremony-statement">{activeCeremony.statement}</p>
            {activeCeremony.admission === "confirmationRequired" ? (
              <dl className="ceremony-boundaries">
                <div>
                  <dt>适用范围</dt>
                  <dd>{activeCeremony.scope}</dd>
                </div>
                <div>
                  <dt>生效时间</dt>
                  <dd>{formatBoundaryTime(activeCeremony.effectiveFromMillis)}</dd>
                </div>
                <div>
                  <dt>终止项</dt>
                  <dd>{terminationText(activeCeremony)}</dd>
                </div>
              </dl>
            ) : null}
            {activeCeremony.experienceKind === "agreementBreach" ? (
              <dl className="ceremony-boundaries">
                <div>
                  <dt>偏离的共同约定</dt>
                  <dd>Claim #{activeCeremony.agreementClaimId}</dd>
                </div>
                <div>
                  <dt>第二自我说明的理由</dt>
                  <dd>{activeCeremony.departureReason}</dd>
                </div>
              </dl>
            ) : null}
            {activeCeremony.experienceKind === "agreementWithdrawal" ? (
              <dl className="ceremony-boundaries">
                <div>
                  <dt>退出的共同约定</dt>
                  <dd>Claim #{activeCeremony.agreementClaimId}</dd>
                </div>
                <div>
                  <dt>行为方</dt>
                  <dd>
                    {activeCeremony.withdrawalActor === "counterpart" ? "第二自我" : "你"}
                  </dd>
                </div>
                <div>
                  <dt>生效时间</dt>
                  <dd>{formatBoundaryTime(activeCeremony.effectiveFromMillis)}</dd>
                </div>
                <div>
                  <dt>理由</dt>
                  <dd>{activeCeremony.departureReason ?? "未填写"}</dd>
                </div>
              </dl>
            ) : null}
            {activeCeremony.supersededAgreements.length > 0 ? (
              <section className="ceremony-evidence" aria-label="将被整份取代的共同约定">
                <h3>本约定将整份取代以下共同约定</h3>
                {activeCeremony.supersededAgreements.map((agreement) => (
                  <dl className="ceremony-boundaries" key={agreement.claimId}>
                    <div>
                      <dt>原约定</dt>
                      <dd>
                        Claim #{agreement.claimId}：{agreement.statement}
                      </dd>
                    </div>
                    <div>
                      <dt>原适用范围</dt>
                      <dd>{agreement.scope}</dd>
                    </div>
                    <div>
                      <dt>原有效期</dt>
                      <dd>
                        {formatBoundaryTime(agreement.effectiveFromMillis)} 至{" "}
                        {agreement.effectiveUntilMillis === null
                          ? "持续有效"
                          : formatBoundaryTime(agreement.effectiveUntilMillis)}
                      </dd>
                    </div>
                  </dl>
                ))}
                <p className="ceremony-note">
                  新约定生效后，以上旧约定整份停止未来约束；系统不会推导任何残余义务，历史仍保留。
                </p>
              </section>
            ) : null}
            <div className="ceremony-evidence" aria-label="支持它的双方原话">
              {activeCeremony.evidence.map((item) => (
                <blockquote key={`${item.evidenceId}-${item.speaker}`}>
                  <span>{item.speaker === "person" ? "你" : "第二自我"}</span>
                  {item.quote}
                </blockquote>
              ))}
            </div>
            <p className="ceremony-note">
              {activeCeremony.experienceKind === "agreementWithdrawal"
                ? "退出已生效并进入共同历史；只停止未来约束，另一方不能否决或撤销已完成的退出。"
                : activeCeremony.admission === "confirmationRequired"
                ? "只有你确认后，这项共同约定才会进入共同经历账本。"
                : "这段共同历史已依据双方证据入账；关闭通知不会撤销记录。"}
            </p>
            <div className="ceremony-actions">
              {activeCeremony.admission === "confirmationRequired" && revisionDraft === null ? (
                <>
                  <button
                    className="secondary-action"
                    disabled={ceremonyAction !== null}
                    onClick={() => void resolveCeremony(activeCeremony, false)}
                    type="button"
                  >
                    暂不记录
                  </button>
                  <button
                    className="secondary-action"
                    disabled={ceremonyAction !== null}
                    onClick={() => beginRevision(activeCeremony)}
                    type="button"
                  >
                    提出修改
                  </button>
                  <button
                    disabled={ceremonyAction !== null}
                    onClick={() => void resolveCeremony(activeCeremony, true)}
                    type="button"
                  >
                    确认入账
                  </button>
                </>
              ) : activeCeremony.experienceKind === "agreementWithdrawal" &&
                activeCeremony.withdrawalActor === "counterpart" ? (
                <>
                  <button
                    className="secondary-action"
                    disabled={ceremonyAction !== null}
                    onClick={() =>
                      void acknowledgeCounterpartWithdrawal(activeCeremony, false)
                    }
                    type="button"
                  >
                    已知悉
                  </button>
                  <button
                    disabled={ceremonyAction !== null}
                    onClick={() =>
                      void acknowledgeCounterpartWithdrawal(activeCeremony, true)
                    }
                    type="button"
                  >
                    继续回应
                  </button>
                </>
              ) : activeCeremony.admission === "nonVetoNotice" ? (
                <button
                  disabled={ceremonyAction !== null}
                  onClick={() => void resolveCeremony(activeCeremony)}
                  type="button"
                >
                  已知悉并关闭
                </button>
              ) : null}
            </div>
            {activeCeremony.admission === "confirmationRequired" && revisionDraft !== null ? (
              <form
                className="revision-form"
                onSubmit={(event) => void submitRevision(event, activeCeremony)}
              >
                <label htmlFor="agreement-statement">候选表述</label>
                <textarea
                  id="agreement-statement"
                  onChange={(event) =>
                    setRevisionDraft({ ...revisionDraft, statement: event.target.value })
                  }
                  value={revisionDraft.statement}
                />
                <label htmlFor="agreement-scope">适用范围</label>
                <input
                  id="agreement-scope"
                  onChange={(event) =>
                    setRevisionDraft({ ...revisionDraft, scope: event.target.value })
                  }
                  value={revisionDraft.scope}
                />
                <label htmlFor="agreement-effective-from">生效时间</label>
                <input
                  id="agreement-effective-from"
                  onChange={(event) =>
                    setRevisionDraft({ ...revisionDraft, effectiveFrom: event.target.value })
                  }
                  type="datetime-local"
                  value={revisionDraft.effectiveFrom}
                />
                <label htmlFor="agreement-effective-until">终止时间（可选）</label>
                <input
                  id="agreement-effective-until"
                  onChange={(event) =>
                    setRevisionDraft({ ...revisionDraft, effectiveUntil: event.target.value })
                  }
                  type="datetime-local"
                  value={revisionDraft.effectiveUntil}
                />
                <label htmlFor="agreement-end-condition">终止条件（可选）</label>
                <input
                  id="agreement-end-condition"
                  onChange={(event) =>
                    setRevisionDraft({ ...revisionDraft, endCondition: event.target.value })
                  }
                  value={revisionDraft.endCondition}
                />
                <div className="ceremony-actions">
                  <button
                    className="secondary-action"
                    disabled={ceremonyAction !== null}
                    onClick={() => setRevisionDraft(null)}
                    type="button"
                  >
                    取消修改
                  </button>
                  <button disabled={ceremonyAction !== null} type="submit">
                    提交新版本
                  </button>
                </div>
              </form>
            ) : null}
            {ceremonies.length > 1 ? (
              <p className="ceremony-queue">还有 {ceremonies.length - 1} 项仪式等待处理</p>
            ) : null}
          </article>
        </div>
      ) : null}

      <footer className="composer-wrap">
        {error ? (
          <p className="error-banner" role="alert">
            {error}
          </p>
        ) : null}
        {revisionNotice ? <p className="revision-notice">{revisionNotice}</p> : null}
        <form className="composer" onSubmit={submitMessage}>
          <label className="sr-only" htmlFor="message">
            写下此刻想说的话
          </label>
          <textarea
            id="message"
            maxLength={16_384}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="写下此刻想说的话…"
            ref={messageInput}
            rows={2}
            value={draft}
          />
          <button disabled={sending || draft.trim().length === 0} type="submit">
            {sending ? "等待" : "发送"}
          </button>
        </form>
        <p className="retention-note">逐字加密保留 · 不自动升格为记忆</p>
      </footer>
    </main>
  );
}

interface VaultSetupProps {
  error: string | null;
  loading: boolean;
  onBegin: () => void;
  onConfirm: () => void;
  onCopy: () => void;
  onExitInterrupted: () => void;
  recoveryKey: string | null;
  recoveryKeyCopied: boolean;
  recoveryKeySaved: boolean;
  setRecoveryKeySaved: (saved: boolean) => void;
  setupAction: boolean;
  status: HostStatus | null;
}

function VaultSetup({
  error,
  loading,
  onBegin,
  onConfirm,
  onCopy,
  onExitInterrupted,
  recoveryKey,
  recoveryKeyCopied,
  recoveryKeySaved,
  setRecoveryKeySaved,
  setupAction,
  status,
}: VaultSetupProps) {
  let content;
  if (loading && status === null) {
    content = (
      <>
        <p className="eyebrow">Local-first vault</p>
        <h1>正在检查本地保险库…</h1>
      </>
    );
  } else if (status?.state === "needsInitialization") {
    content = (
      <>
        <p className="eyebrow">First run</p>
        <h1>创建你的加密保险库</h1>
        <p className="vault-setup-copy">
          保险库只保存在这台电脑上。下一步会生成一枚独立恢复密钥；只有你能保管它，平台无法替你找回。
        </p>
        <button
          className="vault-primary-action"
          disabled={setupAction}
          onClick={onBegin}
          type="button"
        >
          {setupAction ? "正在生成…" : "生成恢复密钥"}
        </button>
      </>
    );
  } else if (recoveryKey !== null) {
    content = (
      <>
        <p className="eyebrow">Recovery key · 仅此一次</p>
        <h1>先保存恢复密钥</h1>
        <p className="vault-setup-copy">
          换机或本机密钥损坏时，只有它能恢复保险库。确认后应用不会再次显示，也不会替你上传或保存明文副本。
        </p>
        <label className="recovery-key-label" htmlFor="recovery-key">
          恢复密钥
        </label>
        <textarea
          aria-label="恢复密钥"
          className="recovery-key-value"
          id="recovery-key"
          readOnly
          rows={3}
          value={recoveryKey}
        />
        <button className="vault-secondary-action" onClick={onCopy} type="button">
          {recoveryKeyCopied ? "已复制" : "复制恢复密钥"}
        </button>
        <label className="recovery-confirmation" htmlFor="recovery-key-saved">
          <input
            checked={recoveryKeySaved}
            id="recovery-key-saved"
            onChange={(event) => setRecoveryKeySaved(event.target.checked)}
            type="checkbox"
          />
          <span>我已把恢复密钥保存在独立且安全的位置，并理解丢失后平台无法恢复。</span>
        </label>
        <button
          className="vault-primary-action"
          disabled={!recoveryKeySaved || setupAction}
          onClick={onConfirm}
          type="button"
        >
          {setupAction ? "正在创建保险库…" : "我已安全保存，创建保险库"}
        </button>
      </>
    );
  } else if (status?.state === "awaitingRecoveryConfirmation") {
    content = (
      <>
        <p className="eyebrow">Setup interrupted</p>
        <h1>恢复密钥页面已中断</h1>
        <p className="vault-setup-copy">
          为防止同一密钥被重复读取，本页面不能再次索取它。确认前没有文件写入磁盘；退出后重新打开即可安全生成新密钥。
        </p>
        <button
          className="vault-primary-action"
          disabled={setupAction}
          onClick={onExitInterrupted}
          type="button"
        >
          退出应用，随后重新打开
        </button>
      </>
    );
  } else {
    content = (
      <>
        <p className="eyebrow">Vault unavailable</p>
        <h1>保险库无法打开</h1>
        <p className="vault-setup-copy">
          {status?.detail ?? "无法读取本地 Core 状态，请重新启动应用后再试。"}
        </p>
      </>
    );
  }

  return (
    <main className="vault-setup-shell">
      <section aria-live="polite" className="vault-setup-card">
        {content}
        {error ? (
          <p className="error-banner vault-setup-error" role="alert">
            {error}
          </p>
        ) : null}
      </section>
    </main>
  );
}

function reflectionBasisText(
  basis: ReflectionInvitationCeremony["basis"],
): string {
  return basis === "importantSingleChange" ? "重要单次变化" : "暂定重复模式";
}

function reflectionImportanceText(
  importance: ReflectionInvitationCeremony["importance"],
): string {
  switch (importance) {
    case "ordinary":
      return "普通";
    case "important":
      return "重要";
    case "immediateSafetyRisk":
      return "即时安全风险";
  }
}

function ceremonyTitle(ceremony: SharedExperienceCeremony): string {
  switch (ceremony.experienceKind) {
    case "agreement":
      return "共同约定待确认";
    case "substantiveDisagreement":
      return "实质分歧已记录";
    case "relationshipChange":
      return "关系变化已记录";
    case "sharedAchievement":
      return "共同完成的重要事情已记录";
    case "agreementBreach":
      return "共同约定偏离已记录";
    case "agreementWithdrawal":
      return ceremony.withdrawalActor === "counterpart"
        ? "第二自我已退出共同约定"
        : "你已退出共同约定";
  }
}

function terminationText(ceremony: SharedExperienceCeremony): string {
  const parts: string[] = [];
  if (ceremony.effectiveUntilMillis !== null) {
    parts.push(`至 ${formatBoundaryTime(ceremony.effectiveUntilMillis)}`);
  }
  if (ceremony.endCondition !== null) {
    parts.push(ceremony.endCondition);
  }
  return parts.length > 0
    ? parts.join("；")
    : "持续有效，直到任何一方退出或双方签署替代约定";
}

function formatBoundaryTime(value: number | null): string {
  return value === null ? "未设置" : new Date(value).toLocaleString("zh-CN");
}

function toDateTimeInput(value: number | null): string {
  if (value === null) {
    return "";
  }
  const date = new Date(value);
  const local = new Date(value - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function ceremonyKey(ceremony: SharedExperienceCeremony): string {
  return `${ceremony.targetKind}:${ceremony.targetId}`;
}

function mergeCeremonies(
  current: SharedExperienceCeremony[],
  incoming: SharedExperienceCeremony[],
): SharedExperienceCeremony[] {
  const byKey = new Map(current.map((item) => [ceremonyKey(item), item]));
  for (const item of incoming) {
    byKey.set(ceremonyKey(item), item);
  }
  return [...byKey.values()];
}

function mergeReflectionInvitations(
  current: ReflectionInvitationCeremony[],
  incoming: ReflectionInvitationCeremony[],
): ReflectionInvitationCeremony[] {
  const byId = new Map(current.map((item) => [item.id, item]));
  for (const item of incoming) {
    byId.set(item.id, item);
  }
  return [...byId.values()].sort((left, right) => left.id - right.id);
}

function mergeTurns(
  current: ConversationTurn[],
  ...incoming: ConversationTurn[]
): ConversationTurn[] {
  const byId = new Map(current.map((turn) => [turn.id, turn]));
  for (const turn of incoming) {
    byId.set(turn.id, turn);
  }
  return [...byId.values()].sort((left, right) => left.id - right.id);
}

function captureStateLabel(state: CaptureState): string {
  switch (state) {
    case "collecting":
      return "持续记录中";
    case "paused":
      return "本人已暂停";
    case "locked":
      return "Windows 会话已锁定";
    case "stopped":
      return "宿主已停止";
  }
}

function gapReasonLabel(
  reason: ActivityTimelineEntry["gapReason"],
): string {
  switch (reason) {
    case "paused":
      return "本人暂停";
    case "sessionLocked":
      return "Windows 会话锁定";
    case "explicitExit":
      return "显式退出";
    case "update":
      return "签名升级";
    case "crash":
      return "宿主崩溃";
    case "sourceUnavailable":
      return "采集源不可用";
    case null:
      return "未知原因";
  }
}

function runtimeKeyStatus(profile: RuntimeProfileView): string {
  if (!profile.apiKeyConfigured) {
    return "未配置";
  }
  return profile.apiKeyLastFour === null
    ? "已配置，不显示短 Key 的任何字符"
    : `已配置，末四位 ${profile.apiKeyLastFour}`;
}

function redactRuntimeProfileView(profile: RuntimeProfileView): RuntimeProfileView {
  return {
    baseUrl: profile.baseUrl,
    model: profile.model,
    apiKeyConfigured: profile.apiKeyConfigured,
    apiKeyLastFour:
      profile.apiKeyConfigured && profile.apiKeyLastFour?.length === 4
        ? profile.apiKeyLastFour
        : null,
  };
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
