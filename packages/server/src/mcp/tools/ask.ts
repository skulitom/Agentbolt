import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { AskInput, compileAskChecked, formatSpecError } from "@agentui/shared";
import type { AgentUiContext } from "../context.js";
import { describeAnswer, fail, openNote, text } from "./shared.js";
import { openSession, waitForSession } from "./session-ops.js";
import { ASK_CONTENT, ASK_FIELDS, ASK_OPTIONS, ASK_PREVIEW } from "./schemas.js";

export function registerAsk(server: McpServer, ctx: AgentUiContext): void {
  server.registerTool(
    "ui_ask",
    {
      title: "Ask the user a question",
      description: [
        "Puts a real question in front of the user — buttons to choose from, fields to fill in, and",
        "optionally a diff/image/markdown to look at — and RETURNS IMMEDIATELY.",
        "",
        "It does not block by default, and you should almost never make it. Carry on with whatever",
        "else you can do, then pick up the answer with ui_wait or ui_get_state at your next natural",
        "checkpoint. Blocking on a human wastes the time they take to think; asking several questions",
        "up front and collecting them later is the intended shape.",
        "",
        "Only pass wait:true when you genuinely cannot do anything else until you know the answer",
        "(for example, the answer decides which file you edit next).",
        "",
        'Shorthand is fine: options can be plain strings ["Ship it","Hold"], and fields can omit',
        "labels and defaults — { id: 'retries', default: 3 } infers a number input.",
        "",
        "ui_schema({ of: 'control', type }) has the full shape of any control type.",
      ].join("\n"),
      inputSchema: {
        question: z.string().describe("The question, in plain language."),
        details: z.string().optional().describe("Extra context shown under the question."),
        options: ASK_OPTIONS,
        fields: ASK_FIELDS,
        content: ASK_CONTENT,
        preview: ASK_PREVIEW,
        title: z.string().optional(),
        wait: z
          .boolean()
          .optional()
          .describe("Block until answered. Defaults to FALSE — prefer returning now and collecting later."),
        timeoutMs: z.number().int().positive().optional().describe("Only used with wait:true. Default 600000 (10 min)."),
        open: z.boolean().optional().describe("Open the user's browser. Defaults to true."),
      },
      outputSchema: {
        sessionId: z.string(),
        url: z.string(),
        status: z.string(),
        answered: z.boolean(),
        choice: z.string().optional(),
        values: z.record(z.string(), z.unknown()).optional(),
        comment: z.string().optional(),
        lastEventSeq: z.number(),
      },
      annotations: { title: "Ask the user a question", openWorldHint: false },
    },
    async (args, extra) => {
      const parsedInput = AskInput.safeParse({
        question: args.question,
        details: args.details,
        options: args.options,
        fields: args.fields,
        content: args.content,
        preview: args.preview,
        title: args.title,
        allowComment: true,
      });
      if (!parsedInput.success) return fail(formatSpecError(parsedInput.error));

      const compiled = compileAskChecked(parsedInput.data);
      if (!compiled.ok) return fail(compiled.message);

      const { session, url, opened } = await openSession(ctx, { spec: compiled.spec, open: args.open });

      if (args.wait !== true) {
        return {
          content: text(
            [
              `Asked: "${args.question}"`,
              `${url}${openNote(opened.reason)}`,
              "",
              "Returned without waiting. Keep working; collect the answer later with",
              `ui_wait({ sessionId: "${session.id}" }) or ui_get_state({ sessionId: "${session.id}" }).`,
            ].join("\n"),
          ),
          structuredContent: {
            sessionId: session.id,
            url,
            status: "asked",
            answered: false,
            lastEventSeq: session.events.lastSeq,
          },
        };
      }

      const { result, output } = await waitForSession(ctx, extra, {
        sessionId: session.id,
        sinceSeq: 0,
        wakeOn: ["finalize"],
        timeoutMs: args.timeoutMs ?? 600_000,
        label: `waiting for an answer to: ${args.question}`,
      });

      if (!output) {
        return {
          content: text(
            [
              `No answer yet (${result.reason}). The question is still open at ${url}.`,
              `Carry on and check back with ui_wait({ sessionId: "${session.id}", sinceSeq: ${result.lastEventSeq} }).`,
            ].join("\n"),
          ),
          structuredContent: {
            sessionId: session.id,
            url,
            status: result.reason,
            answered: false,
            lastEventSeq: result.lastEventSeq,
          },
        };
      }

      return {
        content: text(describeAnswer(output, args.question)),
        structuredContent: {
          sessionId: session.id,
          url,
          status: "answered",
          answered: true,
          ...(output.choice === undefined ? {} : { choice: output.choice }),
          values: output.values,
          ...(output.comments.length ? { comment: output.comments.map((c) => c.text).join("\n") } : {}),
          lastEventSeq: session.events.lastSeq,
        },
      };
    },
  );
}
