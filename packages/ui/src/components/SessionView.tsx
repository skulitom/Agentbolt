import { useEffect, useMemo, useRef, useState } from "react";
import { controlIsIllustrated, groupControls } from "@agentui/shared";
import { useSession } from "../store.js";
import { ControlRenderer } from "./ControlRenderer.js";
import { ContentRenderer } from "./ContentRenderer.js";
import { PreviewFrame } from "./PreviewFrame.js";
import { HomeLink } from "./HomeLink.js";

export function SessionView() {
  const spec = useSession((s) => s.spec);
  const revision = useSession((s) => s.revision);
  const status = useSession((s) => s.status);
  const connection = useSession((s) => s.connection);
  const choice = useSession((s) => s.choice);
  const sentCount = useSession((s) => s.sentCount);
  const hasPreview = useSession((s) => s.hasPreview);
  const previewOrigin = useSession((s) => s.previewOrigin);
  const sessionId = useSession((s) => s.sessionId);
  const touchedCount = useSession((s) => s.touched.size);
  const resetAll = useSession((s) => s.resetAll);
  const pressAction = useSession((s) => s.pressAction);
  const finalize = useSession((s) => s.finalize);
  const sendComment = useSession((s) => s.sendComment);

  const [comment, setComment] = useState("");
  const [confirming, setConfirming] = useState(false);
  const done = status !== "active";
  const mainRef = useRef<HTMLDivElement>(null);
  const seenRevision = useRef<number | null>(null);

  useTabAttention(spec?.title, status);

  // A re-authored spec swaps content in place — deliberately no remount, which
  // would reset scroll and open <details> state — so a brief settle fade is
  // what makes the swap legible at all. Skips the first revision seen: initial
  // load is not a change. 220ms mirrors --t-gentle in styles.css.
  useEffect(() => {
    if (seenRevision.current !== null && revision !== seenRevision.current) {
      if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        mainRef.current?.animate({ opacity: [0.6, 1] }, { duration: 220, easing: "ease-out" });
      }
    }
    seenRevision.current = revision;
  }, [revision]);

  // With a preview on screen, controls that illustrate something come first and
  // the rest sink below them — your call, over my assumption that they should
  // just sit wherever the agent put them. "Illustrates something" is broader
  // than "the preview frame can draw it": a control referenced by a `sample`
  // rule or a bound snippet changes the screen too, just not in the frame.
  const groups = useMemo(
    () => (spec ? groupControls(spec, { visualFirst: hasPreview, isVisual: (c) => controlIsIllustrated(spec, c) }) : []),
    [spec, hasPreview],
  );

  if (!spec) return <p className="muted">Loading…</p>;

  const submitComment = () => {
    const text = comment.trim();
    // An empty send is still meaningful: "come look at this".
    sendComment(text);
    setComment("");
  };

  // With a preview, side-by-side beats stacking: a wide single column wastes
  // half the screen and pushes the controls below the fold. Narrow viewports
  // fall back to stacked via the media query.
  const showPreview = hasPreview && Boolean(sessionId) && Boolean(previewOrigin);
  const split = showPreview && (spec.layout?.previewPosition ?? "right") === "right";

  return (
    <div className="session" data-split={split}>
      <header className="session-head">
        <div>
          <HomeLink exceptSessionId={sessionId} />
          <h1>{spec.title}</h1>
          {spec.description ? <p className="muted">{spec.description}</p> : null}
        </div>
        <StatusPill status={status} connection={connection} choice={choice} />
      </header>

      {showPreview ? (
        <aside className="session-side">
          <PreviewFrame sessionId={sessionId!} previewOrigin={previewOrigin!} />
        </aside>
      ) : null}

      <div className="session-main" ref={mainRef}>
      <ContentRenderer blocks={spec.content} />

      {groups.map(({ group, controls }) => (
        <section key={group || "_"} className="group">
          {group ? <h2 className="group-title">{group}</h2> : null}
          <div className="controls">
            {controls.map((control) => (
              <ControlRenderer key={control.id} control={control} disabled={done} />
            ))}
          </div>
        </section>
      ))}

      {/* Collapsed by default: a big empty textarea reads as an obligation to
          justify yourself, when the controls above are already the answer. */}
      {spec.comment.enabled && !done ? (
        <details className="comment-box" open={comment.length > 0}>
          <summary>{spec.comment.label ?? "Add a note (optional)"}</summary>
          <textarea
            id="comment"
            rows={2}
            placeholder={spec.comment.placeholder ?? "Anything the values above don't capture"}
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter") submitComment();
            }}
          />
        </details>
      ) : null}

      <footer className="action-bar">
        <span className="status-line">
          <ConnectionDot connection={connection} />
          {done ? (
            <span>{status === "finalized" ? "sent — safe to close this tab" : "session ended"}</span>
          ) : (
            <span>
              {connection === "open" ? "connected" : connection}
              {sentCount > 0 ? ` · ${sentCount} update${sentCount === 1 ? "" : "s"} sent` : ""}
            </span>
          )}
        </span>

        {!done ? (
          <span className="buttons">
            {sessionId ? <DismissSelf sessionId={sessionId} /> : null}
            {touchedCount > 0 ? (
              <button type="button" className="subtle" onClick={resetAll} title="Put every control back to what the agent suggested">
                Reset all
              </button>
            ) : null}
            {spec.comment.enabled ? (
              <button type="button" onClick={submitComment} title="Ctrl/Cmd+Enter">
                Send to agent
              </button>
            ) : null}

            {spec.actions.map((action) => (
              <button
                key={action.id}
                type="button"
                className={`btn-${action.style}`}
                onClick={() => {
                  if (comment.trim()) sendComment(comment.trim());
                  pressAction(action.id);
                }}
              >
                {action.label}
              </button>
            ))}

            {spec.actions.length === 0 ? (
              confirming ? (
                <>
                  <button type="button" onClick={() => setConfirming(false)}>
                    Cancel
                  </button>
                  <button
                    type="button"
                    className="btn-primary"
                    onClick={() => {
                      if (comment.trim()) sendComment(comment.trim());
                      finalize();
                    }}
                  >
                    Yes, finish
                  </button>
                </>
              ) : (
                <button type="button" className="btn-primary" onClick={() => setConfirming(true)}>
                  Done
                </button>
              )
            ) : null}
          </span>
        ) : null}
      </footer>
      </div>
    </div>
  );
}

