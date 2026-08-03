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

export function App() {
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [ceremonies, setCeremonies] = useState<SharedExperienceCeremony[]>([]);
  const [reflectionInvitations, setReflectionInvitations] = useState<
    ReflectionInvitationCeremony[]
  >([]);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
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
  const [agreementManagerOpen, setAgreementManagerOpen] = useState(false);
  const [agreementLoading, setAgreementLoading] = useState(false);
  const [withdrawalDraft, setWithdrawalDraft] = useState<WithdrawalDraft | null>(null);
  const [withdrawalSubmitting, setWithdrawalSubmitting] = useState(false);
  const [revisionDraft, setRevisionDraft] = useState<AgreementRevisionDraft | null>(null);
  const [revisionNotice, setRevisionNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const conversationEnd = useRef<HTMLDivElement>(null);
  const messageInput = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    let active = true;
    void Promise.all([
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
    ])
      .then(([
        restored,
        restoredCeremonies,
        restoredIdentity,
        restoredReflections,
        restoredCaptureStatus,
        restoredTimeline,
      ]) => {
        if (active) {
          setTurns(restored);
          setCeremonies(restoredCeremonies);
          setIdentityHistory(restoredIdentity);
          setReflectionInvitations(restoredReflections);
          setCaptureStatus(restoredCaptureStatus);
          setActivityTimeline(restoredTimeline);
          setError(null);
        }
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(errorMessage(reason));
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    conversationEnd.current?.scrollIntoView?.({ block: "end" });
  }, [turns, sending]);

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

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
