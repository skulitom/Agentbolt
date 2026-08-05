import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { CallToolResult } from "@modelcontextprotocol/sdk/types.js";
import {
  AskInput,
  compileAskChecked,
  formatSpecError,
  parseSpec,
  summarizeEvent,
  validateValuesAgainstSpec,
  type SessionEvent,
  type SessionOutput,
  type Values,
  type WakeGroup,
} from "@agentui/shared";
import type { AgentUiContext } from "./context.js";
import type { Session } from "../core/session.js";
import type { SessionManager, SessionSummary } from "../core/session-manager.js";
import { registerLibraryTools } from "./library-tools.js";
import { broadcastToSession } from "../web/ws.js";
import { withHeartbeat } from "./heartbeat.js";
import { log } from "../log.js";

/**
 * The ui_* tools.
 *
 * Tool descriptions carry the whole contract on purpose. The skill teaches the
 * workflow in depth, but it is not always loaded — in Cursor, Codex or a bare
 * MCP client these strings are the only documentation an agent ever sees, so
 * each one has to stand alone.
 */

/* ------------------------------------------------------------------ shared */

function text(body: string): CallToolResult["content"] {
  return [{ type: "text", text: body }];
}

function fail(message: string): CallToolResult {
  return { isError: true, content: text(message) };
}

function eventLines(events: SessionEvent[]): string {
  return events.length ? events.map(summarizeEvent).join("\n") : "(no new events)";
}

