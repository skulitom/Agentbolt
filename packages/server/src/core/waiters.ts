import {
  wakeTypesFor,
  type EventType,
  type SessionEvent,
  type WakeGroup,
} from "@agentui/shared";
import type { SessionManager } from "./session-manager.js";
import { log } from "../log.js";

export interface WaitRequest {
  /** Omit to wake on any active session — useful when several are outstanding. */
  sessionId?: string | undefined;
  sinceSeq: number;
  wakeOn: readonly WakeGroup[];
  timeoutMs: number;
}

export interface WaitResult {
  reason: "event" | "timeout" | "closed";
  sessionId?: string;
  events: SessionEvent[];
  lastEventSeq: number;
}

interface Parked {
  sessionId: string | undefined;
  sinceSeq: number;
  types: Set<EventType>;
  resolve(result: WaitResult): void;
  timer: NodeJS.Timeout;
}

/**
 * Parked ui_wait calls.
 *
 * Two rules do the real work here:
 *
 *  - `values_interim` is not in any wake group, so a slider drag never wakes an
 *    agent. Break that and one drag produces hundreds of wakeups and the whole
 *    async model collapses.
 *  - A woken waiter receives EVERY qualifying event since its cursor, not just
 *    the one that happened to fire. Otherwise a burst delivers the last event
 *    and silently drops the rest.
 */
export class Waiters {
  readonly #parked = new Set<Parked>();

  constructor(private readonly sessions: SessionManager) {}

  get parkedCount(): number {
    return this.#parked.size;
  }

  /** Called for every appended event, from the session's own log. */
  notify(sessionId: string, event: SessionEvent): void {
    for (const waiter of [...this.#parked]) {
      if (waiter.sessionId !== undefined && waiter.sessionId !== sessionId) continue;
      if (!waiter.types.has(event.type)) continue;
      this.#release(waiter, sessionId, event.type === "closed" ? "closed" : "event");
    }
  }

  #release(waiter: Parked, sessionId: string, reason: WaitResult["reason"]): void {
    if (!this.#parked.delete(waiter)) return;
    clearTimeout(waiter.timer);

    const session = this.sessions.get(sessionId);
    const events = session
      ? session.events.since(waiter.sinceSeq, waiter.types, 50)
      : [];

    waiter.resolve({
      reason,
      sessionId,
      events,
      lastEventSeq: session?.events.lastSeq ?? waiter.sinceSeq,
    });
  }

  async wait(request: WaitRequest): Promise<WaitResult> {
    const types = wakeTypesFor(request.wakeOn);

    // Never park when the answer is already sitting there — a cursor that has
    // fallen behind must return immediately, or an agent that was busy while the
    // human answered would wait out the full timeout for events it already has.
    const pending = this.#collectPending(request, types);
    if (pending) return pending;

    return new Promise<WaitResult>((resolve) => {
      const waiter: Parked = {
        sessionId: request.sessionId,
        sinceSeq: request.sinceSeq,
        types,
        resolve,
        timer: setTimeout(() => {
          this.#parked.delete(waiter);
          resolve({
            reason: "timeout",
            ...(request.sessionId === undefined ? {} : { sessionId: request.sessionId }),
            events: [],
            lastEventSeq: request.sessionId
              ? (this.sessions.get(request.sessionId)?.events.lastSeq ?? request.sinceSeq)
              : request.sinceSeq,
          });
        }, request.timeoutMs),
      };
      waiter.timer.unref?.();
      this.#parked.add(waiter);
    });
  }

  #collectPending(request: WaitRequest, types: Set<EventType>): WaitResult | undefined {
    const candidates = request.sessionId
      ? [this.sessions.get(request.sessionId)].filter((s) => s !== undefined)
      : this.sessions.list();

    for (const session of candidates) {
      const events = session.events.since(request.sinceSeq, types, 50);
      if (events.length) {
        return {
          reason: events.some((e) => e.type === "closed") ? "closed" : "event",
          sessionId: session.id,
          events,
          lastEventSeq: session.events.lastSeq,
        };
      }
    }
    return undefined;
  }

  /** Releases everything waiting on a session that is going away. */
  releaseForClosedSession(sessionId: string): void {
    for (const waiter of [...this.#parked]) {
      if (waiter.sessionId !== undefined && waiter.sessionId !== sessionId) continue;
      this.#release(waiter, sessionId, "closed");
    }
  }

  disposeAll(): void {
    for (const waiter of [...this.#parked]) {
      clearTimeout(waiter.timer);
      waiter.resolve({ reason: "closed", events: [], lastEventSeq: waiter.sinceSeq });
    }
    this.#parked.clear();
    log.debug("released all waiters");
  }
}
