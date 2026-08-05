import open from "open";
import { SessionManager } from "../core/session-manager.js";
import { Waiters } from "../core/waiters.js";
import { Library } from "../core/library.js";
import { parseSpec } from "@agentui/shared";
import { startWebServer, type WebServer } from "../web/http.js";
import { anyClientConnected, attachWebSocket, connectedClientCount, relaySessionEvent } from "../web/ws.js";
import { log } from "../log.js";

/**
 * Shared runtime for the MCP tools.
 *
 * The web server starts lazily on the first UI call, so a session where nobody
 * asks for a widget costs nothing but an idle stdio process — which is what
 * makes it reasonable to install this globally and forget about it.
 */
export class AgentUiContext {
  readonly sessions = new SessionManager();
  readonly waiters = new Waiters(this.sessions);
  readonly library = new Library();
  #web: WebServer | undefined;
  #starting: Promise<WebServer> | undefined;
  #onLibraryChange: (() => void) | undefined;
  #watched = new Set<string>();

  /** Re-exported so tools share one spec parser and one error format. */
  readonly parseSpec = parseSpec;

  onLibraryChange(handler: () => void): void {
    this.#onLibraryChange = handler;
  }

  /**
   * Refreshes the shortlist folded into the ui_create description after a save.
   * Best-effort: many clients fetch tools/list once at startup and never again,
   * so the startup snapshot is what actually carries discovery. A widget saved
   * during this session is already known to the agent that saved it.
   */
  async refreshLibraryIndex(): Promise<void> {
    this.#onLibraryChange?.();
  }

  async web(): Promise<WebServer> {
    if (this.#web) return this.#web;
    // Concurrent first calls must not race two servers onto two ports.
    this.#starting ??= (async () => {
      // A widget the human launches themselves from /library must be watched
      // exactly like an agent-created one, or nothing would wake a parked wait.
      const web = await startWebServer(this.sessions, this.library, (sessionId) => this.watch(sessionId));
      attachWebSocket(web.server, this.sessions);
      this.#web = web;
      return web;
    })();
    return this.#starting;
  }

  get origin(): string | undefined {
    return this.#web?.origin;
  }

  /** Absolute URL for a session, starting the web server if needed. */
  async sessionUrl(sessionId: string): Promise<string> {
    const web = await this.web();
    return web.sessionUrl(sessionId);
  }

  async dashboardUrl(): Promise<string> {
    const web = await this.web();
    return web.origin;
  }

  connectedClients(sessionId: string): number {
    return connectedClientCount(sessionId);
  }

  /**
   * Restores sessions from disk and wires each into the event machinery.
   *
   * The wiring half is the part that must not be skipped: hydration alone
   * rebuilds the session objects, but without watch() nothing would wake a
   * parked ui_wait or mirror events to tabs — a restored session would look
   * alive while the agent slept through every response until its timeout.
   */
  async hydrate(): Promise<number> {
    const restored = await this.sessions.hydrate();
    for (const session of this.sessions.list()) this.watch(session.id);
    return restored;
  }

  /**
   * Mirrors session events to attached tabs and to any parked ui_wait.
   * Idempotent: a session watched twice (created, then swept by hydrate's
   * watch-all, or any future path) must not double-deliver every event.
   */
  watch(sessionId: string): void {
    if (this.#watched.has(sessionId)) return;
    const session = this.sessions.get(sessionId);
    if (!session) return;
    this.#watched.add(sessionId);
    session.events.onAppend((event) => {
      relaySessionEvent(sessionId, event);
      this.waiters.notify(sessionId, event);
    });
  }

  /**
   * Opens the human's browser at a session — but only when they do not already
   * have AgentUI open.
   *
   * This is the difference between a tool an agent reaches for and one it
   * avoids. Asking is cheap for the agent (a few milliseconds) and expensive for
   * the human (a context switch), so three small questions used to cost three
   * tabs — at which point typing the questions into chat is simply the better
   * trade, and the tool goes unused. When something is already open, the new
   * question surfaces there instead and the marginal cost of asking drops to
   * roughly zero.
   *
   * Never throws: headless boxes, SSH and WSL without a browser are normal, and
   * there the URL in the result is the whole answer.
   */
  async openInBrowser(url: string): Promise<{ opened: boolean; reason: "opened" | "already-open" | "suppressed" | "failed" }> {
    if (process.env["AGENT_UI_NO_OPEN"] === "1") return { opened: false, reason: "suppressed" };

    const web = this.#web;
    if (web && (web.inboxIsOpen() || anyClientConnected() > 0)) {
      log.info("not opening a tab — AgentUI is already open", { url });
      return { opened: false, reason: "already-open" };
    }

    // Takes the real decision but stops short of launching, so the tab policy
    // can be measured in tests without throwing browser windows at whoever is
    // running them.
    if (process.env["AGENT_UI_OPEN_DRY_RUN"] === "1") return { opened: true, reason: "opened" };

    try {
      await open(url);
      return { opened: true, reason: "opened" };
    } catch (error) {
      log.warn("could not open a browser; the URL still works if opened manually", { url, error: String(error) });
      return { opened: false, reason: "failed" };
    }
  }
}
