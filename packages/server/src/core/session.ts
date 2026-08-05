import path from "node:path";
import fsp from "node:fs/promises";
import {
  defaultsFromSpec,
  normalizeValues,
  nowIso,
  shouldAutoPreview,
  AUTO_PREVIEW_MODULE,
  SessionComment,
  type InterfaceSpec,
  type SessionOutput,
  type SessionState,
  type SessionStatus,
  type Values,
} from "@agentui/shared";
import { EventLog } from "./event-log.js";
import { DebouncedJsonWriter, readJson, writeFileAtomic, writeJsonAtomic } from "./storage.js";
import { log } from "../log.js";

export interface SessionInit {
  sessionId: string;
  dir: string;
  spec: InterfaceSpec;
  previewModule?: string | undefined;
  template?: string | undefined;
  values?: Values | undefined;
  /** Continues the event sequence after a restart so held cursors stay valid. */
  startSeq?: number | undefined;
}

/**
 * One live widget instance: its spec, its values, its event log, and the
 * lifecycle active -> finalized -> closed.
 *
 * The server is authoritative for all of it. Browsers may have stale state
 * after a disconnect and agents may push values that no longer fit an updated
 * spec, so everything crossing the boundary is re-validated here.
 */
export class Session {
  readonly id: string;
  readonly dir: string;
  readonly events: EventLog;

  #spec: InterfaceSpec;
  #values: Values;
  #status: SessionStatus = "active";
  #revision = 0;
  #comments: SessionComment[] = [];
  #touchedIds = new Set<string>();
  #choice: string | undefined;
  #authoredPreview: boolean;
  #template: string | undefined;
  #createdAt = nowIso();
  #updatedAt = nowIso();
  #finalizedAt: string | undefined;
  #output: SessionOutput | undefined;
  #stateWriter: DebouncedJsonWriter;

  constructor(init: SessionInit) {
    this.id = init.sessionId;
    this.dir = init.dir;
    this.#spec = init.spec;
    this.#values = init.values ? normalizeValues(init.spec, init.values) : defaultsFromSpec(init.spec);
    this.#authoredPreview = Boolean(init.previewModule);
    this.#template = init.template;
    this.events = new EventLog(init.dir, init.startSeq ?? 0);
    this.#stateWriter = new DebouncedJsonWriter(path.join(init.dir, "state.json"));
  }

  /* ------------------------------------------------------------- accessors */

  get spec(): InterfaceSpec {
    return this.#spec;
  }
  get values(): Values {
    return this.#values;
  }
  get status(): SessionStatus {
    return this.#status;
  }
  get revision(): number {
    return this.#revision;
  }
  /** Whether the session shows a preview frame at all (authored or generated). */
  get hasPreview(): boolean {
    return this.#authoredPreview || shouldAutoPreview(this.#spec);
  }
  get comments(): readonly SessionComment[] {
    return this.#comments;
  }
  get choice(): string | undefined {
    return this.#choice;
  }
  get title(): string {
    return this.#spec.title;
  }
  get isOpen(): boolean {
    return this.#status === "active";
  }
  get createdAt(): string {
    return this.#createdAt;
  }
  /** Last change of any kind. Survives a restart, so it is what staleness is measured from. */
  get updatedAt(): string {
    return this.#updatedAt;
  }

