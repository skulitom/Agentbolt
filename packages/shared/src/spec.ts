import { z } from "zod";

/**
 * The Interface Spec: the JSON an agent authors to describe a widget.
 *
 * Two rules shape every decision in this file:
 *
 * 1. It must be easy for a language model to get right first time. That means a
 *    FLAT controls array (no nesting), short property names, `default` on every
 *    control, and a discriminated union so a mistake is reported against one
 *    variant instead of seven.
 * 2. When it does get it wrong, the error has to teach. Every object is strict,
 *    so `minimum: 0` instead of `min: 0` is caught by name rather than silently
 *    dropped, and formatSpecError() turns issues into per-path one-liners.
 */

const SLUG = /^[a-z][a-z0-9_]*$/;

const idField = z
  .string()
  .regex(SLUG, "must be snake_case: lowercase letters, digits and underscores, starting with a letter");

const controlBase = {
  id: idField,
  label: z.string().min(1),
  description: z.string().optional(),
  /** Controls sharing a group render together, in first-appearance order. */
  group: z.string().optional(),
};

const HEX_COLOR = /^#[0-9a-fA-F]{6}$/;

export const SliderControl = z.strictObject({
  ...controlBase,
  type: z.literal("slider"),
  min: z.number(),
  max: z.number(),
  step: z.number().positive().default(0.01),
  default: z.number(),
  unit: z.string().optional(),
});

export const NumberControl = z.strictObject({
  ...controlBase,
  type: z.literal("number"),
  min: z.number().optional(),
  max: z.number().optional(),
  step: z.number().positive().optional(),
  default: z.number(),
  unit: z.string().optional(),
});

export const SelectControl = z.strictObject({
  ...controlBase,
  type: z.literal("select"),
  options: z
    .array(z.strictObject({ value: z.string(), label: z.string().optional() }))
    .min(1, "a select needs at least one option"),
  default: z.string(),
});

export const ToggleControl = z.strictObject({
  ...controlBase,
  type: z.literal("toggle"),
  default: z.boolean(),
});

export const ColorControl = z.strictObject({
  ...controlBase,
  type: z.literal("color"),
  default: z.string().regex(HEX_COLOR, "must be a 6-digit hex colour like #ef9f27"),
});

export const TextControl = z.strictObject({
  ...controlBase,
  type: z.literal("text"),
  default: z.string().default(""),
  multiline: z.boolean().default(false),
  placeholder: z.string().optional(),
});

export const VectorControl = z.strictObject({
  ...controlBase,
  type: z.literal("vector"),
  size: z.union([z.literal(2), z.literal(3)]),
  default: z.array(z.number()),
  min: z.number().optional(),
  max: z.number().optional(),
  step: z.number().positive().optional(),
  labels: z.array(z.string()).optional(),
});

export const Control = z.discriminatedUnion("type", [
  SliderControl,
  NumberControl,
  SelectControl,
  ToggleControl,
  ColorControl,
  TextControl,
  VectorControl,
]);

export type Control = z.infer<typeof Control>;
export type ControlType = Control["type"];

/**
 * Read-only content shown above the controls, so an agent can *show* something
 * and ask about it — a diff, a screenshot it just took, a block of prose.
 */
export const ContentBlock = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("markdown"), value: z.string() }),
  z.strictObject({ kind: z.literal("code"), value: z.string(), language: z.string().optional() }),
  z.strictObject({ kind: z.literal("diff"), value: z.string(), filename: z.string().optional() }),
  z.strictObject({
    kind: z.literal("image"),
    /** Absolute local path; the server copies it into the session's assets dir. */
    path: z.string().optional(),
    /** Or an inline data: URI, for something the agent generated in memory. */
    dataUri: z.string().optional(),
    alt: z.string().optional(),
    caption: z.string().optional(),
  }),
  /** Untrusted: rendered inside the same sandboxed frame as preview modules. */
  z.strictObject({ kind: z.literal("html"), value: z.string(), height: z.number().int().positive().optional() }),
]);

export type ContentBlock = z.infer<typeof ContentBlock>;

/**
 * Action-bar buttons. When a spec declares actions, clicking one finalizes the
 * session and records which was chosen — this is how ui_ask's option buttons
 * are expressed without a second code path.
 */
export const Action = z.strictObject({
  id: idField,
  label: z.string().min(1),
  style: z.enum(["default", "primary", "danger"]).default("default"),
});

export type Action = z.infer<typeof Action>;

