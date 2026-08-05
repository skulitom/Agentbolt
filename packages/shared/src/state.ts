import { z } from "zod";
import { InterfaceSpec, type Values } from "./spec.js";

export const SESSION_STATUSES = ["active", "finalized", "closed"] as const;
export type SessionStatus = (typeof SESSION_STATUSES)[number];

/**
 * state.json — the authoritative live state of a session, flushed atomically.
 * Deliberately plain JSON so an agent that cannot reach the tools can still
 * just Read it off disk.
 */
export const SessionState = z.object({
  schemaVersion: z.literal(1),
  sessionId: z.string(),
  title: z.string(),
  status: z.enum(SESSION_STATUSES),
  values: z.record(z.string(), z.unknown()),
  /** Control ids the human has actually interacted with (see SessionOutput). */
  touched: z.array(z.string()).default([]),
  /** Bumped on every ui_update, so the browser knows to re-render. */
  revision: z.number().int().nonnegative(),
  lastEventSeq: z.number().int().nonnegative(),
  createdAt: z.string(),
  updatedAt: z.string(),
  finalizedAt: z.string().optional(),
  /** Which action button ended the session, when one was used. */
  choice: z.string().optional(),
  /** Set when this session was instantiated from a saved widget. */
  template: z.string().optional(),
});

export type SessionState = z.infer<typeof SessionState>;

export const SessionComment = z.object({
  ts: z.string(),
  text: z.string(),
});
export type SessionComment = z.infer<typeof SessionComment>;

/**
 * output.json — the deliverable, written once on finalize and never rewritten.
 * Finalize is idempotent: a second finalize returns this rather than replacing it.
 */
export const SessionOutput = z.object({
  schemaVersion: z.literal(1),
  sessionId: z.string(),
  title: z.string(),
  finalizedAt: z.string(),
  status: z.enum(["answered", "cancelled"]),
  /** Which action button was pressed, if the widget declared any. */
  choice: z.string().optional(),
  values: z.record(z.string(), z.unknown()),
  /**
   * Provenance, and the difference between a decision and a leftover.
   *
   * `touched` lists controls the human actually engaged with; `untouched` lists
   * the rest, whose values are still just the authored defaults. An agent that
   * ignores this will confidently act on settings nobody chose — treat an
   * untouched value as "no opinion expressed", not as an answer.
   */
  touched: z.array(z.string()).default([]),
  untouched: z.array(z.string()).default([]),
  comments: z.array(SessionComment),
  eventCount: z.number().int().nonnegative(),
});

export type SessionOutput = z.infer<typeof SessionOutput>;

export interface SessionSnapshot {
  state: SessionState;
  spec: InterfaceSpec;
  values: Values;
}

export function nowIso(): string {
  return new Date().toISOString();
}
