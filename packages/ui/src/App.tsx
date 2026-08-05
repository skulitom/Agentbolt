import { useEffect, useState } from "react";
import { sessionIdFromPath, useSession } from "./store.js";
import { WebSocketTransport, websocketUrlFor } from "./transport.js";
import { SessionView } from "./components/SessionView.js";
import { Dashboard } from "./components/Dashboard.js";
import { LibraryView } from "./components/LibraryView.js";

/**
 * Routing is client-side off location.pathname — "/" is the dashboard,
 * "/s/<id>" is one session. The server serves index.html for exactly those
 * paths, so an asset typo still 404s honestly instead of returning HTML.
 */
export function App() {
  const sessionId = sessionIdFromPath();
  if (window.location.pathname.startsWith("/library")) {
    return (
      <Shell>
        <LibraryView />
      </Shell>
    );
  }
  if (!sessionId) return <Shell><Dashboard /></Shell>;
  return (
    <Shell>
      <SessionBoot sessionId={sessionId} />
    </Shell>
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return <main className="shell">{children}</main>;
}

function SessionBoot({ sessionId }: { sessionId: string }) {
  const attach = useSession((s) => s.attach);
  const spec = useSession((s) => s.spec);
  const [error, setError] = useState<string>();

  useEffect(() => {
    let cancelled = false;

    // Fetch first so the page paints even if the socket is slow or blocked;
    // the socket's `init` is authoritative and overwrites this shortly after.
    void (async () => {
      try {
        const response = await fetch(`/api/session/${sessionId}`);
        if (!response.ok) throw new Error(response.status === 404 ? "no such session" : `HTTP ${response.status}`);
        const body = (await response.json()) as {
          spec: unknown;
          state: { values: Record<string, unknown>; status: string; revision: number; lastEventSeq: number };
          hasPreview: boolean;
          previewOrigin: string;
        };
        if (cancelled) return;
        useSession.setState({
          spec: body.spec as never,
          values: body.state.values as never,
          status: body.state.status as never,
          revision: body.state.revision,
          lastEventSeq: body.state.lastEventSeq,
          hasPreview: body.hasPreview,
          previewOrigin: body.previewOrigin,
        });
      } catch (err) {
        if (!cancelled) setError(String(err instanceof Error ? err.message : err));
      }
    })();

    const transport = new WebSocketTransport(websocketUrlFor(), sessionId, () => useSession.getState().lastEventSeq);
    attach(transport, sessionId);

    return () => {
      cancelled = true;
      transport.close();
    };
  }, [sessionId, attach]);

  if (error) {
    return (
      <div className="notice">
        <p>Could not load session {sessionId}: {error}</p>
        <p className="muted">
          It may have been closed, or the agent may have restarted. <a href="/">See all widgets</a>.
        </p>
      </div>
    );
  }

  if (!spec) return <p className="muted">Loading…</p>;
  return <SessionView />;
}
