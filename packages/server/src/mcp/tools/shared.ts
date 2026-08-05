import { z } from "zod";
import type { CallToolResult } from "@modelcontextprotocol/sdk/types.js";
import { summarizeEvent, type SessionEvent, type SessionOutput } from "@agentui/shared";
import type { Session } from "../../core/session.js";
import type { SessionManager, SessionSummary } from "../../core/session-manager.js";
import type { WaitResult } from "../../core/waiters.js";

/**
 * Result-shaping shared by every ui_* tool.
 *
 * Tool descriptions carry the whole contract on purpose. The skill teaches the
 * workflow in depth, but it is not always loaded — in Cursor, Codex or a bare
 * MCP client these strings are the only documentation an agent ever sees, so
 * each one has to stand alone.
 */

export function text(body: string): CallToolResult["content"] {
  return [{ type: "text", text: body }];
}

export function fail(message: string): CallToolResult {
  return { isError: true, content: text(message) };
}

export function eventLines(events: SessionEvent[]): string {
  return events.length ? events.map(summarizeEvent).join("\n") : "(no new events)";
}

/** Every state-shaped result carries the next cursor, so a caller cannot lose its place. */
export function stateSummary(
  sessions: SessionManager,
  session: Session,
  url: string,
  connected: number,
): Record<string, unknown> {
  const { stale, idleLabel } = sessions.summarize(session);
  return {
    sessionId: session.id,
    title: session.title,
    status: session.status,
    stale,
    idleLabel,
    url,
    values: session.values,
    revision: session.revision,
    lastEventSeq: session.events.lastSeq,
    connectedClients: connected,
  };
}

/**
 * The list, grouped the same way the dashboard groups it.
 *
 * A flat list of ids and statuses is what made leftover sessions look like open
 * questions: "active" is technically true of a widget from three days ago, and
 * an agent reading it has no way to tell. The headings carry that distinction,
 * because the text is what the model actually reads.
 */
export function renderSessionList(rows: SessionSummary[]): string {
  if (!rows.length) return "No widgets yet. Create one with ui_create, or ask a single question with ui_ask.";

  const waiting = rows.filter((row) => row.status === "active" && !row.stale);
  const stale = rows.filter((row) => row.status === "active" && row.stale);
  const finished = rows.filter((row) => row.status !== "active");

  const line = (row: SessionSummary) => `  ${row.sessionId}  ${row.title} (idle ${row.idleLabel})`;
  const groups: string[][] = [];

  if (waiting.length) groups.push([`${waiting.length} open, waiting for the user:`, ...waiting.map(line)]);
  if (stale.length) {
    groups.push([
      `${stale.length} left over from an earlier run — idle far longer than anyone would wait, so do`,
      "NOT read these as open questions. Close the ones you recognise with ui_close; the user can",
      "clear the rest from the dashboard.",
      ...stale.map(line),
    ]);
  }
  if (finished.length) {
    groups.push([
      `${finished.length} finished:`,
      ...finished.map((row) => `  ${row.sessionId}  ${row.status.padEnd(9)} ${row.title}`),
    ]);
  }
  return groups.map((group) => group.join("\n")).join("\n\n");
}

/**
 * Tells the agent what happened to the browser, so it can say something true to
 * the user rather than always claiming a tab was opened.
 */
export function openNote(reason: "opened" | "already-open" | "suppressed" | "failed"): string {
  switch (reason) {
    case "opened":
      return " (opened in the browser)";
    case "already-open":
      return " (added to the AgentUI tab you already have open — no new tab)";
    case "failed":
      return " (could not open a browser; open that URL yourself)";
    case "suppressed":
      return "";
  }
}

export function describeAnswer(output: SessionOutput, question: string): string {
  const lines = [`Answer to "${question}":`];
  if (output.choice) lines.push(`  chose: ${output.choice}`);
  if (output.touched.length) {
    lines.push(
      `  set deliberately: ${output.touched.map((id) => `${id}=${JSON.stringify(output.values[id])}`).join(", ")}`,
    );
  }
  if (output.untouched.length) {
    // Saying this plainly stops an agent treating an authored default as a decision.
    lines.push(`  left as suggested (NOT chosen by the user): ${output.untouched.join(", ")}`);
  }
  for (const comment of output.comments) if (comment.text) lines.push(`  note: ${comment.text}`);
  return lines.join("\n");
}

/**
 * The first line of a wake-up, which is the part a model actually acts on.
 *
 * "Session closed" is true of a dismissal but says nothing about it, and the
 * difference matters: a widget the agent closed itself is bookkeeping, while a
 * widget the user dismissed is a decision — re-asking is the wrong response to
 * exactly one of them.
 */
export function headline(result: WaitResult): string {
  if (result.reason !== "closed") return "The user responded";
  const dismissed = result.events.some(
    (event) => event.type === "closed" && (event.payload as { reason?: string } | undefined)?.reason === "dismissed",
  );
  return dismissed ? "The user dismissed this without answering" : "Session closed";
}

export const SESSION_ID = z.string().describe("The sessionId returned by ui_create.");

export const SINCE_SEQ = z
  .number()
  .int()
  .nonnegative()
  .optional()
  .describe("Cursor: only events with seq greater than this are returned. Pass the lastEventSeq you last received.");
