import { z } from "zod";
import { collectPlaceholders, isBindTransform } from "./bind.js";
import { RULE_SHAPE, parseSampleRule } from "./sample.js";

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

/**
 * A select whose options each carry a column: the same axes, answered for every
 * candidate.
 *
 * The VALUE is still `options[].value`, a plain string, so nothing downstream —
 * ControlValue, normalizeValues, state.json, the ws protocol — learns a new
 * shape. What the extra schema buys is validation: every option must fill every
 * axis. Prose lets an agent write two sentences about approach A's performance
 * and none about B's; a matrix has a hole where B's cell goes, and the parser
 * refuses to ship the hole. That forced parallelism is the entire point.
 */
export const CompareControl = z.strictObject({
  ...controlBase,
  type: z.literal("compare"),
  axes: z
    .array(
      z.strictObject({
        id: idField,
        label: z.string().min(1),
        kind: z.enum(["text", "bool", "code", "scale"]).default("text"),
        /** Required when kind is "scale": the ordered vocabulary, worst first. */
        scale: z.array(z.string()).min(2, "a scale needs at least two steps").optional(),
        /**
         * Which end wins. "none" is the default and is load-bearing: the tool
         * has to be able to say "these differ and neither is better" rather than
         * colouring in a winner it has not earned.
         */
        better: z.enum(["low", "high", "none"]).default("none"),
        /**
         * Generates this axis's cells from the option value: "npm i {}". Four
         * templates instead of sixteen hand-written cells, and the cells cannot
         * drift out of parallel.
         */
        template: z.string().optional(),
      }),
    )
    .min(2, "a compare with one axis is a select — use that instead"),
  options: z
    .array(
      z.strictObject({
        value: z.string().min(1),
        label: z.string().optional(),
        summary: z.string().optional(),
        /** Keyed by axis id. Every non-template axis must be present. */
        cells: z.record(z.string(), z.union([z.string(), z.boolean()])).default({}),
      }),
    )
    .min(2, "a compare needs at least two options"),
  default: z.string(),
});

/**
 * A subset of a known set, with a live total.
 *
 * The one control the union genuinely lacked, and the one that serves a
 * permanent family of questions: which files, which tests, which clients, which
 * packages. It is also the release that teaches ControlValue about `string[]`.
 *
 * `weight` is something MEASURED — lines from `wc -l`, kB from `du`. With no
 * weights this is a checkbox list that says "2 of 3 selected", which is honest
 * degradation; the docs must not oversell it as a chart.
 */
export const MultiSelectControl = z.strictObject({
  ...controlBase,
  type: z.literal("multiselect"),
  options: z
    .array(
      z.strictObject({
        value: z.string().min(1),
        label: z.string().optional(),
        note: z.string().optional(),
        weight: z.number().optional(),
      }),
    )
    .min(2, "a multiselect with one option is a toggle — use that instead"),
  /** Unit for the summed weights: "lines", " kB", "s". */
  weightUnit: z.string().optional(),
  default: z.array(z.string()),
});

export const Control = z.discriminatedUnion("type", [
  SliderControl,
  NumberControl,
  SelectControl,
  ToggleControl,
  ColorControl,
  TextControl,
  VectorControl,
  CompareControl,
  MultiSelectControl,
]);

export type Control = z.infer<typeof Control>;
export type ControlType = Control["type"];

/**
 * Read-only content shown above the controls, so an agent can *show* something
 * and ask about it — a diff, a screenshot it just took, a block of prose.
 */