/**
 * "I am not going to answer this."
 *
 * The dashboard's dismiss control is the one people reach for, but the decision
 * is usually made here — you open the question, read it, and want out. Without
 * this the only exits are answering something you do not mean or closing the
 * tab, and closing the tab leaves the agent still waiting.
 *
 * Two-step, because it releases whatever is parked on this session. The step in
 * between carries an optional reason, and that box is worth more than the
 * dismissal itself: "not now", "wrong question", "I already did this" each send
 * the agent somewhere different, and none of them was a button it thought to
 * offer. Optional to the last, though — a dismissal that demanded a
 * justification would just become one people avoid making.
 *
 * The reply needs no handling: dismissing appends a `closed` event, which the
 * socket relays back and the store turns into the read-only end state.
 */
function DismissSelf({ sessionId }: { sessionId: string }) {
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");

  function dismiss() {
    setBusy(true);
    void fetch(`/api/session/${encodeURIComponent(sessionId)}/dismiss`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ note: note.trim() }),
    }).finally(() => setBusy(false));
  }

  if (!armed) {
    return (
      <button
        type="button"
        className="subtle"
        onClick={() => setArmed(true)}
        title="Close this without answering — the agent is told you chose not to"
      >
        Dismiss
      </button>
    );
  }

  return (
    <>
      <input
        type="text"
        className="dismiss-note"
        // The reason is the point of the step, so the cursor starts in it —
        // the buttons are a Tab away either side.
        autoFocus
        maxLength={500}
        placeholder="Why? (optional)"
        aria-label="Reason for dismissing, optional"
        value={note}
        onChange={(e) => setNote(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") dismiss();
          if (e.key === "Escape") {
            setNote("");
            setArmed(false);
          }
        }}
      />
      <button
        type="button"
        className="subtle"
        onClick={() => {
          setNote("");
          setArmed(false);
        }}
      >
        Keep
      </button>
      <button type="button" className="btn-danger" disabled={busy} onClick={dismiss}>
        {busy ? "…" : note.trim() ? "Dismiss with reason" : "Dismiss without answering"}
      </button>
    </>
  );
}

function StatusPill({
  status,
  connection,
  choice,
}: {
  status: string;
  connection: string;
  choice: string | undefined;
}) {
  if (status === "finalized") return <span className="pill pill-done">answered{choice ? `: ${choice}` : ""}</span>;
  if (status === "closed") return <span className="pill">session ended</span>;
  if (connection !== "open") return <span className="pill pill-warn">{connection}</span>;
  return <span className="pill pill-active">the agent is waiting</span>;
}

function ConnectionDot({ connection }: { connection: string }) {
  return <span className={`dot dot-${connection}`} aria-hidden="true" />;
}

/**
 * The tab IS the notification — it is force-opened precisely to get attention —
 * so it has to keep signalling while input is still wanted, and stop the moment
 * it is not.
 */
function useTabAttention(title: string | undefined, status: string): void {
  const original = useRef(document.title);

  useEffect(() => {
    const pending = status === "active";
    document.title = title ? `${pending ? "● " : ""}${title} — AgentUI` : original.current;
    setFavicon(pending);
  }, [title, status]);

  useEffect(() => () => setFavicon(false), []);
}

function setFavicon(pending: boolean): void {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
    <rect width="32" height="32" rx="7" fill="${pending ? "#ef9f27" : "#888780"}"/>
    <rect x="7" y="14" width="18" height="4" rx="2" fill="#fff"/>
    ${pending ? '<circle cx="25" cy="7" r="5" fill="#e24b4a"/>' : ""}
  </svg>`;
  let link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
  if (!link) {
    link = document.createElement("link");
    link.rel = "icon";
    document.head.appendChild(link);
  }
  link.type = "image/svg+xml";
  link.href = `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
