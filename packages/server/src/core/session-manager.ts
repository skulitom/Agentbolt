import path from "node:path";
import fsp from "node:fs/promises";
import { customAlphabet } from "nanoid";
import {
  type ClientMessage,
  type ContentBlock,
  type InterfaceSpec,
  type ServerMessage,
  type SessionStatus,
  type Values,
} from "@agentui/shared";
import { Session, type SessionCloseDetails } from "./session.js";
import { dataDir, sessionsDir } from "./paths.js";
import { ensureDir, ensureSelfIgnoringDir, existsSync } from "./storage.js";
import { staleAfterMs, stalenessOf } from "./staleness.js";
import { log } from "../log.js";

// Readable aloud, and long enough not to collide across a working day.
const newId = customAlphabet("23456789abcdefghijkmnpqrstuvwxyz", 10);

/**
 * Where a message should go after the session has processed it. The manager
 * decides *what* to send; the transport decides *how*.
 */
export interface Outbound {
  to: "all" | "others" | "sender";
  message: ServerMessage;
}

export interface CreateOptions {
  spec: InterfaceSpec;
  previewModule?: string | undefined;
  values?: Values | undefined;
  template?: string | undefined;
}

/**
 * One row of the session list, shared verbatim by /api/sessions and
 * ui_get_state's list mode.
 *
 * Single source on purpose: the dashboard and the agent disagreeing about what
 * is still waiting is exactly the bug this shape exists to prevent, and two
 * hand-written mappings would drift the first time either surface changed.
 */
export interface SessionSummary {
  sessionId: string;
  title: string;
  /** The persisted lifecycle — active | finalized | closed. Never "stale". */
  status: SessionStatus;
  /** Derived: active, but idle long enough that nobody is plausibly waiting. See staleness.ts. */
  stale: boolean;
  idleMs: number | undefined;
  idleLabel: string;
  createdAt: string;
  updatedAt: string;
  lastEventSeq: number;
  hasPreview: boolean;
  /**
   * Buttons for a question that can be answered in one click, with nothing to
   * fill in and nothing to look at.
   *
   * Present so the inbox can resolve it in place. Most questions an agent asks
   * are exactly this shape, and making them cost a page navigation is what
   * turned "ask the user" into a worse trade than typing the question into chat.
   * Empty for anything with controls or a preview — those genuinely need the
   * room, and answering them blind would be worse than the navigation.
   */
  quickChoices: Array<{ id: string; label: string; style: string }>;
}

/**
 * A question is answerable from the inbox when pressing a button is the whole
 * interaction: buttons exist, there is nothing to fill in, and there is nothing
 * to look at first. Anything else keeps its own page.
 */
function quickChoicesFor(session: Session): SessionSummary["quickChoices"] {
  if (session.status !== "active") return [];
  const spec = session.spec;
  // Prose is fine to show inline; a diff, image or code block is the reason the
  // question was asked visually in the first place and must not be skipped past.
  // A BOUND markdown block is one of those: it is prose that changes as you
  // type, which is a thing to look at rather than a thing to read past.
  const needsLookingAt = spec.content.some((block) => block.kind !== "markdown" || block.bind);
  if (!spec.actions.length || spec.controls.length > 0 || needsLookingAt) return [];
  return spec.actions.map((action) => ({ id: action.id, label: action.label, style: action.style }));
}

/** Waiting first, then stale, then finished; freshest first inside each group. */
function listRank(summary: SessionSummary): number {
  if (summary.status !== "active") return 2;
  return summary.stale ? 1 : 0;
}

/**
 * Owns every live session in this process.
 *
 * Deliberately transport-agnostic: `handleClientMessage` takes a session id and
 * a validated message and returns what should be broadcast. It never sees a
 * WebSocket. That is what lets an HTTP POST adapter — or, if the inline-widget
 * spike ever passes, an MCP tools/call adapter — reuse the exact same logic
 * instead of forking the event pipeline.
 */
export class SessionManager {
  readonly #sessions = new Map<string, Session>();
  #hydrated = false;

  get size(): number {
    return this.#sessions.size;
  }