export const ContentBlock = z.discriminatedUnion("kind", [
  /** `bind` substitutes `{{control_id}}` live — see bind.ts for the rules. */
  z.strictObject({ kind: z.literal("markdown"), value: z.string(), bind: z.boolean().default(false) }),
  z.strictObject({
    kind: z.literal("code"),
    value: z.string(),
    language: z.string().optional(),
    filename: z.string().optional(),
    bind: z.boolean().default(false),
  }),
  z.strictObject({ kind: z.literal("diff"), value: z.string(), filename: z.string().optional() }),
  /**
   * A rule replayed against rows the reader recognises.
   *
   * `source` is REQUIRED and always rendered, because the failure this block
   * exists to prevent is a convincing table of numbers the agent made up. An
   * agent that has to type "illustrative — I have not read your machine" will
   * usually go and read it instead, and a human who sees that string discounts
   * correctly. Same move as `touched`: make the weak case visible rather than
   * asking anyone to remember it.
   */
  z.strictObject({
    kind: z.literal("sample"),
    label: z.string().min(1),
    /** A path, a command, or the formula. The provenance gate. */
    source: z.string().min(1),
    /** Exactly "<row_field> <op> <control_id>". Not an expression language. */
    rule: z.string(),
    /** [when the rule holds, when it does not]. */
    verdict: z.tuple([z.string(), z.string()]).optional(),
    /** Numeric field to sort by and show. Defaults to the rule's field. */
    field: z.string().optional(),
    unit: z.string().optional(),
    /** Live sum over matching rows. */
    total: z
      .strictObject({ field: z.string(), unit: z.string().optional(), label: z.string().optional() })
      .optional(),
    rows: z
      .array(z.looseObject({ label: z.string().min(1), note: z.string().optional() }))
      .min(2, "with fewer than two rows you have a sentence, not a table")
      .max(60),
  }),
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

/**
 * Named rather than inline so `ui_ask` can accept the same object: a shorthand
 * question with a `sample` block and a slider that carries a unit needs to be
 * able to decline the generated frame, or it gets a bar redrawing what the
 * table already said.
 */
export const PreviewOptions = z.strictObject({
  height: z.number().int().min(120).max(1200).default(400),
  /**
   * Set false to decline the generated preview. For a widget whose values
   * genuinely have nothing to show, an empty frame is worse than none — and the
   * alternative workaround (dropping `unit`) would corrupt the data to fix the
   * presentation.
   */
  auto: z.boolean().default(true),
});

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
    preview: PreviewOptions.optional(),
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

      if (control.type === "compare") checkCompare(control, index, ctx);
      if (control.type === "multiselect") checkMultiSelect(control, index, ctx);
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

    const controlsById = new Map(spec.controls.map((control) => [control.id, control]));
    spec.content.forEach((block, index) => {
      if (block.kind === "image" && !block.path && !block.dataUri) {
        ctx.addIssue({
          code: "custom",
          path: ["content", index],
          message: "an image block needs either `path` (absolute local path) or `dataUri`",
        });
      }
      if (block.kind === "sample") checkSample(block, index, controlsById, ctx);
      if ((block.kind === "markdown" || block.kind === "code") && block.bind) {
        checkBoundBlock(block.value, index, controlsById, ctx);
      }
    });
  });

/** Structural, so these helpers do not depend on zod's internal ctx type. */
interface Ctx {
  addIssue(issue: { code: "custom"; path: Array<string | number>; message: string }): void;
}

/**
 * The forced-parallelism check. A missing cell is not a nicety: it is the exact
 * place where prose would have let the agent argue one side and go quiet on the
 * other, so it is reported per option, per axis, by name.
 */