  state(): SessionState {
    const state: SessionState = {
      schemaVersion: 1,
      sessionId: this.id,
      title: this.title,
      status: this.#status,
      values: this.#values,
      touched: [...this.#touchedIds],
      revision: this.#revision,
      lastEventSeq: this.events.lastSeq,
      createdAt: this.#createdAt,
      updatedAt: this.#updatedAt,
    };
    if (this.#finalizedAt !== undefined) state.finalizedAt = this.#finalizedAt;
    if (this.#choice !== undefined) state.choice = this.#choice;
    if (this.#template !== undefined) state.template = this.#template;
    return state;
  }

  /* ------------------------------------------------------------ persistence */

  async writeInitial(previewModule?: string): Promise<void> {
    await writeJsonAtomic(path.join(this.dir, "spec.json"), this.#spec);
    if (previewModule) await writeFileAtomic(path.join(this.dir, "preview.mjs"), previewModule);
    await this.#stateWriter.flush(this.state());
    this.events.append("session_created", { title: this.title, template: this.#template });
  }

  #touch(immediate: boolean): void {
    this.#updatedAt = nowIso();
    if (immediate) void this.#stateWriter.flush(this.state());
    else this.#stateWriter.schedule(this.state());
  }

  /* -------------------------------------------------------------- mutations */

  /**
   * `interim` is throttled drag noise: recorded, never a wake reason, and only
   * lazily flushed. `committed` is a real decision and is written immediately.
   */
  /**
   * `touched` is REPLACED, not merged.
   *
   * Clearing a flag matters as much as setting one — resetting a control to the
   * recommendation is how someone says "no opinion here" after poking at it, and
   * a merge would make that impossible to express. The client always sends the
   * complete list (and a reconnecting tab is seeded from this set via `init`),
   * so replacement is the coherent semantic.
   *
   * Doing it here rather than as a follow-up call also keeps it atomic: the
   * event payload and the state flush below must observe the post-reset set, not
   * a half-updated one.
   */
  setValues(incoming: Values, kind: "interim" | "committed", touched?: readonly string[]): Values {
    this.#values = normalizeValues(this.#spec, { ...this.#values, ...incoming });
    if (touched) this.#touchedIds = new Set(touched);
    this.events.append(kind === "interim" ? "values_interim" : "values_committed", {
      values: this.#values,
      touched: [...this.#touchedIds],
    });
    this.#touch(kind === "committed");
    return this.#values;
  }

  get touched(): string[] {
    return [...this.#touchedIds];
  }

  /** Values pushed by the agent via ui_update — broadcast, not a wake reason. */
  pushValues(incoming: Values): Values {
    this.#values = normalizeValues(this.#spec, { ...this.#values, ...incoming });
    this.#touch(true);
    return this.#values;
  }

  addComment(text: string): SessionComment {
    const comment: SessionComment = { ts: nowIso(), text };
    this.#comments.push(comment);
    this.events.append("comment", comment);
    this.#touch(true);
    return comment;
  }

  recordAction(actionId: string): void {
    this.events.append("action", { actionId });
    this.#touch(true);
  }

  async updateSpec(spec: InterfaceSpec, previewModule?: string | undefined): Promise<void> {
    this.#spec = spec;
    // Values for controls that still exist survive the update; the rest fall
    // back to the new defaults. Re-authoring a widget must not wipe the human's
    // work-in-progress.
    this.#values = normalizeValues(spec, this.#values);
    this.#revision += 1;
    if (previewModule !== undefined) {
      await writeFileAtomic(path.join(this.dir, "preview.mjs"), previewModule);
      this.#authoredPreview = true;
    }
    await writeJsonAtomic(path.join(this.dir, "spec.json"), spec);
    this.events.append("spec_updated", { revision: this.#revision });
    this.#touch(true);
  }

  recordPreviewEvent(name: string, data: unknown): void {
    this.events.append("preview_event", { name, data });
  }

  recordPreviewError(message: string, stack?: string): void {
    this.events.append("preview_error", { message, stack });
    this.#touch(true);
  }

  /* -------------------------------------------------------------- lifecycle */

  /**
   * Idempotent by contract: a double-click, or a reconnect racing the first
   * finalize, must return the output that was already written rather than
   * rewriting it or erroring.
   */
  async finalize(options: { values?: Values | undefined; choice?: string | undefined } = {}): Promise<SessionOutput> {
    if (this.#output) return this.#output;

    if (options.values) this.#values = normalizeValues(this.#spec, { ...this.#values, ...options.values });
    if (options.choice !== undefined) this.#choice = options.choice;

    this.#status = "finalized";
    this.#finalizedAt = nowIso();

    const output: SessionOutput = {
      schemaVersion: 1,
      sessionId: this.id,
      title: this.title,
      finalizedAt: this.#finalizedAt,
      status: "answered",
      values: this.#values,
      touched: [...this.#touchedIds],
      untouched: this.#spec.controls.map((c) => c.id).filter((id) => !this.#touchedIds.has(id)),
      comments: [...this.#comments],
      eventCount: this.events.lastSeq,
    };
    if (this.#choice !== undefined) output.choice = this.#choice;

    this.#output = output;
    await writeJsonAtomic(path.join(this.dir, "output.json"), output);
    this.events.append("finalized", { choice: this.#choice });
    await this.#stateWriter.flush(this.state());
    return output;
  }

  async close(): Promise<void> {
    if (this.#status === "closed") return;
    this.#status = "closed";
    this.events.append("closed");
    await this.#stateWriter.flush(this.state());
    this.#stateWriter.dispose();
  }

  async output(): Promise<SessionOutput | undefined> {
    if (this.#output) return this.#output;
    const fromDisk = await readJson<SessionOutput>(path.join(this.dir, "output.json"));
    if (fromDisk) this.#output = fromDisk;
    return this.#output;
  }

  /**
   * The authored preview module, or a generated one when the widget has values
   * with a visible magnitude and the agent did not supply its own. Live feedback
   * is the point of the tool, so it should not depend on the agent remembering.
   */
  async previewModule(): Promise<string | undefined> {
    if (this.#authoredPreview) {
      try {
        return await fsp.readFile(path.join(this.dir, "preview.mjs"), "utf8");
      } catch (error) {
        log.warn("preview module missing", { session: this.id, error: String(error) });
        return undefined;
      }
    }
    return shouldAutoPreview(this.#spec) ? AUTO_PREVIEW_MODULE : undefined;
  }

  /** True when the agent wrote the preview, as opposed to one being generated. */
  get hasAuthoredPreview(): boolean {
    return this.#authoredPreview;
  }

  /** Restores a session from disk after an MCP restart. */
  static async hydrate(dir: string, sessionId: string): Promise<Session | undefined> {
    const state = await readJson<SessionState>(path.join(dir, "state.json"));
    const spec = await readJson<InterfaceSpec>(path.join(dir, "spec.json"));
    if (!state || !spec) return undefined;

    const session = new Session({
      sessionId,
      dir,
      spec,
      values: state.values as Values,
      startSeq: state.lastEventSeq,
    });
    session.#status = state.status;
    session.#revision = state.revision;
    session.#createdAt = state.createdAt;
    session.#updatedAt = state.updatedAt;
    session.#finalizedAt = state.finalizedAt;
    session.#choice = state.choice;
    session.#template = state.template;
    session.#touchedIds = new Set(state.touched ?? []);
    session.#authoredPreview = await fsp
      .access(path.join(dir, "preview.mjs"))
      .then(() => true)
      .catch(() => false);

    // Comments live only in memory and the event log — state.json does not carry
    // them. Rebuild from the log, or a comment made before a restart would
    // silently vanish from the finalize output while still sitting in
    // events.ndjson. Best-effort: an unreadable log must not block hydration.
    try {
      for (const event of await session.events.readAll()) {
        if (event.type !== "comment") continue;
        const comment = SessionComment.safeParse(event.payload);
        if (comment.success) session.#comments.push(comment.data);
      }
    } catch (error) {
      log.warn("could not restore comments from event log", { session: sessionId, error: String(error) });
    }
    return session;
  }
}