export const InterfaceSpec = z
  .strictObject({
    schemaVersion: z.literal(1).default(1),
    title: z.string().min(1),
    description: z.string().optional(),
    content: z.array(ContentBlock).default([]),
    controls: z.array(Control).default([]),
    actions: z.array(Action).default([]),
    /** Free-text box for the human. Defaults on — it is how you say "why". */
    comment: z
      .strictObject({
        enabled: z.boolean().default(true),
        placeholder: z.string().optional(),
        label: z.string().optional(),
      })
      .default({ enabled: true }),
    preview: z
      .strictObject({
        height: z.number().int().min(120).max(1200).default(400),
      })
      .optional(),
    layout: z
      .strictObject({
        columns: z.union([z.literal(1), z.literal(2)]).default(1),
        previewPosition: z.enum(["top", "right"]).default("top"),
      })
      .optional(),
  })
  .superRefine((spec, ctx) => {
    if (spec.controls.length === 0 && spec.actions.length === 0 && spec.content.length === 0) {
      ctx.addIssue({
        code: "custom",
        path: [],
        message: "a widget needs at least one of: controls, actions, or content",
      });
    }

    const seen = new Map<string, number>();
    spec.controls.forEach((control, index) => {
      const first = seen.get(control.id);
      if (first !== undefined) {
        ctx.addIssue({
          code: "custom",
          path: ["controls", index, "id"],
          message: `duplicate control id "${control.id}" (already used by controls[${first}])`,
        });
      } else {
        seen.set(control.id, index);
      }

      if (control.type === "slider" || control.type === "number") {
        const { min, max } = control;
        if (min !== undefined && max !== undefined && min >= max) {
          ctx.addIssue({
            code: "custom",
            path: ["controls", index, "min"],
            message: `min (${min}) must be less than max (${max})`,
          });
        }
        if (min !== undefined && control.default < min) {
          ctx.addIssue({
            code: "custom",
            path: ["controls", index, "default"],
            message: `default ${control.default} is below min ${min}`,
          });
        }
        if (max !== undefined && control.default > max) {
          ctx.addIssue({
            code: "custom",
            path: ["controls", index, "default"],
            message: `default ${control.default} is above max ${max}`,
          });
        }
      }

      if (control.type === "select") {
        const values = control.options.map((o) => o.value);
        if (!values.includes(control.default)) {
          ctx.addIssue({
            code: "custom",
            path: ["controls", index, "default"],
            message: `default "${control.default}" is not one of the options (${values.join(", ")})`,
          });
        }
      }

      if (control.type === "vector") {
        if (control.default.length !== control.size) {
          ctx.addIssue({
            code: "custom",
            path: ["controls", index, "default"],
            message: `expected ${control.size} numbers to match size, got ${control.default.length}`,
          });
        }
        if (control.labels && control.labels.length !== control.size) {
          ctx.addIssue({
            code: "custom",
            path: ["controls", index, "labels"],
            message: `expected ${control.size} labels to match size, got ${control.labels.length}`,
          });
        }
      }
    });

    const actionIds = new Set<string>();
    spec.actions.forEach((action, index) => {
      if (actionIds.has(action.id)) {
        ctx.addIssue({
          code: "custom",
          path: ["actions", index, "id"],
          message: `duplicate action id "${action.id}"`,
        });
      }
      actionIds.add(action.id);
    });

    spec.content.forEach((block, index) => {
      if (block.kind === "image" && !block.path && !block.dataUri) {
        ctx.addIssue({
          code: "custom",
          path: ["content", index],
          message: "an image block needs either `path` (absolute local path) or `dataUri`",
        });
      }
    });
  });

/** What an agent writes (defaults still optional). */
export type InterfaceSpecInput = z.input<typeof InterfaceSpec>;
/** What the rest of the system sees (defaults filled in). */
export type InterfaceSpec = z.output<typeof InterfaceSpec>;

export type ControlValue = string | number | boolean | number[];
export type Values = Record<string, ControlValue>;

export function defaultsFromSpec(spec: InterfaceSpec): Values {
  const values: Values = {};
  for (const control of spec.controls) {
    values[control.id] = control.type === "vector" ? [...control.default] : control.default;
  }
  return values;
}

/**
 * Groups in first-appearance order; ungrouped controls come first under "".
 *
 * When `visualFirst` is set (which the renderer does whenever a preview is
 * showing), groups with nothing the preview can illustrate sink below the ones
 * it can. Authored group labels are preserved either way — reordering respects
 * the agent's grouping instead of flattening it, so "Behaviour" stays
 * "Behaviour", it just stops sitting between two groups you can watch change.
 */
export function groupControls(
  spec: InterfaceSpec,
  options: { visualFirst?: boolean; isVisual?: (control: Control) => boolean } = {},
): Array<{ group: string; controls: Control[] }> {
  const order: string[] = [];
  const buckets = new Map<string, Control[]>();
  for (const control of spec.controls) {
    const key = control.group ?? "";
    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = [];
      buckets.set(key, bucket);
      order.push(key);
    }
    bucket.push(control);
  }
  const grouped = order.map((group) => ({ group, controls: buckets.get(group) ?? [] }));
  if (!options.visualFirst || !options.isVisual) return grouped;

  const isVisual = options.isVisual;
  // Stable partition: relative order is kept inside each half.
  const withVisual = grouped.filter((entry) => entry.controls.some(isVisual));
  const withoutVisual = grouped.filter((entry) => !entry.controls.some(isVisual));
  return [...withVisual, ...withoutVisual];
}