function checkCompare(control: z.infer<typeof CompareControl>, index: number, ctx: Ctx): void {
  const at = (...rest: Array<string | number>) => ["controls", index, ...rest];

  const axisIds = new Set<string>();
  control.axes.forEach((axis, a) => {
    if (axisIds.has(axis.id)) {
      ctx.addIssue({ code: "custom", path: at("axes", a, "id"), message: `duplicate axis id "${axis.id}"` });
    }
    axisIds.add(axis.id);
    if (axis.kind === "scale" && !axis.scale) {
      ctx.addIssue({
        code: "custom",
        path: at("axes", a, "scale"),
        message: 'a "scale" axis needs its `scale`, worst first — e.g. ["S", "M", "L"]',
      });
    }
    if (axis.kind !== "scale" && axis.scale) {
      ctx.addIssue({
        code: "custom",
        path: at("axes", a, "kind"),
        message: '`scale` only means something on kind: "scale"',
      });
    }
  });

  const values = new Set<string>();
  control.options.forEach((option, o) => {
    if (values.has(option.value)) {
      ctx.addIssue({ code: "custom", path: at("options", o, "value"), message: `duplicate option value "${option.value}"` });
    }
    values.add(option.value);

    for (const [cellId, cell] of Object.entries(option.cells)) {
      const axis = control.axes.find((x) => x.id === cellId);
      if (!axis) {
        ctx.addIssue({
          code: "custom",
          path: at("options", o, "cells", cellId),
          message: `no axis with id "${cellId}" (declared: ${[...axisIds].join(", ")})`,
        });
        continue;
      }
      if (axis.template !== undefined) {
        ctx.addIssue({
          code: "custom",
          path: at("options", o, "cells", cellId),
          message: `axis "${cellId}" generates its cells from \`template\`; remove this one or drop the template`,
        });
        continue;
      }
      if (typeof cell === "boolean" && axis.kind !== "bool") {
        ctx.addIssue({
          code: "custom",
          path: at("options", o, "cells", cellId),
          message: `axis "${cellId}" is kind "${axis.kind}", so its cells are strings`,
        });
      }
      if (axis.kind === "bool" && typeof cell !== "boolean") {
        ctx.addIssue({
          code: "custom",
          path: at("options", o, "cells", cellId),
          message: `axis "${cellId}" is kind "bool", so its cells are true or false`,
        });
      }
      if (axis.kind === "scale" && axis.scale && typeof cell === "string" && !axis.scale.includes(cell)) {
        ctx.addIssue({
          code: "custom",
          path: at("options", o, "cells", cellId),
          message: `"${cell}" is not one of axis "${cellId}"'s scale (${axis.scale.join(", ")})`,
        });
      }
    }

    for (const axis of control.axes) {
      if (axis.template === undefined && !(axis.id in option.cells)) {
        ctx.addIssue({
          code: "custom",
          path: at("options", o, "cells"),
          message: `missing cell for axis "${axis.id}" — every option answers every axis, or the comparison has a hole in it`,
        });
      }
    }
  });

  if (!values.has(control.default)) {
    ctx.addIssue({
      code: "custom",
      path: at("default"),
      message: `default "${control.default}" is not one of the options (${[...values].join(", ")})`,
    });
  }
}

function checkMultiSelect(control: z.infer<typeof MultiSelectControl>, index: number, ctx: Ctx): void {
  const values = new Set<string>();
  control.options.forEach((option, o) => {
    if (values.has(option.value)) {
      ctx.addIssue({
        code: "custom",
        path: ["controls", index, "options", o, "value"],
        message: `duplicate option value "${option.value}"`,
      });
    }
    values.add(option.value);
  });

  const chosen = new Set<string>();
  control.default.forEach((value, d) => {
    if (!values.has(value)) {
      ctx.addIssue({
        code: "custom",
        path: ["controls", index, "default", d],
        message: `"${value}" is not one of the options (${[...values].join(", ")})`,
      });
    }
    if (chosen.has(value)) {
      ctx.addIssue({
        code: "custom",
        path: ["controls", index, "default", d],
        message: `"${value}" is selected twice`,
      });
    }
    chosen.add(value);
  });
}

/**
 * Everything a `sample` claims is checked here, against the rows themselves —
 * a rule naming a field no row carries, or a `>` over something that is not a
 * number, is a per-path error at parse time rather than a NaN on screen.
 */
function checkSample(
  block: Extract<z.infer<typeof ContentBlock>, { kind: "sample" }>,
  index: number,
  controlsById: Map<string, z.infer<typeof Control>>,
  ctx: Ctx,
): void {
  const at = (...rest: Array<string | number>) => ["content", index, ...rest];
  const rule = parseSampleRule(block.rule);
  if (!rule) {
    ctx.addIssue({ code: "custom", path: at("rule"), message: `${RULE_SHAPE} — got "${block.rule}"` });
    return;
  }

  const control = controlsById.get(rule.control);
  if (!control) {
    const ids = [...controlsById.keys()];
    ctx.addIssue({
      code: "custom",
      path: at("rule"),
      message: `"${rule.control}" is not a declared control id (${ids.length ? ids.join(", ") : "this widget has no controls"}); the right-hand side of a rule is always a control`,
    });
  } else if (rule.ordering && control.type !== "slider" && control.type !== "number") {
    ctx.addIssue({
      code: "custom",
      path: at("rule"),
      message: `"${rule.op}" compares numbers, but "${rule.control}" is a ${control.type} — use == or != to match it as a string`,
    });
  }

  const rows = block.rows as Array<Record<string, unknown>>;
  const requireField = (field: string, where: string, numeric: boolean): void => {
    rows.forEach((row, r) => {
      const value = row[field];
      if (value === undefined) {
        ctx.addIssue({
          code: "custom",
          path: at("rows", r),
          message: `no "${field}" on this row, but ${where} names it`,
        });
        return;
      }
      if (numeric && !Number.isFinite(Number(value))) {
        ctx.addIssue({
          code: "custom",
          path: at("rows", r, field),
          message: `${where} needs a number here, got ${JSON.stringify(value)}`,
        });
      }
    });
  };

  requireField(rule.field, "`rule`", rule.ordering);
  // An explicit `field` is declared as the numeric column to sort by and show;
  // when it is omitted the rule's own field does that job and is already checked.
  if (block.field !== undefined && block.field !== rule.field) requireField(block.field, "`field`", true);
  if (block.total) requireField(block.total.field, "`total.field`", true);
}

