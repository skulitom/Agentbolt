import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { WakeGroup } from "@agentui/shared";
import type { AgentUiContext } from "../context.js";
import { SESSION_ID, SINCE_SEQ, describeAnswer, eventLines, headline, text } from "./shared.js";
import { waitForSession } from "./session-ops.js";

export function registerWait(server: McpServer, ctx: AgentUiContext): void {
  server.registerTool(
    "ui_wait",
    {
      title: "Wait for the user to respond",
      description: [
        "Blocks until the user commits a change, comments, presses a button, or finishes — then",
        "returns everything that happened since your cursor.",
        "",
        "Call this when you have run out of other work, not as a way of pausing. Omit sessionId to",
        "wake on ANY open widget, which is what you want when several questions are outstanding.",
        "",
        "A 'timeout' reason is NORMAL and not an error — it means the user has not acted yet. Either",
        "go do something else, or call again with the same sinceSeq; nothing is lost either way,",
        "because the cursor makes the loop lossless.",
        "",
        "Slider drags do NOT wake you. Only committed changes, comments, button presses, preview",
        "errors and finishing do.",
        "",
        "A 'closed' reason with an event saying the user dismissed it is an ANSWER: they saw the",
        "question and chose not to answer. Take the decision yourself and say which way you went —",
        "do not re-ask it. If that event carries a reason they typed, act on the reason: it is the",
        "thing you did not think to ask about.",
      ].join("\n"),
      inputSchema: {
        sessionId: SESSION_ID.optional().describe("Omit to wake on any open widget."),
        sinceSeq: SINCE_SEQ,
        timeoutMs: z.number().int().positive().optional().describe("Default 240000 (4 min)."),
        wakeOn: z
          .array(z.enum(["values", "comment", "error", "finalize"]))
          .optional()
          .describe('Defaults to all of ["values","comment","error","finalize"].'),
      },
      outputSchema: {
        reason: z.string(),
        sessionId: z.string().optional(),
        events: z.array(z.record(z.string(), z.unknown())),
        lastEventSeq: z.number(),
        status: z.string().optional(),
        values: z.record(z.string(), z.unknown()).optional(),
        output: z.record(z.string(), z.unknown()).optional(),
      },
      annotations: { title: "Wait for the user to respond", readOnlyHint: true, openWorldHint: false },
    },
    async (args, extra) => {
      const wakeOn = (args.wakeOn ?? ["values", "comment", "error", "finalize"]) as WakeGroup[];

      const { result, effectiveTimeoutMs, session, output } = await waitForSession(ctx, extra, {
        sessionId: args.sessionId,
        sinceSeq: args.sinceSeq ?? 0,
        wakeOn,
        timeoutMs: args.timeoutMs ?? 240_000,
        label: "waiting for the user",
      });

      const structured: Record<string, unknown> = {
        reason: result.reason,
        events: result.events,
        lastEventSeq: result.lastEventSeq,
      };
      if (result.sessionId) structured["sessionId"] = result.sessionId;
      if (session) {
        structured["status"] = session.status;
        structured["values"] = session.values;
      }
      if (output) structured["output"] = output;

      const body =
        result.reason === "timeout"
          ? [
              `No response yet (waited ${Math.round(effectiveTimeoutMs / 1000)}s). This is normal, not an error.`,
              `Do something else and check back, or call ui_wait again with sinceSeq: ${result.lastEventSeq}.`,
            ].join("\n")
          : [
              `${headline(result)}${result.sessionId ? ` (${result.sessionId})` : ""}:`,
              eventLines(result.events),
              output ? `\nfinal answer: ${describeAnswer(output, session?.title ?? "")}` : "",
              `\ncursor: sinceSeq ${result.lastEventSeq}`,
            ]
              .filter(Boolean)
              .join("\n");

      return { content: text(body), structuredContent: structured };
    },
  );
}