export interface ValueProblem {
  id: string;
  message: string;
}

/**
 * Validates a values object against a spec. Used both for values pushed by an
 * agent via ui_update and for values arriving from the browser — the server
 * never trusts either side.
 */
export function validateValuesAgainstSpec(spec: InterfaceSpec, values: unknown): ValueProblem[] {
  if (typeof values !== "object" || values === null || Array.isArray(values)) {
    return [{ id: "", message: "values must be a plain object keyed by control id" }];
  }
  const record = values as Record<string, unknown>;
  const byId = new Map(spec.controls.map((c) => [c.id, c]));
  const problems: ValueProblem[] = [];

  for (const key of Object.keys(record)) {
    if (!byId.has(key)) problems.push({ id: key, message: `no control with id "${key}" in this widget` });
  }

  for (const control of spec.controls) {
    if (!(control.id in record)) continue;
    const value = record[control.id];
    switch (control.type) {
      case "slider":
      case "number": {
        if (typeof value !== "number" || Number.isNaN(value)) {
          problems.push({ id: control.id, message: `expected a number, got ${describe(value)}` });
        } else if (control.min !== undefined && value < control.min) {
          problems.push({ id: control.id, message: `${value} is below min ${control.min}` });
        } else if (control.max !== undefined && value > control.max) {
          problems.push({ id: control.id, message: `${value} is above max ${control.max}` });
        }
        break;
      }
      case "toggle":
        if (typeof value !== "boolean") {
          problems.push({ id: control.id, message: `expected true or false, got ${describe(value)}` });
        }
        break;
      case "color":
        if (typeof value !== "string" || !HEX_COLOR.test(value)) {
          problems.push({ id: control.id, message: `expected a hex colour like #ef9f27, got ${describe(value)}` });
        }
        break;
      case "text":
        if (typeof value !== "string") {
          problems.push({ id: control.id, message: `expected a string, got ${describe(value)}` });
        }
        break;
      case "select": {
        const allowed = control.options.map((o) => o.value);
        if (typeof value !== "string" || !allowed.includes(value)) {
          problems.push({
            id: control.id,
            message: `expected one of ${allowed.join(", ")}, got ${describe(value)}`,
          });
        }
        break;
      }
      case "vector": {
        if (!Array.isArray(value) || value.length !== control.size || value.some((n) => typeof n !== "number")) {
          problems.push({
            id: control.id,
            message: `expected ${control.size} numbers, got ${describe(value)}`,
          });
        }
        break;
      }
    }
  }
  return problems;
}

function describe(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return `an array of ${value.length}`;
  if (typeof value === "string") return `the string ${JSON.stringify(value)}`;
  return typeof value;
}

/** Coerces values to be safe to render: fills missing, drops unknown, clamps. */
export function normalizeValues(spec: InterfaceSpec, values: Values): Values {
  const defaults = defaultsFromSpec(spec);
  const out: Values = { ...defaults };
  for (const control of spec.controls) {
    const incoming = values[control.id];
    if (incoming === undefined) continue;
    if (validateValuesAgainstSpec(spec, { [control.id]: incoming }).length === 0) {
      out[control.id] = control.type === "vector" ? [...(incoming as number[])] : incoming;
    }
  }
  return out;
}

const DOC_HINT =
  "See the agentui skill for a worked example, or call ui_library({ query }) to reuse a saved widget.";

/**
 * Turns zod issues into per-path one-liners a model can act on directly.
 * This is the difference between an agent self-correcting in one retry and
 * flailing through five.
 */
export function formatSpecError(error: z.ZodError): string {
  const lines = error.issues.map((issue) => {
    const path = issue.path.length ? formatPath(issue.path) : "(root)";
    return `  ${path}: ${issue.message}`;
  });
  const unique = [...new Set(lines)];
  return [`Invalid widget spec (${unique.length} problem${unique.length === 1 ? "" : "s"}):`, ...unique, DOC_HINT].join(
    "\n",
  );
}

function formatPath(path: ReadonlyArray<PropertyKey>): string {
  let out = "";
  for (const segment of path) {
    if (typeof segment === "number") out += `[${segment}]`;
    else out += out ? `.${String(segment)}` : String(segment);
  }
  return out;
}

export interface ParsedSpec {
  ok: true;
  spec: InterfaceSpec;
}
export interface FailedSpec {
  ok: false;
  message: string;
}

export function parseSpec(input: unknown): ParsedSpec | FailedSpec {
  const result = InterfaceSpec.safeParse(input);
  if (result.success) return { ok: true, spec: result.data };
  return { ok: false, message: formatSpecError(result.error) };
}
