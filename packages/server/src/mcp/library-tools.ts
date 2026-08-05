import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { CallToolResult } from "@modelcontextprotocol/sdk/types.js";
import { LIBRARY_SCOPES, WIDGET_SLUG, slugifyWidgetName, type WidgetSummary } from "@agentui/shared";
import type { AgentUiContext } from "./context.js";

function text(body: string): CallToolResult["content"] {
  return [{ type: "text", text: body }];
}
function fail(message: string): CallToolResult {
  return { isError: true, content: text(message) };
}

function renderRows(rows: WidgetSummary[]): string {
  if (!rows.length) return "(nothing matched)";
  const width = Math.max(...rows.map((r) => r.slug.length));
  return rows
    .map((row) => {
      const bits = [row.description || "—"];
      if (row.tags.length) bits.push(`[${row.tags.join(", ")}]`);
      if (row.uses) bits.push(`used ${row.uses}×`);
      if (row.scope === "project") bits.push("(project)");
      return `  ${row.slug.padEnd(width)}  ${bits.join("  ")}`;
    })
    .join("\n");
}

export function registerLibraryTools(server: McpServer, ctx: AgentUiContext): void {
  server.registerTool(
    "ui_save",
    {
      title: "Save a widget for reuse",
      description: [
        "Saves a widget to the library so you never have to build it again — in this project or any",
        "other. Save anything you got right and might plausibly want twice.",
        "",
        "Pass a sessionId to save a live widget (its current values become the starting point next",
        "time), or pass a spec directly. Saves globally by default.",
        "",
        "`triggers` are short phrases that should surface this widget in a search later — the single",
        "most useful field for making sure it gets found rather than rebuilt.",
      ].join("\n"),
      inputSchema: {
        sessionId: z.string().optional().describe("Save this live session's widget."),
        spec: z.unknown().optional().describe("Or save a spec directly."),
        previewModule: z.string().optional(),
        name: z.string().describe('Human name, e.g. "Muzzle flash tuning".'),
        slug: z.string().optional().describe("URL-safe id. Derived from the name if omitted."),
        description: z.string().optional().describe("One line: what it is for. Shown in the agent-facing index."),
        tags: z.array(z.string()).optional(),
        triggers: z.array(z.string()).optional().describe('Phrases that should find it, e.g. ["muzzle flash","vfx timing"].'),
        scope: z.enum(LIBRARY_SCOPES).optional().describe('"user" (default, available everywhere) or "project".'),
        overwrite: z.boolean().optional(),
      },
      outputSchema: { slug: z.string(), scope: z.string(), path: z.string() },
      annotations: { title: "Save a widget for reuse", openWorldHint: false },
    },
    async (args) => {
      let spec = args.spec;
      let previewModule = args.previewModule;
      let lastValues: Record<string, unknown> | undefined;

      if (args.sessionId) {
        const session = ctx.sessions.get(args.sessionId);
        if (!session) return fail(`No session "${args.sessionId}".`);
        spec = session.spec;
        previewModule ??= await session.previewModule();
        lastValues = session.values;
      }
      if (spec === undefined) return fail("Pass either sessionId or spec.");

      const parsed = ctx.parseSpec(spec);
      if (!parsed.ok) return fail(parsed.message);

      const slug = args.slug ?? slugifyWidgetName(args.name);
      if (!WIDGET_SLUG.test(slug)) {
        return fail(`"${slug}" is not a valid slug — use lowercase letters, digits and hyphens.`);
      }

      try {
        const { widget, dir } = await ctx.library.save({
          slug,
          name: args.name,
          description: args.description,
          tags: args.tags,
          triggers: args.triggers,
          spec: parsed.spec,
          previewModule,
          lastValues,
          scope: args.scope ?? "user",
          overwrite: args.overwrite ?? false,
        });
        await ctx.refreshLibraryIndex();
        return {
          content: text(`Saved "${widget.name}" as ${widget.slug}. Reuse it with ui_create({ template: "${widget.slug}" }).`),
          structuredContent: { slug: widget.slug, scope: args.scope ?? "user", path: dir },
        };
      } catch (error) {
        return fail(String(error instanceof Error ? error.message : error));
      }
    },
  );

  server.registerTool(
    "ui_library",
    {
      title: "Find a saved widget",
      description: [
        "Searches the widget library. Check here BEFORE authoring a widget from scratch — reusing one",
        "you already got right is faster and keeps the user's settings from the last time.",
        "",
        "`query` matches names, descriptions, tags, trigger phrases and control labels. Results are",
        "compact; pass `get` with a slug to retrieve the full spec ready to instantiate or tweak.",
      ].join("\n"),
      inputSchema: {
        query: z.string().optional().describe("Search text. Omit to list the most relevant widgets."),
        get: z.string().optional().describe("Slug of a widget to fetch in full."),
        limit: z.number().int().positive().max(50).optional(),
      },
      outputSchema: {
        results: z.array(z.record(z.string(), z.unknown())).optional(),
        total: z.number().optional(),
        widget: z.record(z.string(), z.unknown()).optional(),
        previewModule: z.string().optional(),
      },
      annotations: { title: "Find a saved widget", readOnlyHint: true, openWorldHint: false },
    },
    async (args) => {
      if (args.get) {
        const found = await ctx.library.load(args.get);
        if (!found) {
          const { results } = await ctx.library.search(args.get, 5);
          return fail(
            `No saved widget "${args.get}".` + (results.length ? `\nDid you mean:\n${renderRows(results)}` : ""),
          );
        }
        const previewModule = await ctx.library.previewModule(args.get);
        return {
          content: text(
            [
              `${found.widget.name} (${found.widget.slug})`,
              found.widget.description,
              `${found.widget.spec.controls.length} controls${previewModule ? " + live preview" : ""}`,
              Object.keys(found.widget.lastValues).length
                ? `Last left at: ${JSON.stringify(found.widget.lastValues)}`
                : "",
              "",
              `Reuse as-is with ui_create({ template: "${found.widget.slug}" }).`,
            ]
              .filter(Boolean)
              .join("\n"),
          ),
          structuredContent: {
            widget: found.widget as unknown as Record<string, unknown>,
            ...(previewModule ? { previewModule } : {}),
          },
        };
      }

      const { results, total } = await ctx.library.search(args.query ?? "", args.limit ?? 10);
      return {
        content: text(
          results.length
            ? `${total} match${total === 1 ? "" : "es"}:\n${renderRows(results)}`
            : "The library is empty. Save a widget worth keeping with ui_save.",
        ),
        structuredContent: { results: results as unknown as Array<Record<string, unknown>>, total },
      };
    },
  );
}