/** Every state-shaped result carries the next cursor, so a caller cannot lose its place. */
function stateSummary(
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
function renderSessionList(rows: SessionSummary[]): string {
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
function openNote(reason: "opened" | "already-open" | "suppressed" | "failed"): string {
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

const SESSION_ID = z.string().describe("The sessionId returned by ui_create.");
const SINCE_SEQ = z
  .number()
  .int()
  .nonnegative()
  .optional()
  .describe("Cursor: only events with seq greater than this are returned. Pass the lastEventSeq you last received.");

/* ------------------------------------------------------------- registration */

export async function registerTools(server: McpServer, ctx: AgentUiContext): Promise<void> {
  // The library shortlist is folded into ui_create's description at startup.
  // Tool descriptions are always in context, which makes them the cheapest place
  // to keep saved widgets discoverable — and the reason the list is hard-capped.
  const index = await ctx.library.indexForToolDescription();

  registerAsk(server, ctx);
  const create = registerCreate(server, ctx, index);
  registerLibraryTools(server, ctx);
  registerUpdate(server, ctx);
  registerWait(server, ctx);
  registerGetState(server, ctx);
  registerClose(server, ctx);

  // Best-effort refresh after a save. Many clients fetch tools/list once and
  // never again, so this is a bonus rather than the mechanism discovery relies on.
  ctx.onLibraryChange(() => {
    void ctx.library
      .indexForToolDescription()
      .then((next) => create.update({ description: createDescription(next) }))
      .catch((error: unknown) => log.debug("could not refresh library index", String(error)));
  });
}

function createDescription(index: string): string {
  return [
    "Opens a small web UI in the user's browser so they can answer you with sliders, pickers,",
    "forms, or a live preview, and RETURNS IMMEDIATELY — the user adjusts it while you keep working.",
    "",
    "Use this for open-ended tuning or anything the user should keep alongside your work. For a",
    "single question, ui_ask is shorter. Neither blocks: collect answers with ui_wait later.",
    "",
    "Check ui_library first — reusing a saved widget keeps the settings the user chose last time.",
    "",
    "Authoring rules: control ids are snake_case and unique; every control needs a `default`;",
    "sliders need `min` and `max`. `content` blocks (markdown/code/diff/image) render above the",
    "controls, so you can show a diff or a screenshot and ask about it. Anything with a magnitude",
    "deserves a `previewModule` — a slider with no visible effect is worse than a text question.",
    "",
    "Invalid specs come back with the exact path of each problem — fix and retry.",
    index ? `\n${index}` : "",
  ]
    .filter(Boolean)
    .join("\n");
}

function registerAsk(server: McpServer, ctx: AgentUiContext): void {
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
      ].join("\n"),
      inputSchema: {
        question: z.string().describe("The question, in plain language."),
        details: z.string().optional().describe("Extra context shown under the question."),
        options: z
          .array(z.unknown())
          .optional()
          .describe('Buttons. Strings, or {label, style?: "default"|"primary"|"danger"}. Pressing one answers.'),
        fields: z.array(z.unknown()).optional().describe("Controls to fill in. Shorthand allowed; ids are snake_case."),
        content: z.unknown().optional().describe("Something to look at: a markdown string, or content blocks."),
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
        title: args.title,
        allowComment: true,
      });
      if (!parsedInput.success) return fail(formatSpecError(parsedInput.error));

      const compiled = compileAskChecked(parsedInput.data);
      if (!compiled.ok) return fail(compiled.message);

      const session = await ctx.sessions.create({ spec: compiled.spec });
      ctx.watch(session.id);
      const url = await ctx.sessionUrl(session.id);
      const opened = args.open === false ? { opened: false, reason: "suppressed" as const } : await ctx.openInBrowser(url);

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

      const { result } = await withHeartbeat(
        extra,
        args.timeoutMs ?? 600_000,
        (timeoutMs) =>
          ctx.waiters.wait({ sessionId: session.id, sinceSeq: 0, wakeOn: ["finalize"], timeoutMs }),
        `waiting for an answer to: ${args.question}`,
      );

      const output = await session.output();
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

function describeAnswer(output: SessionOutput, question: string): string {
  const lines = [`Answer to "${question}":`];
  if (output.choice) lines.push(`  chose: ${output.choice}`);
  if (output.touched.length) {
    lines.push(`  set deliberately: ${output.touched.map((id) => `${id}=${JSON.stringify(output.values[id])}`).join(", ")}`);
  }
  if (output.untouched.length) {
    // Saying this plainly stops an agent treating an authored default as a decision.
    lines.push(`  left as suggested (NOT chosen by the user): ${output.untouched.join(", ")}`);
  }
  for (const comment of output.comments) if (comment.text) lines.push(`  note: ${comment.text}`);
  return lines.join("\n");
}

function registerUpdate(server: McpServer, ctx: AgentUiContext): void {
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
        spec: z.unknown().optional().describe("Replacement spec. Values for controls that still exist are preserved."),
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

function registerWait(server: McpServer, ctx: AgentUiContext): void {
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

      const { result, effectiveTimeoutMs } = await withHeartbeat(
        extra,
        args.timeoutMs ?? 240_000,
        (timeoutMs) =>
          ctx.waiters.wait({
            sessionId: args.sessionId,
            sinceSeq: args.sinceSeq ?? 0,
            wakeOn,
            timeoutMs,
          }),
        "waiting for the user",
      );

      const session = result.sessionId ? ctx.sessions.get(result.sessionId) : undefined;
      const output = session ? await session.output() : undefined;

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
              `${result.reason === "closed" ? "Session closed" : "The user responded"}${result.sessionId ? ` (${result.sessionId})` : ""}:`,
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

function registerCreate(server: McpServer, ctx: AgentUiContext, index: string) {
  return server.registerTool(
    "ui_create",
    {
      title: "Create an interactive widget",
      description: createDescription(index),
      inputSchema: {
        template: z
          .string()
          .optional()
          .describe(
            "Slug of a saved widget to reuse instead of authoring one. It reopens at the values the user last left, which is what makes reuse worth having. Find one with ui_library.",
          ),
        spec: z
          .unknown()
          .optional()
          .describe(
            'The widget spec: { title, content?, controls?, actions?, preview? }. Example: {"title":"Muzzle flash","controls":[{"id":"intensity","label":"Intensity","type":"slider","min":0,"max":1,"default":0.7}]}',
          ),
        previewModule: z
          .string()
          .optional()
          .describe(
            'Optional ES module for a live preview: export default { setup({container, spec, values, emit}), update(values), dispose?() }. Only "three" may be imported.',
          ),
        values: z.record(z.string(), z.unknown()).optional().describe("Starting values, overriding the spec defaults."),
        // NOT .default(): zod 4's toJSONSchema uses io:"output", under which a
        // field with a default is emitted as *required* — which would force
        // every caller to pass it. Optional here, defaulted in the handler.
        open: z.boolean().optional().describe("Open the user's browser at the widget. Defaults to true."),
      },
      outputSchema: {
        sessionId: z.string(),
        url: z.string(),
        values: z.record(z.string(), z.unknown()),
        status: z.string(),
        lastEventSeq: z.number(),
        browserOpened: z.boolean(),
        hint: z.string().optional(),
      },
      annotations: { title: "Create an interactive widget", openWorldHint: false },
    },
    async (args) => {
      let spec;
      let previewModule = args.previewModule;
      let startingValues = args.values as Values | undefined;
      let hint: string | undefined;

      if (args.template) {
        const saved = await ctx.library.load(args.template);
        if (!saved) {
          const { results } = await ctx.library.search(args.template, 5);
          return fail(
            `No saved widget "${args.template}".` +
              (results.length ? `\nClosest matches: ${results.map((r) => r.slug).join(", ")}` : ""),
          );
        }
        spec = saved.widget.spec;
        previewModule ??= await ctx.library.previewModule(args.template);
        // Reopening where the user left it is what makes reuse feel like reuse
        // rather than starting from scratch with a familiar shape.
        startingValues ??= saved.widget.lastValues as Values;
      } else {
        if (args.spec === undefined) return fail("Pass either a spec or a template slug.");
        const parsed = parseSpec(args.spec);
        if (!parsed.ok) return fail(parsed.message);
        spec = parsed.spec;

        // Fires only when it would otherwise have been forgotten: an agent
        // hand-authoring something the library already has.
        const similar = await ctx.library.findSimilar(spec);
        if (similar) {
          hint =
            `You already have a saved widget very like this one: "${similar.summary.slug}" ` +
            `(${Math.round(similar.score * 100)}% similar). ` +
            `ui_create({ template: "${similar.summary.slug}" }) would reuse it, at the values the user last chose.`;
        }
      }

      const session = await ctx.sessions.create({
        spec,
        previewModule,
        values: startingValues,
        template: args.template,
      });
      ctx.watch(session.id);
      if (args.template) await ctx.library.recordUse(args.template);

      const url = await ctx.sessionUrl(session.id);
      const opened = args.open === false ? { opened: false, reason: "suppressed" as const } : await ctx.openInBrowser(url);
      const browserOpened = opened.opened;

      return {
        content: text(
          [
            `Widget "${session.title}" is live at ${url}`,
            openNote(opened.reason).trim() || "(open that URL to see it)",
            `sessionId: ${session.id}`,
            args.template ? `reused saved widget "${args.template}"` : "",
            "",
            "This did not block. Keep working, then call ui_wait({ sessionId }) when you want the answer.",
            hint ? `\nNOTE: ${hint}` : "",
          ]
            .filter(Boolean)
            .join("\n"),
        ),
        structuredContent: {
          sessionId: session.id,
          url,
          values: session.values,
          status: session.status,
          lastEventSeq: session.events.lastSeq,
          browserOpened,
          ...(hint ? { hint } : {}),
        },
      };
    },
  );
}

function registerGetState(server: McpServer, ctx: AgentUiContext): void {
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

function registerClose(server: McpServer, ctx: AgentUiContext): void {
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
