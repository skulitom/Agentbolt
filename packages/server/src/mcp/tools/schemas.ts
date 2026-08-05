import { z } from "zod";
import { CONTENT_KINDS, CONTROL_TYPES, FieldShorthand, PREVIEW_VIEW_NAMES } from "@agentui/shared";

/**
 * What the AGENT sees of the spec.
 *
 * Measured, not guessed: `InterfaceSpec` as JSON Schema is 7,725 chars (~1,930
 * tokens) and `AskInput` is 4,678 (~1,170), so inlining the real schemas on
 * ui_create, ui_ask and ui_save costs about 5,000 tokens RESIDENT ON EVERY
 * REQUEST. That is why those parameters were `z.unknown()`, and the reasoning
 * was sound — but the consequence was that the only structural feedback an
 * agent ever got was a failed call.
 *
 * The middle path is shallow-but-typed. `z.looseObject` passes unknown keys
 * through untouched, so `min`, `max`, `options`, `axes` and `rows` still reach
 * the handler and are still validated by the real `parseSpec`. What the agent
 * gains is the shape of the containers and — the part that actually prevents
 * failed calls — the enumerated vocabulary: which control types and content
 * kinds exist, derived from the registries rather than restated. Full detail is
 * one cheap `ui_schema` call away.
 *
 * TRAP: no `.default()` in anything exported from here. zod 4's toJSONSchema
 * runs in "output" mode for tool schemas, under which a defaulted field is
 * emitted as REQUIRED — so a convenience default turns into a mandatory
 * argument in the agent's eyes. A size-budget test in mcp-e2e guards the total.
 */

const CONTROL_TYPE_ENUM = z.enum(CONTROL_TYPES as [string, ...string[]]);
const CONTENT_KIND_ENUM = z.enum(CONTENT_KINDS as [string, ...string[]]);

/* ------------------------------------------------------------------ ui_ask */

export const ASK_OPTIONS = z
  .array(z.union([z.string(), z.looseObject({ label: z.string(), style: z.enum(["default", "primary", "danger"]).optional() })]))
  .optional()
  .describe('Buttons. Strings, or {label, style?}. Pressing one answers.');

export const ASK_FIELDS = z
  .array(FieldShorthand)
  .optional()
  .describe(
    "Controls to fill in. ids are snake_case. Everything but `id` is optional — the type is inferred from `default` when omitted. Type-specific keys (min/max, options, axes, size) pass through; see ui_schema.",
  );

const SHALLOW_BLOCK = z
  .looseObject({ kind: CONTENT_KIND_ENUM })
  .describe("A content block. Remaining keys depend on `kind` — see ui_schema({ of: 'content' }).");

export const ASK_CONTENT = z
  .union([z.string(), SHALLOW_BLOCK, z.array(z.union([z.string(), SHALLOW_BLOCK]))])
  .optional()
  .describe("Something to look at: a markdown string, or content blocks.");

/**
 * Shared by ui_ask and ui_create. `views` is spelled out rather than left to
 * pass through, because it is the one preview field an agent cannot guess: the
 * list of view names is the whole vocabulary.
 */
const PREVIEW_OPTIONS = z.looseObject({
  auto: z.boolean().optional(),
  height: z.number().int().optional(),
  position: z.enum(["top", "right"]).optional(),
  views: z
    .array(
      z.looseObject({
        view: z.enum(PREVIEW_VIEW_NAMES as [string, ...string[]]),
        controls: z.array(z.string()),
        title: z.string().optional(),
      }),
    )
    .optional()
    .describe("Arrange the generated preview yourself. Declaring any view replaces inference entirely."),
});

export const ASK_PREVIEW = PREVIEW_OPTIONS.optional().describe(
  "{ auto: false } declines the generated preview — use it when a sample or compare already shows the effect. `views` arranges it by hand.",
);

/* --------------------------------------------------------------- ui_create */

export const CREATE_SPEC = z
  .looseObject({
    title: z.string(),
    description: z.string().optional(),
    content: z.array(SHALLOW_BLOCK).optional(),
    controls: z
      .array(
        z.looseObject({
          id: z.string(),
          label: z.string(),
          type: CONTROL_TYPE_ENUM,
          group: z.string().optional(),
          description: z.string().optional(),
        }),
      )
      .optional(),
    actions: z.array(z.looseObject({ id: z.string(), label: z.string() })).optional(),
    preview: PREVIEW_OPTIONS.optional(),
  })
  .optional()
  .describe(
    "The widget. Every control needs `id`, `label`, `type` and `default`; type-specific keys (min/max for sliders, options for selects, axes for compare) pass through and are validated on arrival. Call ui_schema for the full shape of any type.",
  );

/* --------------------------------------------------------------- ui_schema */

export const SCHEMA_OF = z
  .enum(["spec", "control", "content", "view", "preview-module"])
  .describe("Which part of the vocabulary to describe.");

export const SCHEMA_TYPE = z
  .string()
  .optional()
  .describe(
    `With of:"control", one of ${CONTROL_TYPES.join(", ")}. With of:"content", one of ${CONTENT_KINDS.join(", ")}. Omit for the whole union.`,
  );