  list(): Session[] {
    return [...this.#sessions.values()];
  }

  get(sessionId: string): Session | undefined {
    return this.#sessions.get(sessionId);
  }

  /**
   * Sessions genuinely still awaiting a human. Used for the dashboard badge and
   * /api/ping.
   *
   * Stale ones are excluded deliberately: a badge that counts leftovers from
   * last week's runs stops meaning anything, which is the whole reason anyone
   * ignores it.
   */
  pending(): Session[] {
    const threshold = staleAfterMs();
    const now = Date.now();
    return this.list().filter((s) => s.status === "active" && !stalenessOf(s.updatedAt, now, threshold).stale);
  }

  /** Open sessions that have gone quiet long enough that nobody is likely to answer them. */
  stale(): Session[] {
    const threshold = staleAfterMs();
    const now = Date.now();
    return this.list().filter((s) => s.status === "active" && stalenessOf(s.updatedAt, now, threshold).stale);
  }

  summarize(session: Session, now = Date.now(), thresholdMs = staleAfterMs()): SessionSummary {
    const { idleMs, idleLabel, stale } = stalenessOf(session.updatedAt, now, thresholdMs);
    return {
      sessionId: session.id,
      title: session.title,
      status: session.status,
      // Only an open session can be stale; a finished one is already sorted.
      stale: session.status === "active" && stale,
      idleMs,
      idleLabel,
      createdAt: session.createdAt,
      updatedAt: session.updatedAt,
      lastEventSeq: session.events.lastSeq,
      hasPreview: session.hasPreview,
      quickChoices: quickChoicesFor(session),
    };
  }

  /** Every session, classified and ordered once, for both list surfaces. */
  summaries(now = Date.now()): SessionSummary[] {
    const threshold = staleAfterMs();
    return this.list()
      .map((session) => this.summarize(session, now, threshold))
      .sort((a, b) => listRank(a) - listRank(b) || (a.idleMs ?? Infinity) - (b.idleMs ?? Infinity));
  }

  /**
   * Closes every stale session at once — the dashboard's "clear all".
   *
   * User-initiated by design. Doing this automatically on hydrate would be the
   * one irreversible reading of the same signal, and it would fire on exactly
   * the sessions whose owner had not yet had a chance to disagree.
   */
  async closeStale(): Promise<string[]> {
    const ids = this.stale().map((session) => session.id);
    // Same act as dismissing one by hand, so it carries the same reason: every
    // one of these is an open question the user has just declined.
    for (const id of ids) await this.close(id, false, { reason: "dismissed" });
    if (ids.length) log.info("cleared stale sessions", { cleared: ids.length });
    return ids;
  }

  async create(options: CreateOptions): Promise<Session> {
    await ensureSelfIgnoringDir(dataDir());

    const sessionId = newId();
    const dir = path.join(sessionsDir(), sessionId);
    await ensureDir(dir);

    // Image blocks may reference files anywhere on disk; copy them in so the
    // session is self-contained and the HTTP layer never serves an arbitrary path.
    const spec = await this.#materializeAssets(options.spec, dir, sessionId);

    const session = new Session({
      sessionId,
      dir,
      spec,
      previewModule: options.previewModule,
      template: options.template,
      values: options.values,
    });
    await session.writeInitial(options.previewModule);

    this.#sessions.set(sessionId, session);
    log.info("session created", { sessionId, title: session.title });
    return session;
  }

  /**
   * Copies `image.path` blocks into the session's assets dir and rewrites them
   * to a served URL. Anything unreadable degrades to a caption rather than
   * failing the whole widget — a missing screenshot should not block a question.
   */
  async #materializeAssets(spec: InterfaceSpec, dir: string, sessionId: string): Promise<InterfaceSpec> {
    if (!spec.content.some((block) => block.kind === "image" && block.path)) return spec;

    const assetsDir = path.join(dir, "assets");
    await ensureDir(assetsDir);

    const content: ContentBlock[] = [];
    let index = 0;
    for (const block of spec.content) {
      if (block.kind !== "image" || !block.path) {
        content.push(block);
        continue;
      }
      const source = path.resolve(block.path);
      const ext = path.extname(source).toLowerCase() || ".png";
      const name = `${index++}${ext}`;
      try {
        await fsp.copyFile(source, path.join(assetsDir, name));
        // Served from the session's own asset route, never from an arbitrary path.
        const rewritten: ContentBlock = { kind: "image", path: `/s/${sessionId}/assets/${name}` };
        if (block.alt !== undefined) rewritten.alt = block.alt;
        if (block.caption !== undefined) rewritten.caption = block.caption;
        content.push(rewritten);
      } catch (error) {
        log.warn("could not read image for content block", { path: source, error: String(error) });
        content.push({
          kind: "markdown",
          value: `_(image unavailable: ${path.basename(source)})_`,
          bind: false,
        });
      }
    }
    return { ...spec, content };
  }

