import { log } from "../log.js";

/** The subset of the SDK's handler `extra` this needs, kept narrow for testing. */
export interface ProgressCapableExtra {
  _meta?: { progressToken?: string | number } | undefined;
  sendNotification?: (notification: {
    method: "notifications/progress";
    params: { progressToken: string | number; progress: number; total?: number; message?: string };
  }) => Promise<void>;
  signal?: AbortSignal;
}

/** How long to wait when the client gives us no way to keep the call alive. */
export const UNSUPPORTED_CLIENT_CLAMP_MS = 50_000;
const HEARTBEAT_MS = 10_000;

export interface HeldCall<T> {
  result: T;
  /** The timeout actually used, which may be less than requested. */
  effectiveTimeoutMs: number;
  heartbeats: number;
}

/**
 * Runs a long call while keeping the MCP client from giving up on it.
 *
 * Progress notifications reset a client's *idle* timer. They do not extend a
 * hard wall-clock limit, and a server cannot read or influence the client's
 * timeout settings — so this is a best-effort keepalive, not a guarantee.
 *
 * When the request carries no progressToken there is no keepalive available at
 * all, so the wait is clamped well under the common 60s default and returns a
 * normal timeout result. That is not a failure: the caller re-issues the wait
 * with the same cursor and misses nothing.
 */
export async function withHeartbeat<T>(
  extra: ProgressCapableExtra,
  requestedTimeoutMs: number,
  run: (effectiveTimeoutMs: number) => Promise<T>,
  message?: string,
): Promise<HeldCall<T>> {
  const token = extra._meta?.progressToken;
  const canHeartbeat = token !== undefined && typeof extra.sendNotification === "function";
  const effectiveTimeoutMs = canHeartbeat ? requestedTimeoutMs : Math.min(requestedTimeoutMs, UNSUPPORTED_CLIENT_CLAMP_MS);

  let heartbeats = 0;
  let timer: NodeJS.Timeout | undefined;

  if (canHeartbeat) {
    timer = setInterval(() => {
      heartbeats += 1;
      void extra
        .sendNotification?.({
          method: "notifications/progress",
          params: {
            progressToken: token,
            // Monotonic by contract; nothing enforces it, and a client that
            // does check will drop notifications that go backwards.
            progress: heartbeats,
            ...(message === undefined ? {} : { message }),
          },
        })
        .catch((error: unknown) => log.debug("progress notification failed", String(error)));
    }, HEARTBEAT_MS);
    timer.unref?.();
  }

  try {
    const result = await run(effectiveTimeoutMs);
    return { result, effectiveTimeoutMs, heartbeats };
  } finally {
    if (timer) clearInterval(timer);
  }
}
