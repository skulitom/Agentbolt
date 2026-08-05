import type { RequestHandlerExtra } from "@modelcontextprotocol/sdk/shared/protocol.js";
import type { ServerNotification, ServerRequest } from "@modelcontextprotocol/sdk/types.js";
import type { SessionOutput, Values, WakeGroup } from "@agentui/shared";
import type { AgentUiContext } from "../context.js";
import type { InterfaceSpec } from "@agentui/shared";
import type { Session } from "../../core/session.js";
import type { WaitResult } from "../../core/waiters.js";
import { withHeartbeat } from "../heartbeat.js";

/**
 * The two sequences ui_ask and ui_create/ui_wait each had their own copy of.
 *
 * They were never *slightly* different — they were the same four steps written
 * twice, and the parts that looked different (ui_ask never records a template
 * use) collapse the moment you notice the field is simply undefined on that
 * path. What is deliberately NOT shared is the rendering: ui_ask reports one
 * question's answer and ui_wait reports a cursor and an event list, and those
 * are genuinely different results for genuinely different questions.
 */

export interface OpenedSession {
  session: Session;
  url: string;
  /** What actually happened to the browser, for openNote(). */
  opened: { opened: boolean; reason: "opened" | "already-open" | "suppressed" | "failed" };
}

export async function openSession(
  ctx: AgentUiContext,
  init: {
    spec: InterfaceSpec;
    previewModule?: string | undefined;
    values?: Values | undefined;
    template?: string | undefined;
    /** false suppresses the browser; the URL is still returned. */
    open?: boolean | undefined;
  },
): Promise<OpenedSession> {
  const session = await ctx.sessions.create({
    spec: init.spec,
    previewModule: init.previewModule,
    values: init.values,
    template: init.template,
  });
  ctx.watch(session.id);
  if (init.template) await ctx.library.recordUse(init.template);

  const url = await ctx.sessionUrl(session.id);
  const opened =
    init.open === false ? { opened: false, reason: "suppressed" as const } : await ctx.openInBrowser(url);

  return { session, url, opened };
}

export interface WaitedForSession {
  result: WaitResult;
  effectiveTimeoutMs: number;
  session: Session | undefined;
  output: SessionOutput | undefined;
}

/**
 * Park until something wakes us, then resolve the session and its output.
 *
 * `withHeartbeat` is not optional decoration: a client with no progressToken
 * gets its request cancelled at 60s, so the wait is clamped and reported
 * rather than silently dying.
 */
export async function waitForSession(
  ctx: AgentUiContext,
  extra: RequestHandlerExtra<ServerRequest, ServerNotification>,
  options: {
    sessionId?: string | undefined;
    sinceSeq: number;
    wakeOn: WakeGroup[];
    timeoutMs: number;
    label: string;
  },
): Promise<WaitedForSession> {
  const { result, effectiveTimeoutMs } = await withHeartbeat(
    extra,
    options.timeoutMs,
    (timeoutMs) =>
      ctx.waiters.wait({
        sessionId: options.sessionId,
        sinceSeq: options.sinceSeq,
        wakeOn: options.wakeOn,
        timeoutMs,
      }),
    options.label,
  );

  const session = result.sessionId ? ctx.sessions.get(result.sessionId) : undefined;
  const output = session ? await session.output() : undefined;
  return { result, effectiveTimeoutMs, session, output };
}
