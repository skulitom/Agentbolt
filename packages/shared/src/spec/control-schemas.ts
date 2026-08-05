import { z } from "zod";

/**
 * The nine control schemas and the union over them.
 *
 * Shapes only. Everything a control DOES — how its default is validated, how
 * its value is checked, whether the preview can draw it, how it renders — lives
 * one file over in control-registry.ts, keyed by the same union. The split is
 * what makes the registry's mapped type work: this file has to be able to
 * declare `Control` before anything can be keyed by it.
 *
 * Two rules shape every decision here:
 *
 * 1. It must be easy for a language model to get right first time. That means a
 *    FLAT controls array (no nesting), short property names, `default` on every
 *    control, and a discriminated union so a mistake is reported against one
 *    variant instead of nine.
 * 2. When it does get it wrong, the error has to teach. Every object is strict,
 *    so `minimum: 0` instead of `min: 0` is caught by name rather than silently
 *    dropped.
 */

export const SLUG = /^[a-z][a-z0-9_]*$/;

export const idField = z
  .string()
  .regex(SLUG, "must be snake_case: lowercase letters, digits and underscores, starting with a letter");

const controlBase = {
  id: idField,
  label: z.string().min(1),
  description: z.string().optional(),
  /** Controls sharing a group render together, in first-appearance order. */
  group: z.string().optional(),
};

export const HEX_COLOR = /^#[0-9a-fA-F]{6}$/;

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

/** Narrows the union to one variant, so the registry's entries stay typed. */
export type ControlOfType<K extends ControlType> = Extract<Control, { type: K }>;

export type ControlValue = string | number | boolean | number[] | string[];
export type Values = Record<string, ControlValue>;
