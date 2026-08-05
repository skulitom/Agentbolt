import path from "node:path";
import { type EventType, type SessionEvent } from "@agentui/shared";
import { appendLine, readLines } from "./storage.js";
import { log } from "../log.js";

/**
 * Append-only event log, one JSON object per line.
 *
 * `sinceSeq` is strictly exclusive everywhere: a caller passing `sinceSeq: 7`
 * gets events with `seq > 7`. Every response that carries events also carries
 * `lastEventSeq`, so the caller always holds its own next cursor and can never
 * miss an event between two polls.
 */
export class EventLog {
  #seq: number;
  #recent: SessionEvent[] = [];
  #listeners = new Set<(event: SessionEvent) => void>();
  readonly #file: string;

  /** How many events to hold in memory for cheap `since` queries. */
  static readonly WINDOW = 500;

  constructor(sessionDir: string, startSeq = 0) {
    this.#file = path.join(sessionDir, "events.ndjson");
    this.#seq = startSeq;
  }

  get lastSeq(): number {
    return this.#seq;
  }

  onAppend(listener: (event: SessionEvent) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  append(type: EventType, payload?: unknown): SessionEvent {
    const event: SessionEvent = {
      seq: ++this.#seq,
      ts: new Date().toISOString(),
      type,
      ...(payload === undefined ? {} : { payload }),
    };

    this.#recent.push(event);
    if (this.#recent.length > EventLog.WINDOW) this.#recent.shift();

    // Fire listeners (waiters, websocket fan-out) synchronously so a wait that
    // is already parked resolves in the same tick the event happens.
    for (const listener of this.#listeners) {
      try {
        listener(event);
      } catch (error) {
        log.error("event listener threw", error);
      }
    }

    // Persistence is best-effort and must not block the live path.
    void appendLine(this.#file, JSON.stringify(event)).catch((error: unknown) =>
      log.error("event log append failed", { file: this.#file, error: String(error) }),
    );

    return event;
  }

  /** Events with `seq > sinceSeq`, newest last, optionally filtered by type. */
  since(sinceSeq: number, types?: ReadonlySet<EventType>, limit = 50): SessionEvent[] {
    const matches = this.#recent.filter((e) => e.seq > sinceSeq && (!types || types.has(e.type)));
    return matches.length > limit ? matches.slice(matches.length - limit) : matches;
  }

  /** Full history from disk — used when a caller's cursor predates the window. */
  async readAll(): Promise<SessionEvent[]> {
    const lines = await readLines(this.#file);
    const events: SessionEvent[] = [];
    for (const line of lines) {
      try {
        events.push(JSON.parse(line) as SessionEvent);
      } catch {
        log.warn("skipping malformed event log line", { file: this.#file });
      }
    }
    return events;
  }

  /**
   * True when `sinceSeq` is old enough that the in-memory window can no longer
   * answer it and the caller should fall back to readAll().
   */
  needsDisk(sinceSeq: number): boolean {
    const oldest = this.#recent[0]?.seq;
    if (oldest === undefined) return sinceSeq < this.#seq;
    return sinceSeq < oldest - 1;
  }
}
