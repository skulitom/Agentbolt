import { useCallback, useEffect, useState } from "react";

interface SessionRow {
  sessionId: string;
  title: string;
  status: string;
  /** Open, but idle long enough that it is almost certainly from an earlier run. */
  stale: boolean;
  idleLabel: string;
  /** Buttons for a question answerable without leaving the inbox. */
  quickChoices: Array<{ id: string; label: string; style: string }>;
  url: string;
  lastEventSeq: number;
}

/**
 * The dashboard is never auto-opened — the per-session tab is what gets your
 * attention. This is the place you come back to on purpose: everything still
 * waiting, across every project with an agent running.
 *
 * Which means the top of this page has to be trustworthy. Sessions restored
 * from earlier runs are still listed — they still work, and one might be the
 * question you meant to come back to — but they are folded away under their own
 * heading and kept out of the count, because a "7 waiting" badge that is mostly
 * last week's demos is a badge nobody reads. The server decides what counts as
 * stale, so this and ui_get_state can never disagree about it.
 */
export function Dashboard() {
  const [project, setProject] = useState<string>("");
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [error, setError] = useState<string>();
  const [clearing, setClearing] = useState(false);

  const load = useCallback(async () => {
    // `inbox=1` marks this as the inbox being open, which is what tells the
    // server a new question does not need its own tab.
    const response = await fetch("/api/sessions?inbox=1");
    const body = (await response.json()) as { project: string; sessions: SessionRow[] };
    setProject(body.project);
    setSessions(body.sessions);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        await load();
        if (!cancelled) setError(undefined);
      } catch (err) {
        if (!cancelled) setError(String(err));
      }
    };
    void poll();
    const timer = window.setInterval(poll, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [load]);

  const waiting = sessions.filter((s) => s.status === "active" && !s.stale);
  const stale = sessions.filter((s) => s.status === "active" && s.stale);
  const finished = sessions.filter((s) => s.status !== "active");

  useEffect(() => {
    document.title = waiting.length ? `● ${waiting.length} waiting — AgentUI` : "AgentUI";
  }, [waiting.length]);

  async function clearStale() {
    if (!window.confirm(`Close ${stale.length} widget${stale.length === 1 ? "" : "s"} from earlier runs?`)) return;
    setClearing(true);
    try {
      await fetch("/api/sessions/stale/close", { method: "POST" });
      await load();
    } catch (err) {
      setError(String(err));
    } finally {
      setClearing(false);
    }
  }

  return (
    <div className="dashboard">
      <header className="session-head">
        <div>
          <h1>AgentUI</h1>
          <p className="muted">
            {project ? `project: ${project}` : " "}
            {" · "}
            {/* The library was previously unreachable from here — saved widgets
                you cannot navigate to are barely saved at all. */}
            <a href="/library">widget library</a>
          </p>
        </div>
        {waiting.length ? <span className="pill pill-active">{waiting.length} waiting</span> : null}
      </header>

      {error ? <p className="notice">Could not reach the server: {error}</p> : null}

      {sessions.length === 0 ? (
        <p className="muted">
          No widgets yet. When an agent calls <code>ui_ask</code> or <code>ui_create</code>, it appears here and
          opens in its own tab.
        </p>
      ) : null}

      <SessionList rows={waiting} onAnswered={() => void load()} />

      {stale.length ? (
        <details className="stale-group">
          <summary>
            {stale.length} from earlier runs — still open, but idle for hours
          </summary>
          <p className="muted stale-note">
            These were never answered and nothing is waiting on them. They still work if you want to finish
            one; otherwise clearing them just closes them.
          </p>
          <button type="button" className="subtle" disabled={clearing} onClick={() => void clearStale()}>
            {clearing ? "Clearing…" : "Clear all"}
          </button>
          <SessionList rows={stale} />
        </details>
      ) : null}

      {finished.length ? (
        <>
          <h2 className="group-title">Finished</h2>
          <SessionList rows={finished} />
        </>
      ) : null}
    </div>
  );
}

function SessionList({ rows, onAnswered }: { rows: SessionRow[]; onAnswered?: () => void }) {
  if (!rows.length) return null;
  return (
    <ul className="session-list">
      {rows.map((row) => (
        <li key={row.sessionId}>
          <a href={row.url}>
            <span className={`dot dot-${dotFor(row)}`} aria-hidden="true" />
            <span className="row-title">{row.title}</span>
            <span className="row-meta">{row.stale ? `idle ${row.idleLabel}` : row.status}</span>
          </a>
          {/* Answerable in place. Most questions are a single choice, and making
              those cost a page navigation is what stops an agent asking at all. */}
          {row.quickChoices?.length ? (
            <QuickAnswer row={row} onAnswered={onAnswered} />
          ) : null}
        </li>
      ))}
    </ul>
  );
}

function QuickAnswer({ row, onAnswered }: { row: SessionRow; onAnswered?: () => void }) {
  const [busy, setBusy] = useState<string>();

  async function answer(actionId: string) {
    setBusy(actionId);
    try {
      await fetch(`/api/session/${encodeURIComponent(row.sessionId)}/answer`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ actionId }),
      });
      onAnswered?.();
    } finally {
      setBusy(undefined);
    }
  }

  return (
    <div className="quick-answer">
      {row.quickChoices.map((choice) => (
        <button
          key={choice.id}
          type="button"
          className={choice.style === "primary" ? "btn-primary" : choice.style === "danger" ? "btn-danger" : ""}
          disabled={Boolean(busy)}
          onClick={() => void answer(choice.id)}
        >
          {busy === choice.id ? "…" : choice.label}
        </button>
      ))}
    </div>
  );
}

function dotFor(row: SessionRow): string {
  if (row.status !== "active") return "closed";
  return row.stale ? "stale" : "open";
}
