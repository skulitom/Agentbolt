import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { AgentUiContext } from "../context.js";
import { SESSION_ID, fail, text } from "./shared.js";
import { log } from "../../log.js";

export function registerClose(server: McpServer, ctx: AgentUiContext): void {
  server.registerTool(
    "ui_close",
    {
      title: "Close a widget",
      description: [
        "Ends a session. Open tabs switch to a read-only 'session ended' state and any ui_wait waiting",
        "on it returns with reason 'closed'. Close widgets you are done with so the user's dashboard",
        "stays meaningful.",
      ].join("\n"),
      inputSchema: {
        sessionId: SESSION_ID,
        deleteFiles: z
          .boolean()
          .optional()
          .describe("Also delete the session directory, including its output.json. Defaults to false."),
      },
      outputSchema: { sessionId: z.string(), closed: z.boolean() },
      annotations: { title: "Close a widget", destructiveHint: true, openWorldHint: false },
    },
    async (args) => {
      const session = ctx.sessions.get(args.sessionId);
      if (!session) return fail(`No session "${args.sessionId}".`);

      const closed = await ctx.sessions.close(args.sessionId, args.deleteFiles ?? false);
      log.info("session closed", { sessionId: args.sessionId, deleteFiles: args.deleteFiles });

      return {
        content: text(`Closed ${args.sessionId} ("${session.title}").`),
        structuredContent: { sessionId: args.sessionId, closed },
      };
    },
  );
}
