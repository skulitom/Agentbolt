import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { parseSpec, validateValuesAgainstSpec, type Values } from "@agentui/shared";
import type { AgentUiContext } from "../context.js";
import { broadcastToSession } from "../../web/ws.js";
import { SESSION_ID, fail, text } from "./shared.js";
import { CREATE_SPEC } from "./schemas.js";

export function registerUpdate(server: McpServer, ctx: AgentUiContext): void {
  server.registerTool(
    "ui_update",
    {
      title: "Update a live widget",
      description: [
        "Changes a widget the user is already looking at: a new spec, a new preview module, or new",
        "values. Open tabs re-render without losing what the user has already adjusted.",
        "",
        "Use this to fix a preview module whose stack trace you saw in the event log, to narrow a",
        "slider range once you know more, or to push a value you computed.",
      ].join("\n"),
      inputSchema: {
        sessionId: SESSION_ID,
        spec: CREATE_SPEC.describe("Replacement spec. Values for controls that still exist are preserved."),
        previewModule: z.string().optional().describe("Replacement preview module; the iframe reloads it."),
        values: z.record(z.string(), z.unknown()).optional().describe("Values to push into the widget."),
      },
      outputSchema: { sessionId: z.string(), revision: z.number(), values: z.record(z.string(), z.unknown()) },
      annotations: { title: "Update a live widget", openWorldHint: false },
    },
    async (args) => {
      const session = ctx.sessions.get(args.sessionId);
      if (!session) return fail(`No session "${args.sessionId}".`);
      if (!session.isOpen) return fail(`Session "${args.sessionId}" is ${session.status}; it can no longer be updated.`);

      if (args.spec !== undefined || args.previewModule !== undefined) {
        const parsed = args.spec === undefined ? { ok: true as const, spec: session.spec } : parseSpec(args.spec);
        if (!parsed.ok) return fail(parsed.message);
        await session.updateSpec(parsed.spec, args.previewModule);
        broadcastToSession(session.id, {
          type: "spec_updated",
          spec: session.spec,
          values: session.values,
          revision: session.revision,
          hasPreview: session.hasPreview,
        });
      }

      if (args.values) {
        const problems = validateValuesAgainstSpec(session.spec, args.values);
        if (problems.length) {
          return fail(`Rejected values:\n${problems.map((p) => `  ${p.id}: ${p.message}`).join("\n")}`);
        }
        const values = session.pushValues(args.values as Values);
        broadcastToSession(session.id, { type: "values_pushed", values });
      }

      return {
        content: text(`Updated "${session.title}" (revision ${session.revision}).`),
        structuredContent: { sessionId: session.id, revision: session.revision, values: session.values },
      };
    },
  );
}
