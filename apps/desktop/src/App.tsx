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
}

type SharedExperienceKind =
  | "agreement"
  | "substantiveDisagreement"
  | "relationshipChange"
  | "sharedAchievement";

export interface SharedExperienceCeremony {
  targetId: number;
  targetKind: "agreementCandidate" | "sharedExperience";
  experienceKind: SharedExperienceKind;
  admission: "confirmationRequired" | "nonVetoNotice";
  statement: string;
  evidence: Array<{
    evidenceId: number;
    speaker: Speaker;
    quote: string;
  }>;
}

const timeFormatter = new Intl.DateTimeFormat("zh-CN", {
  hour: "2-digit",
  minute: "2-digit",
});

export function App() {
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [ceremonies, setCeremonies] = useState<SharedExperienceCeremony[]>([]);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [sending, setSending] = useState(false);
  const [ceremonyAction, setCeremonyAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const conversationEnd = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let active = true;
    void Promise.all([
      invoke<ConversationTurn[]>("list_conversation"),
      invoke<SharedExperienceCeremony[]>("list_shared_experience_ceremonies"),
    ])
      .then(([restored, restoredCeremonies]) => {
        if (active) {
          setTurns(restored);
          setCeremonies(restoredCeremonies);
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
  ) {
    const key = ceremonyKey(ceremony);
    if (ceremonyAction !== null) {
      return;
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
    } catch (reason: unknown) {
      setError(errorMessage(reason));
    } finally {
      setCeremonyAction(null);
    }
  }

  const activeCeremony = ceremonies[0];

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

      {activeCeremony ? (
        <div className="ceremony-layer">
          <article
            aria-labelledby="ceremony-title"
            aria-modal="true"
            className="ceremony-card"
            role="dialog"
          >
            <p className="eyebrow">共同历史仪式</p>
            <h2 id="ceremony-title">{ceremonyTitle(activeCeremony.experienceKind)}</h2>
            <p className="ceremony-statement">{activeCeremony.statement}</p>
            <div className="ceremony-evidence" aria-label="支持它的双方原话">
              {activeCeremony.evidence.map((item) => (
                <blockquote key={`${item.evidenceId}-${item.speaker}`}>
                  <span>{item.speaker === "person" ? "你" : "第二自我"}</span>
                  {item.quote}
                </blockquote>
              ))}
            </div>
            <p className="ceremony-note">
              {activeCeremony.admission === "confirmationRequired"
                ? "只有你确认后，这项共同约定才会进入共同经历账本。"
                : "这段共同历史已依据双方证据入账；关闭通知不会撤销记录。"}
            </p>
            <div className="ceremony-actions">
              {activeCeremony.admission === "confirmationRequired" ? (
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
                    disabled={ceremonyAction !== null}
                    onClick={() => void resolveCeremony(activeCeremony, true)}
                    type="button"
                  >
                    确认入账
                  </button>
                </>
              ) : (
                <button
                  disabled={ceremonyAction !== null}
                  onClick={() => void resolveCeremony(activeCeremony)}
                  type="button"
                >
                  已知悉并关闭
                </button>
              )}
            </div>
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
        <form className="composer" onSubmit={submitMessage}>
          <label className="sr-only" htmlFor="message">
            写下此刻想说的话
          </label>
          <textarea
            id="message"
            maxLength={16_384}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="写下此刻想说的话…"
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

function ceremonyTitle(kind: SharedExperienceKind): string {
  switch (kind) {
    case "agreement":
      return "共同约定待确认";
    case "substantiveDisagreement":
      return "实质分歧已记录";
    case "relationshipChange":
      return "关系变化已记录";
    case "sharedAchievement":
      return "共同完成的重要事情已记录";
  }
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

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
