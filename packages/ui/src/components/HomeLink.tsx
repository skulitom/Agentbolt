import { useEffect, useState } from "react";

/**
 * Way back to the inbox from inside a widget.
 *
 * It carries a count as well as a direction, because the useful thing to know
 * while answering one question is whether there are others waiting — otherwise
 * you finish, close the tab, and never learn that two more were queued behind
 * it. The count is deliberately of *other* widgets, so it reads as "what's left"
 * rather than including the one you are looking at.
 */
export function HomeLink({ exceptSessionId }: { exceptSessionId?: string | undefined }) {
  const [othersWaiting, setOthersWaiting] = useState(0);

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      try {
        // No `inbox=1`: reading this must not make the server think the inbox is
        // open, or it would stop opening tabs for new questions.
        const response = await fetch("/api/sessions");
        const body = (await response.json()) as {
          sessions: Array<{ sessionId: string; status: string; stale: boolean }>;
        };
        if (cancelled) return;
        setOthersWaiting(
          body.sessions.filter((s) => s.status === "active" && !s.stale && s.sessionId !== exceptSessionId).length,
        );
      } catch {
        // A count is a nicety; failing to fetch it must never break the widget.
      }
    };

    void load();
    const timer = window.setInterval(load, 5000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [exceptSessionId]);

  return (
    <a className="home-link" href="/" title="Back to all widgets">
      <span aria-hidden="true">←</span>
      <span>All widgets</span>
      {othersWaiting > 0 ? <span className="home-count">{othersWaiting} more waiting</span> : null}
    </a>
  );
}
