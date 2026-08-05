import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import {
  CONTENT_KINDS,
  CONTROL_TYPES,
  Control,
  ContentBlock,
  InterfaceSpec,
  PREVIEW_MODULE_CONTRACT,
  PREVIEW_IMPORTS,
  PREVIEW_VIEWS,
  PREVIEW_VIEW_NAMES,
} from "@agentui/shared";
import { fail, text } from "./shared.js";
import { SCHEMA_OF, SCHEMA_TYPE } from "./schemas.js";

/**
 * The full schema, on demand.
 *
 * ui_create and ui_ask carry a shallow shape so the agent knows the containers
 * and the vocabulary without ~6k tokens riding on every request. This is where
 * the detail lives when it is actually needed: one variant is 150-400 tokens,
 * the whole control union ~1,400, the whole spec ~2,300 — paid once, by an
 * agent that has decided it needs them.
 *
 * Generated from the same zod schemas the parser uses, so it cannot drift and
 * cannot miss a type someone added.
 */

/** Input mode: what an AGENT WRITES, with defaulted fields still optional. */
const asJsonSchema = (schema: z.ZodType): unknown => z.toJSONSchema(schema, { io: "input" });

/**
 * Finds one variant of a discriminated union by its discriminant value.
 *
 * The cast is because the two unions discriminate on different keys ("type" for
 * controls, "kind" for blocks) and TypeScript cannot index a union of object
 * shapes by a union of keys. What it is reaching for exists on every variant by
 * construction — that is what makes it a discriminated union.
 */
function variantOf(
  union: typeof Control | typeof ContentBlock,
  discriminant: "type" | "kind",
  name: string,
): z.ZodType | undefined {
  return union.options.find(
    (option) => (option.shape as Record<string, { value?: string }>)[discriminant]?.value === name,
  );
}

export function registerSchemaTool(server: McpServer): void {
  server.registerTool(
    "ui_schema",
    {
      title: "Look up the exact shape of a control, block or spec",
      description: [
        "Returns the JSON Schema for part of the widget vocabulary, with every field and constraint.",
        "",
        "ui_create and ui_ask deliberately advertise only the shape of the containers, so the full",
        "detail is not costing you tokens on every request. Call this when you need the exact keys —",
        "a compare's axes, a sample's rows, a slider's bounds — rather than guessing and retrying.",
        "",
        `of:"control" with type:"slider" | ${CONTROL_TYPES.slice(1).map((t) => `"${t}"`).join(" | ")}`,
        `of:"content" with type:${CONTENT_KINDS.map((k) => `"${k}"`).join(" | ")}`,
        `of:"view" for the preview views usable in preview.views (${PREVIEW_VIEW_NAMES.join(", ")})`,
        'of:"spec" for the whole InterfaceSpec; of:"preview-module" for the module contract.',
      ].join("\n"),
      inputSchema: { of: SCHEMA_OF, type: SCHEMA_TYPE },
      outputSchema: { of: z.string(), type: z.string().optional(), schema: z.unknown().optional() },
      annotations: { title: "Look up a schema", readOnlyHint: true, openWorldHint: false },
    },
    async (args) => {
      switch (args.of) {
        case "preview-module":
          return {
            content: text(
              [
                "A preview module is an ES module with a default export:",
                "",
                PREVIEW_MODULE_CONTRACT,
                "",
                `Importable bare specifiers: ${PREVIEW_IMPORTS.join(", ")}.`,
                'meta.source is "user" during a live drag — track it 1:1, never animate — or "agent"',
                "for a pushed value, where animating the change is what makes it legible.",
              ].join("\n"),
            ),
            structuredContent: { of: args.of },
          };

        case "spec":
          return {
            content: text(JSON.stringify(asJsonSchema(InterfaceSpec), null, 2)),
            structuredContent: { of: args.of, schema: asJsonSchema(InterfaceSpec) },
          };

        case "control": {
          if (!args.type) {
            return {
              content: text(JSON.stringify(asJsonSchema(Control), null, 2)),
              structuredContent: { of: args.of, schema: asJsonSchema(Control) },
            };
          }
          const variant = variantOf(Control, "type", args.type);
          if (!variant) return fail(`No control type "${args.type}". Declared: ${CONTROL_TYPES.join(", ")}.`);
          return {
            content: text(JSON.stringify(asJsonSchema(variant), null, 2)),
            structuredContent: { of: args.of, type: args.type, schema: asJsonSchema(variant) },
          };
        }

        case "view": {
          // Not a JSON Schema: what an author needs about a view is what it
          // DRAWS and what it accepts, which is the registry itself.
          const names = args.type ? [args.type] : PREVIEW_VIEW_NAMES;
          const unknown = names.filter((name) => !PREVIEW_VIEWS[name]);
          if (unknown.length) {
            return fail(`No preview view "${unknown[0]}". Declared: ${PREVIEW_VIEW_NAMES.join(", ")}.`);
          }
          const rows = names.map((name) => {
            const view = PREVIEW_VIEWS[name]!;
            const accepts = view.accepts === "*" ? "any control" : [...view.accepts].join(", ");
            return {
              view: name,
              label: view.label,
              accepts,
              minControls: view.min ?? 1,
              opensAFrame: view.draws,
            };
          });
          return {
            content: text(
              [
                "Views for `preview.views: [{ view, controls, title? }]`.",
                "Declaring any view replaces inference entirely.",
                "",
                ...rows.map(
                  (r) =>
                    `  ${r.view.padEnd(8)} ${r.label.padEnd(13)} draws: ${r.accepts}` +
                    (r.minControls > 1 ? ` (needs ${r.minControls}+)` : "") +
                    (r.opensAFrame ? "" : "  — never opens a frame on its own"),
                ),
              ].join("\n"),
            ),
            structuredContent: { of: args.of, ...(args.type ? { type: args.type } : {}), schema: rows },
          };
        }

        case "content": {
          if (!args.type) {
            return {
              content: text(JSON.stringify(asJsonSchema(ContentBlock), null, 2)),
              structuredContent: { of: args.of, schema: asJsonSchema(ContentBlock) },
            };
          }
          const variant = variantOf(ContentBlock, "kind", args.type);
          if (!variant) return fail(`No content kind "${args.type}". Declared: ${CONTENT_KINDS.join(", ")}.`);
          return {
            content: text(JSON.stringify(asJsonSchema(variant), null, 2)),
            structuredContent: { of: args.of, type: args.type, schema: asJsonSchema(variant) },
          };
        }
      }
    },
  );
}
