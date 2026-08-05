import http from "node:http";
import path from "node:path";
import express, { type Express } from "express";
import { projectName, webRoot } from "../core/paths.js";
import { HOST, candidateOriginsAllProjects, listenOnAvailablePair } from "../core/ports.js";
import type { SessionManager } from "../core/session-manager.js";
import type { Library } from "../core/library.js";
import { createPreviewApp } from "./preview.js";
import { crossOriginWriteGuard, hostGuard, peerCors, trustOwnOrigins } from "./security.js";
import { log } from "../log.js";

export interface WebServer {
  server: http.Server;
  app: Express;
  port: number;
  origin: string;
  /** Second loopback origin (port + 1) that hosts agent-authored preview code. */
  previewOrigin: string;
  sessionUrl(sessionId: string): string;
  /** True when the user has the inbox open, so a new question needs no new tab. */
  inboxIsOpen(): boolean;
}

/** How long after its last poll the inbox is still assumed to be open. */
const INBOX_PRESENCE_MS = 8_000;

/**
 * The local web server. Started lazily on the first UI call and never stopped —
 * it dies with the process, which is the whole lifecycle. No child processes
 * anywhere, so there is nothing to orphan.
 */
export async function startWebServer(
  sessions: SessionManager,
  library: Library,
  onSessionCreated?: (sessionId: string) => void,
): Promise<WebServer> {
  const app = express();
  const root = webRoot();
  // The preview port is only known once both listeners bind, but routes are
  // registered before that; this box lets them read it afterwards.
  const previewPortRef = { value: 0 };
  let lastInboxPollAt = 0;

  app.disable("x-powered-by");
  app.use(hostGuard);
  app.use(crossOriginWriteGuard);
  app.use(express.json({ limit: "8mb" }));

  /* ------------------------------------------------------------------ api */

  // Peer discovery: neighbouring instances answer this so one dashboard can
  // show sessions from every project you have an agent running in.
  /**
   * Peers: the other AgentUI instances on this machine, one per project with an
   * agent running.
   *
   * Probing happens here rather than in the browser so the page needs no
   * cross-origin permission, and so a project whose dashboard you are NOT
   * looking at can still surface a waiting question. Each instance's port is
   * derived from its project path, so the candidate list is known without any
   * registry or discovery protocol.
   */
  app.get("/api/peers", async (_req, res) => {
    const mine = previewPortRef.value - 1;
    const candidates = candidateOriginsAllProjects()
      .filter((origin) => origin.startsWith("http://"))
      .map((origin) => Number(new URL(origin).port))
      .filter((port) => port % 2 === 0 && port !== mine);

    const peers = await Promise.all(
      candidates.map(async (port) => {
        try {
          const controller = new AbortController();
          const timer = setTimeout(() => controller.abort(), 250);
          const response = await fetch(`http://${HOST}:${port}/api/ping`, { signal: controller.signal });
          clearTimeout(timer);
          if (!response.ok) return undefined;
          const body = (await response.json()) as { app?: string; project?: string; pending?: number };
          if (body.app !== "agentui") return undefined;
          return { port, project: body.project ?? "?", pending: body.pending ?? 0, url: `http://${HOST}:${port}` };
        } catch {
          return undefined; // Nothing there, which is the common case.
        }
      }),
    );

    res.json({ peers: peers.filter(Boolean) });
  });

  app.get("/api/ping", peerCors, (_req, res) => {
    res.json({
      app: "agentui",
      project: projectName(),
      // Excludes stale sessions, so a neighbouring dashboard's badge counts
      // people actually waiting rather than last week's leftovers.
      pending: sessions.pending().length,
      stale: sessions.stale().length,
      sessions: sessions.size,
    });
  });

  app.get("/api/sessions", (req, res) => {
    // Only the inbox's own polling counts as presence. A session page also reads
    // this (to show how many other widgets are waiting), and letting that set the
    // flag would suppress tab-opening for 8s after the last tab closed — exactly
    // when a new question most needs a tab.
    if (req.query["inbox"] === "1") lastInboxPollAt = Date.now();
    res.json({
      project: projectName(),
      // The same summaries ui_get_state's list mode returns. Both surfaces must
      // agree on what is still waiting, so neither computes it for itself.
      sessions: sessions.summaries().map((summary) => ({ ...summary, url: `/s/${summary.sessionId}` })),
    });
  });

  // The dashboard's "clear all". Closing is the honest end state — the tabs go
  // read-only and any parked ui_wait is released — and it stays a human's call
  // rather than something hydration does behind their back.
  app.post("/api/sessions/stale/close", async (_req, res) => {
    const closed = await sessions.closeStale();
    res.json({ closed, count: closed.length });
  });

  /**
   * Dismiss one session by hand — the per-row control on the dashboard, and the
   * quiet button on a session page.
   *
   * "Clear all" only ever reached the leftovers the *server* had judged stale,
   * which left the two rows a person most wants gone with no way out: a question
   * you have decided not to answer, and a finished one you have already read.
   * Dismissing is a close, so the tabs go read-only and any parked ui_wait is
   * released with the reason on the event — an agent is never left waiting on a
   * row that is no longer on screen.
   */
  app.post("/api/session/:id/dismiss", async (req, res) => {
    const sessionId = String(req.params["id"]);
    // Optional to the last: a dismissal that demanded a justification would just
    // become a dismissal people avoid making.
    const note = typeof req.body?.note === "string" ? req.body.note : undefined;
    const { dismissed, wasWaiting } = await sessions.dismiss(sessionId, note);
    if (!dismissed) {
      res.status(404).json({ error: "no such session" });
      return;
    }
    res.json({ ok: true, sessionId, wasWaiting });
  });

  /**
   * Answer a one-click question straight from the inbox.
   *
   * Routed through the same handleClientMessage the WebSocket uses, so an
   * answer given here is indistinguishable from one given in the session page:
   * same events, same waiters woken, same output.json. That is exactly what the
   * transport-agnostic message layer was for.
   */
  app.post("/api/session/:id/answer", async (req, res) => {
    const sessionId = String(req.params["id"]);
    const actionId = String(req.body?.actionId ?? "");
    const session = sessions.get(sessionId);
    if (!session) {
      res.status(404).json({ error: "no such session" });
      return;
    }
    if (!session.spec.actions.some((action) => action.id === actionId)) {
      res.status(400).json({ error: `"${actionId}" is not one of this widget's actions` });
      return;
    }
    await sessions.handleClientMessage(sessionId, { type: "action", actionId });
    res.json({ ok: true, sessionId, choice: actionId });
  });

  app.get("/api/session/:id", (req, res) => {
    const session = sessions.get(String(req.params["id"]));
    if (!session) {
      res.status(404).json({ error: "no such session" });
      return;
    }
    res.json({
      spec: session.spec,
      state: session.state(),
      hasPreview: session.hasPreview,
      previewOrigin: `http://${HOST}:${previewPortRef.value}`,
    });
  });

  /* ------------------------------------------------------------- library */

  app.get("/api/library", async (req, res) => {
    const query = typeof req.query["q"] === "string" ? req.query["q"] : "";
    const { results, total } = await library.search(query, 60);
    res.json({ results, total });
  });

  app.post("/api/library/:slug/pin", async (req, res) => {
    const pinned = req.body?.pinned !== false;
    const ok = await library.setPinned(String(req.params["slug"]), pinned);
    res.status(ok ? 200 : 404).json({ ok, pinned });
  });

  app.delete("/api/library/:slug", async (req, res) => {
    const ok = await library.remove(String(req.params["slug"]));
    res.status(ok ? 200 : 404).json({ ok });
  });

  /**
   * Launches a saved widget without an agent involved — the human side of the
   * library. It still writes output.json, so an agent can be pointed at the
   * result afterwards.
   */
  app.post("/api/library/:slug/open", async (req, res) => {
    const slug = String(req.params["slug"]);
    const saved = await library.load(slug);
    if (!saved) {
      res.status(404).json({ error: "no such widget" });
      return;
    }
    const session = await sessions.create({
      spec: saved.widget.spec,
      previewModule: await library.previewModule(slug),
      values: saved.widget.lastValues as never,
      template: slug,
    });
    onSessionCreated?.(session.id);
    await library.recordUse(slug);
    res.json({ sessionId: session.id, url: `/s/${session.id}` });
  });

  /* -------------------------------------------------------------- assets */

  // Session-scoped assets (images an agent attached). Registered BEFORE the
  // SPA route so it is not swallowed by it — Express 5 matches in order.
  app.use(
    "/s/:id/assets",
    (req, res, next) => {
      const session = sessions.get(String(req.params["id"]));
      if (!session) {
        res.status(404).end();
        return;
      }
      // express.static resolves within the root and rejects traversal itself.
      express.static(path.join(session.dir, "assets"), { fallthrough: false, index: false })(req, res, next);
    },
  );

  // NOTE: `/p/:id.mjs` is deliberately NOT served here. Preview modules are
  // agent-authored code and belong to the preview origin only (see preview.ts),
  // which serves them under `Content-Security-Policy: default-src 'none'`. A
  // second copy on the shell origin was reachable without that header.

  // Built shell: hashed assets and the vendored three.js.
  app.use("/assets", express.static(path.join(root, "assets"), { immutable: true, maxAge: "1y" }));
  app.use("/vendor", express.static(path.join(root, "vendor"), { immutable: true, maxAge: "1y" }));
  app.use(express.static(root, { index: false }));

  /* ----------------------------------------------------------------- spa */

  // Client-side routes. Explicit rather than a catch-all wildcard, so a typo in
  // an asset path 404s honestly instead of silently returning HTML.
  const spa = (_req: express.Request, res: express.Response) => {
    res.setHeader("Cache-Control", "no-store");
    res.sendFile(path.join(root, "index.html"));
  };
  app.get("/", spa);
  app.get("/library", spa);
  app.get("/s/:id", spa);

  app.use((_req, res) => {
    res.status(404).type("text/plain").send("not found\n");
  });

  const server = http.createServer(app);
  const previewServer = http.createServer(createPreviewApp(sessions));

  const { shellPort, previewPort } = await listenOnAvailablePair(server, previewServer);
  previewPortRef.value = previewPort;
  // Whatever we actually bound is by definition our own origin, even if
  // AGENT_UI_PORT put us outside the derived range.
  trustOwnOrigins(shellPort, previewPort);
  const origin = `http://${HOST}:${shellPort}`;
  const previewOrigin = `http://${HOST}:${previewPort}`;

  log.info("web server ready", { origin, previewOrigin, project: projectName() });

  return {
    server,
    app,
    port: shellPort,
    origin,
    previewOrigin,
    sessionUrl: (sessionId: string) => `${origin}/s/${sessionId}`,
    inboxIsOpen: () => Date.now() - lastInboxPollAt < INBOX_PRESENCE_MS,
  };
}
