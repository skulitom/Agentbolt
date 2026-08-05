import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { AgentUiContext } from "../context.js";
import { SESSION_ID, SINCE_SEQ, eventLines, fail, renderSessionList, stateSummary, text } from "./shared.js";

export function registerGetState(server: McpServer, ctx: AgentUiContext): void {
  server.registerTool(
    "ui_get_state",
    {
      title: "Read widget state",
      description: [
        "Non-blocking snapshot of a widget: current values, status, and any events since your cursor.",
        "Omit sessionId to list every session. Once a session is finalized the result also carries the",
        "final `output`, so this is all you need to collect an answer.",
        "",
        "The list separates widgets actually waiting for the user from ones left over from an earlier",
        "run (`stale: true` — still open, but idle for hours). Never treat a stale one as a question",
        "you are owed an answer to.",
        "",
        "To *wait* for the user rather than poll, use ui_wait — it blocks efficiently instead of",
        "burning turns.",
      ].join("\n"),
      inputSchema: {
        sessionId: SESSION_ID.optional().describe("Omit to list all sessions."),
        sinceSeq: SINCE_SEQ,
      },
      outputSchema: {
        sessions: z.array(z.record(z.string(), z.unknown())).optional(),
        session: z.record(z.string(), z.unknown()).optional(),
        events: z.array(z.record(z.string(), z.unknown())).optional(),
        output: z.record(z.string(), z.unknown()).optional(),
      },
      annotations: { title: "Read widget state", readOnlyHint: true, openWorldHint: false },
    },
    async (args) => {
      // No id: list mode. Same summaries the dashboard renders — see SessionSummary.
      if (!args.sessionId) {
        const summaries = ctx.sessions.summaries();
        const origin = ctx.origin;
        const sessions = summaries.map((summary) => ({
          ...summary,
          url: origin ? `${origin}/s/${summary.sessionId}` : undefined,
          connectedClients: ctx.connectedClients(summary.sessionId),
        }));
        return {
          content: text(renderSessionList(summaries)),
          structuredContent: { sessions },
        };
      }

      const session = ctx.sessions.get(args.sessionId);
      if (!session) return fail(`No session "${args.sessionId}". Call ui_get_state with no arguments to list them.`);

      const sinceSeq = args.sinceSeq ?? 0;
      const events = session.events.needsDisk(sinceSeq)
        ? (await session.events.readAll()).filter((e) => e.seq > sinceSeq).slice(-50)
        : session.events.since(sinceSeq);

      const url = await ctx.sessionUrl(session.id);
      const output = await session.output();
      const { stale, idleLabel } = ctx.sessions.summarize(session);

      const structured: Record<string, unknown> = {
        session: stateSummary(ctx.sessions, session, url, ctx.connectedClients(session.id)),
        events,
      };
      if (output) structured["output"] = output;

      return {
        content: text(
          [
            `${session.title} — ${session.status} (${ctx.connectedClients(session.id)} tab(s) open)`,
            stale
              ? `NOTE: untouched for ${idleLabel}, so it is almost certainly left over from an earlier run rather than a question anyone is about to answer.`
              : "",
            `values: ${JSON.stringify(session.values)}`,
            `cursor: lastEventSeq ${session.events.lastSeq}`,
            "",
            eventLines(events),
            output ? `\nfinal output: ${JSON.stringify(output.values)}` : "",
          ]
            .filter(Boolean)
            .join("\n"),
        ),
        structuredContent: structured,
      };
    },
  );
}