/**
 * Placeholder validation runs in ONE direction only: inside a bound block, a
 * `{{name}}` that is not a declared control is an error. Braces in an UNBOUND
 * block are left entirely alone — erroring on those would make a widget about a
 * Helm chart or a GitHub Action unauthorable.
 */
function checkBoundBlock(
  value: string,
  index: number,
  controlsById: Map<string, z.infer<typeof Control>>,
  ctx: Ctx,
): void {
  for (const placeholder of collectPlaceholders(value)) {
    const path = ["content", index, "value"];
    if (placeholder.transform !== undefined && !isBindTransform(placeholder.transform)) {
      ctx.addIssue({
        code: "custom",
        path,
        message: `${placeholder.raw}: unknown transform ":${placeholder.transform}" (pascal, camel, kebab)`,
      });
      continue;
    }
    const control = controlsById.get(placeholder.id);
    if (!control) {
      const ids = [...controlsById.keys()];
      ctx.addIssue({
        code: "custom",
        path,
        message: `${placeholder.raw} names no control (declared: ${ids.length ? ids.join(", ") : "none"}). Drop \`bind\` if the braces are meant literally.`,
      });
      continue;
    }
    if (control.type === "toggle") {
      ctx.addIssue({
        code: "custom",
        path,
        message: `${placeholder.raw} is a toggle, and "true" is not what you want in the text — use a select whose option VALUES are the alternative lines, and bind that`,
      });
    }
    if (control.type === "multiselect" || control.type === "vector") {
      ctx.addIssue({
        code: "custom",
        path,
        message: `${placeholder.raw} is a ${control.type}, which is a list, not a string — show the selection with a \`sample\` block instead`,
      });
    }
  }
}

/** What an agent writes (defaults still optional). */
export type InterfaceSpecInput = z.input<typeof InterfaceSpec>;
/** What the rest of the system sees (defaults filled in). */
export type InterfaceSpec = z.output<typeof InterfaceSpec>;

export type ControlValue = string | number | boolean | number[] | string[];
export type Values = Record<string, ControlValue>;

/**
 * Array-valued controls — `vector` and now `multiselect` — must be COPIED, or a
 * caller that mutates its values mutates the spec's defaults with it.
 */
function cloneValue(control: Control, value: ControlValue): ControlValue {
  if (!Array.isArray(value)) return value;
  if (control.type === "vector") return [...(value as number[])];
  if (control.type === "multiselect") return [...(value as string[])];
  return value;
}

export function defaultsFromSpec(spec: InterfaceSpec): Values {
  const values: Values = {};
  for (const control of spec.controls) {
    values[control.id] = cloneValue(control, control.default);
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
      // A compare's value IS a select's value: one option value, a plain string.
      // That is the whole cost argument for the control, and it holds here.
      case "select":
      case "compare": {
        const allowed = control.options.map((o) => o.value);
        if (typeof value !== "string" || !allowed.includes(value)) {
          problems.push({
            id: control.id,
            message: `expected one of ${allowed.join(", ")}, got ${describe(value)}`,
          });
        }
        break;
      }
      case "multiselect": {
        const allowed = control.options.map((o) => o.value);
        if (!Array.isArray(value) || value.some((v) => typeof v !== "string")) {
          problems.push({ id: control.id, message: `expected an array of option values, got ${describe(value)}` });
          break;
        }
        const unknown = value.filter((v) => !allowed.includes(v as string));
        if (unknown.length) {
          problems.push({
            id: control.id,
            message: `not options of this control: ${unknown.join(", ")} (allowed: ${allowed.join(", ")})`,
          });
        } else if (new Set(value).size !== value.length) {
          problems.push({ id: control.id, message: "the same option is selected more than once" });
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
      out[control.id] = cloneValue(control, incoming);
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
