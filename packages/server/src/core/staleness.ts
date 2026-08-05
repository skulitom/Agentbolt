import { log } from "../log.js";

/**
 * When a restored session stops counting as "waiting for you".
 *
 * Hydration deliberately brings back every non-closed session: an agent whose
 * MCP server restarted mid-question must find its widget exactly where it left
 * it, values and cursor intact. The cost is that a session nobody will ever
 * answer — yesterday's demo, last week's experiment — comes back looking
 * identical to one that is genuinely live, so the dashboard badge and
 * ui_get_state both insist someone is waiting on it. That is the thing being
 * fixed here.
 *
 * Staleness is the distinction, and it is DERIVED, never persisted:
 *
 *  - It is a pure function of idle time, so there is no extra lifecycle state to
 *    keep in step, and it corrects itself the instant anyone touches the session
 *    again — no un-staling step to forget.
 *  - It changes presentation only. A stale session is still open, still reachable
 *    at its URL, and answering it still writes output.json exactly as before.
 *
 * That asymmetry is what lets the threshold be fairly tight. Mislabelling a live
 * session costs it a heading; auto-closing one would destroy the answer somebody
 * was about to give. Anything irreversible here belongs to the user (the
 * dashboard's clear-all) or to the agent (ui_close), not to boot-time policy.
 *
 * Measured from `updatedAt`, not `createdAt`: a widget somebody has been
 * adjusting all afternoon is not abandoned just because it is old.
 */

/** Long enough to outlast lunch and a long meeting; short enough that this morning's demo is stale by the afternoon. */
export const DEFAULT_STALE_AFTER_HOURS = 4;

/** Hours of idleness before a session is presented as stale. `0` disables staleness entirely. */
export const STALE_AFTER_ENV = "AGENT_UI_STALE_AFTER_HOURS";

const HOUR_MS = 3_600_000;

let warnedAboutEnv = false;

export function staleAfterMs(): number {
  const raw = process.env[STALE_AFTER_ENV];
  if (raw === undefined || raw.trim() === "") return DEFAULT_STALE_AFTER_HOURS * HOUR_MS;

  const hours = Number(raw);
  if (!Number.isFinite(hours) || hours < 0) {
    if (!warnedAboutEnv) {
      warnedAboutEnv = true;
      log.warn(`ignoring ${STALE_AFTER_ENV}: expected a non-negative number of hours`, { value: raw });
    }
    return DEFAULT_STALE_AFTER_HOURS * HOUR_MS;
  }
  // Opt-out: never demote anything, however old.
  return hours === 0 ? Number.POSITIVE_INFINITY : hours * HOUR_MS;
}

export interface Staleness {
  /** Milliseconds since the session last changed, or undefined if its timestamp is unreadable. */
  idleMs: number | undefined;
  /** Compact duration for display — the server formats it so every surface shows the same words. */
  idleLabel: string;
  stale: boolean;
}

export function stalenessOf(updatedAt: string, now = Date.now(), thresholdMs = staleAfterMs()): Staleness {
  const at = Date.parse(updatedAt);
  // A hand-edited or truncated state.json tells us nothing about whether anyone
  // is waiting. "Stale" is a claim, so with no evidence we decline to make it.
  const idleMs = Number.isFinite(at) ? Math.max(0, now - at) : undefined;
  return { idleMs, idleLabel: formatIdle(idleMs), stale: idleMs !== undefined && idleMs > thresholdMs };
}

/** "42s" / "17m" / "6h" / "3d" — compact enough to sit in a list row. */
export function formatIdle(idleMs: number | undefined): string {
  if (idleMs === undefined) return "unknown";
  const seconds = Math.floor(idleMs / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}
