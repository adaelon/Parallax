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
}

const timeFormatter = new Intl.DateTimeFormat("zh-CN", {
  hour: "2-digit",
  minute: "2-digit",
});

export function App() {
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const conversationEnd = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let active = true;
    void invoke<ConversationTurn[]>("list_conversation")
      .then((restored) => {
        if (active) {
          setTurns(restored);
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
      setDraft("");
    } catch (reason: unknown) {
      setError(errorMessage(reason));
      try {
        const restored = await invoke<ConversationTurn[]>("list_conversation");
        setTurns(restored);
      } catch {
        // Keep the last readable conversation when recovery itself is unavailable.
      }
    } finally {
      setSending(false);
    }
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