  /**
   * Handles one validated message from any client of a session.
   * Returns what to broadcast; the caller owns delivery.
   */
  async handleClientMessage(sessionId: string, message: ClientMessage): Promise<Outbound[]> {
    const session = this.#sessions.get(sessionId);
    if (!session) return [{ to: "sender", message: { type: "error", message: `unknown session ${sessionId}` } }];

    if (message.type === "ping") return [{ to: "sender", message: { type: "pong" } }];

    if (message.type === "hello") {
      return [{ to: "sender", message: this.initMessage(session) }];
    }

    // Everything below mutates. A finalized or closed session is read-only —
    // a stale tab must not be able to overwrite an answer already delivered.
    if (!session.isOpen) {
      return [
        {
          to: "sender",
          message: { type: "error", message: `session is ${session.status}; no further changes accepted` },
        },
      ];
    }

    switch (message.type) {
      case "values": {
        const values = session.setValues(message.values as Values, message.kind, message.touched);
        // Interim frames are echoed to *other* tabs only, so the person dragging
        // never fights their own input.
        return [{ to: "others", message: { type: "values_pushed", values } }];
      }
      case "comment": {
        session.addComment(message.text);
        return [];
      }
      case "action": {
        if (message.values) session.setValues(message.values as Values, "committed", message.touched);
        session.recordAction(message.actionId);
        await session.finalize({ choice: message.actionId });
        return [{ to: "all", message: { type: "finalized", choice: message.actionId } }];
      }
      case "finalize": {
        if (message.touched) session.setValues({}, "committed", message.touched);
        await session.finalize({ values: message.values as Values | undefined });
        return [{ to: "all", message: { type: "finalized" } }];
      }
      case "preview_event": {
        session.recordPreviewEvent(message.name, message.data);
        return [];
      }
      case "preview_error": {
        // Straight into the event log, so the agent that wrote the broken module
        // sees its own stack trace and can fix it with ui_update.
        session.recordPreviewError(message.message, message.stack);
        return [];
      }
    }
  }

  initMessage(session: Session): ServerMessage {
    const message: ServerMessage = {
      type: "init",
      sessionId: session.id,
      spec: session.spec,
      values: session.values,
      touched: session.touched,
      status: session.status,
      revision: session.revision,
      lastEventSeq: session.events.lastSeq,
      hasPreview: session.hasPreview,
    };
    if (session.choice !== undefined) (message as { choice?: string }).choice = session.choice;
    return message;
  }

  async close(sessionId: string, deleteFiles = false, details: SessionCloseDetails = {}): Promise<boolean> {
    const session = this.#sessions.get(sessionId);
    if (!session) return false;
    // close() appends a `closed` event, which is what releases parked waiters —
    // an agent must never be left blocked on a session that no longer exists.
    await session.close(details);
    this.#sessions.delete(sessionId);
    if (deleteFiles) await fsp.rm(session.dir, { recursive: true, force: true }).catch(() => {});
    return true;
  }

  /**
   * The user's own "no, not this one" — a single session closed by hand from the
   * dashboard or from its own page.
   *
   * It is a close like any other, so the tabs go read-only and any parked
   * ui_wait is released rather than left hanging on a row that has vanished.
   * What differs is what the event carries: declining to answer is real
   * information, and an agent told only "closed" would have no way to tell it
   * from its own ui_close and would happily ask again. `note` is the rest of
   * that information — "not now", "wrong question", "I already did it" each
   * lead somewhere different, and none of them is a button the agent thought to
   * offer.
   */
  async dismiss(sessionId: string, note?: string): Promise<{ dismissed: boolean; wasWaiting: boolean }> {
    const session = this.#sessions.get(sessionId);
    if (!session) return { dismissed: false, wasWaiting: false };
    // A finished session has already given its answer; removing its row is
    // tidying, not refusal, and must not be reported as one.
    const wasWaiting = session.isOpen;
    await this.close(sessionId, false, { reason: wasWaiting ? "dismissed" : "cleared", note });
    log.info("session dismissed by the user", { sessionId, wasWaiting, withNote: Boolean(note?.trim()) });
    return { dismissed: true, wasWaiting };
  }

  /**
   * Restores sessions from disk after an MCP restart. A corrupt session dir logs
   * and is skipped — never a crash at boot, because that would take down every
   * other session too.
   *
   * Age is NOT a hydration filter, and that is the deliberate half of the stale
   * policy. Skipping old sessions here would 404 a bookmarked URL while
   * state.json still said "active", and it would have to guess — at the one
   * moment it has the least information — which restart is a crash recovery a
   * minute later and which is a new day. Everything comes back; `summaries()`
   * then says plainly which ones are still worth your attention.
   */
  async hydrate(): Promise<number> {
    if (this.#hydrated) return this.#sessions.size;
    this.#hydrated = true;

    const root = sessionsDir();
    if (!existsSync(root)) return 0;

    let restored = 0;
    for (const entry of await fsp.readdir(root, { withFileTypes: true }).catch(() => [])) {
      if (!entry.isDirectory()) continue;
      try {
        const session = await Session.hydrate(path.join(root, entry.name), entry.name);
        if (!session) continue;
        if (session.status === "closed") continue;
        this.#sessions.set(entry.name, session);
        restored++;
      } catch (error) {
        log.warn("skipping unreadable session", { session: entry.name, error: String(error) });
      }
    }
    if (restored) log.info("restored sessions from disk", { restored, stale: this.stale().length });
    return restored;
  }
}
