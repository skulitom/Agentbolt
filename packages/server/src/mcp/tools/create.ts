import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { parseSpec, type InterfaceSpec, type Values } from "@agentui/shared";
import type { AgentUiContext } from "../context.js";
import { openSavedWidget } from "../../core/library-open.js";
import { fail, openNote, text } from "./shared.js";
import { openSession } from "./session-ops.js";
import { CREATE_SPEC } from "./schemas.js";

export function createDescription(index: string): string {
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
    "sliders need `min` and `max`. `content` blocks (markdown/code/diff/image/sample) render above",
    "the controls, so you can show a diff or a screenshot and ask about it. Anything with a magnitude",
    "deserves a `previewModule` — a slider with no visible effect is worse than a text question.",
    "",
    "ui_schema({ of: 'control', type }) gives the exact fields of any control type; ui_schema({ of:",
    "'spec' }) gives the whole thing. Invalid specs come back with the exact path of each problem.",
    index ? `\n${index}` : "",
  ]
    .filter(Boolean)
    .join("\n");
}

export function registerCreate(server: McpServer, ctx: AgentUiContext, index: string) {
  return server.registerTool(
    "ui_create",
    {
      title: "Create an interactive widget",
      description: createDescription(index),
      inputSchema: {
        template: z
          .string()
          .optional()
          .describe("Slug of a saved widget to instantiate instead of describing one inline."),
        spec: CREATE_SPEC,
        previewModule: z
          .string()
          .optional()
          .describe(
            'Optional ES module for a live preview: export default { setup({container, spec, values, emit}), update(values, meta), dispose?() }. meta.source is "user" during live drags (track 1:1, never animate) or "agent" for pushed values (animate the change). Importable: "three" and "agentui" (preview helpers plus spring/tween/easings motion utilities). See ui_schema({ of: "preview-module" }).',
          ),
        values: z.record(z.string(), z.unknown()).optional().describe("Starting values, overriding the spec defaults."),
        open: z.boolean().optional().describe("Open the user's browser. Defaults to true."),
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
      let spec: InterfaceSpec;
      let previewModule = args.previewModule;
      let startingValues = args.values as Values | undefined;
      let hint: string | undefined;

      if (args.template) {
        // Shared with POST /api/library/:slug/open — the human side of the
        // library instantiates a saved widget the same way, and used to do it
        // with its own copy of these steps.
        const opened = await openSavedWidget(ctx.sessions, ctx.library, args.template, {
          previewModule,
          values: startingValues,
        });
        if (!opened.ok) {
          return fail(
            `No saved widget "${opened.slug}".` +
              (opened.near.length ? `\nClosest matches: ${opened.near.map((r) => r.slug).join(", ")}` : ""),
          );
        }
        ctx.watch(opened.session.id);
        const url = await ctx.sessionUrl(opened.session.id);
        const browser =
          args.open === false ? { opened: false, reason: "suppressed" as const } : await ctx.openInBrowser(url);
        return createResult(opened.session, url, browser, args.template, undefined);
      }

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

      const { session, url, opened } = await openSession(ctx, {
        spec,
        previewModule,
        values: startingValues,
        open: args.open,
      });
      return createResult(session, url, opened, undefined, hint);
    },
  );
}

function createResult(
  session: { id: string; title: string; values: Values; status: string; events: { lastSeq: number } },
  url: string,
  opened: { opened: boolean; reason: "opened" | "already-open" | "suppressed" | "failed" },
  template: string | undefined,
  hint: string | undefined,
) {
  return {
    content: text(
      [
        `Widget "${session.title}" is live at ${url}`,
        openNote(opened.reason).trim() || "(open that URL to see it)",
        `sessionId: ${session.id}`,
        template ? `reused saved widget "${template}"` : "",
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
      browserOpened: opened.opened,
      ...(hint ? { hint } : {}),
    },
  };
}
