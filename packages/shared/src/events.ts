import { z } from "zod";

/**
 * The append-only event log is the single source of truth for "what happened",
 * and `sinceSeq` cursors over it are how an agent catches up losslessly.
 *
 * The critical distinction in this file is between `values_interim` and
 * `values_committed`. Interim events are drag noise: they keep ui_get_state
 * fresh and they are recorded, but they must NEVER wake a waiter. Committed
 * events fire on pointer release / blur / an explicit send, and those are the
 * ones an agent is woken for. Blur that line and a single slider drag spams the
 * agent with hundreds of wakeups.
 */
export const EVENT_TYPES = [
  "session_created",
  "spec_updated",
  "values_interim",
  "values_committed",
  "comment",
  "action",
  "preview_event",
  "preview_error",
  "client_connected",
  "client_disconnected",
  "finalized",
  "closed",
] as const;

export type EventType = (typeof EVENT_TYPES)[number];

/** Event types that may wake a ui_wait. Deliberately excludes values_interim. */
export const WAKEABLE_EVENTS = [
  "values_committed",
  "comment",
  "action",
  "preview_error",
  "finalized",
  "closed",
] as const satisfies readonly EventType[];

export type WakeableEvent = (typeof WAKEABLE_EVENTS)[number];

/** Coarse categories an agent can subscribe to via ui_wait({ wakeOn }). */
export const WAKE_GROUPS = {
  values: ["values_committed"],
  comment: ["comment", "action"],
  error: ["preview_error"],
  finalize: ["finalized", "closed"],
} as const satisfies Record<string, readonly EventType[]>;

export type WakeGroup = keyof typeof WAKE_GROUPS;
export const WAKE_GROUP_NAMES = Object.keys(WAKE_GROUPS) as WakeGroup[];

export function wakeTypesFor(groups: readonly WakeGroup[]): Set<EventType> {
  const out = new Set<EventType>();
  for (const group of groups) for (const type of WAKE_GROUPS[group]) out.add(type);
  return out;
}

export const SessionEvent = z.object({
  seq: z.number().int().positive(),
  ts: z.string(),
  type: z.enum(EVENT_TYPES),
  payload: z.unknown().optional(),
});

export type SessionEvent = z.infer<typeof SessionEvent>;

/** Compact rendering used in tool results, where context is expensive. */
export function summarizeEvent(event: SessionEvent): string {
  const payload = (event.payload ?? {}) as Record<string, unknown>;
  switch (event.type) {
    case "values_committed":
      return `#${event.seq} values changed: ${compactValues(payload["values"])}`;
    case "comment":
      return `#${event.seq} comment: ${JSON.stringify(payload["text"] ?? "")}`;
    case "action":
      return `#${event.seq} pressed "${String(payload["actionId"] ?? "?")}"`;
    case "preview_error":
      return `#${event.seq} preview error: ${String(payload["message"] ?? "")}`;
    case "preview_event":
      return `#${event.seq} preview event ${String(payload["name"] ?? "")}: ${JSON.stringify(payload["data"] ?? null)}`;
    case "finalized":
      return `#${event.seq} finalized`;
    // A close the user performed themselves is not the same news as one the
    // agent asked for: "dismissed" means the question was seen and declined, and
    // re-asking it is the wrong move. The reason is only ever set for a session
    // that was still open, so this never overstates what happened.
    case "closed": {
      // The note is the whole reason the box is offered, so it is never
      // summarised away — a dismissal that says why is more useful than most
      // answers.
      const note = typeof payload["note"] === "string" && payload["note"] ? `: ${JSON.stringify(payload["note"])}` : "";
      return payload["reason"] === "dismissed"
        ? `#${event.seq} dismissed by the user${note}`
        : `#${event.seq} session closed${note}`;
    }
    case "spec_updated":
      return `#${event.seq} widget updated (revision ${String(payload["revision"] ?? "?")})`;
    case "client_connected":
      return `#${event.seq} browser connected`;
    case "client_disconnected":
      return `#${event.seq} browser disconnected`;
    case "session_created":
      return `#${event.seq} session created`;
    case "values_interim":
      return `#${event.seq} (dragging) ${compactValues(payload["values"])}`;
  }
}

function compactValues(values: unknown): string {
  if (typeof values !== "object" || values === null) return "{}";
  const entries = Object.entries(values as Record<string, unknown>).map(([k, v]) => {
    if (Array.isArray(v)) return `${k}=[${v.map((n) => round(n)).join(", ")}]`;
    if (typeof v === "number") return `${k}=${round(v)}`;
    if (typeof v === "string") return `${k}=${JSON.stringify(v)}`;
    return `${k}=${String(v)}`;
  });
  return entries.join(" ");
}

function round(value: unknown): string {
  if (typeof value !== "number") return String(value);
  return Number.isInteger(value) ? String(value) : String(Math.round(value * 1000) / 1000);
}
