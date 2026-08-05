import type { Ctx } from "./errors.js";
import { describeValue } from "./errors.js";
import { HEX_COLOR, type Control, type ControlOfType, type ControlType, type ControlValue } from "./control-schemas.js";
import { isEasingSelect, isFontish, isSizeish } from "../vocab.js";

/**
 * Everything a control type DOES, in one entry per type.
 *
 * Before this table, adding one control type meant finding and editing about
 * fifteen sites across eight files: the schema, the union, a superRefine arm, a
 * cloneValue arm, a validateValuesAgainstSpec arm, a checkBoundBlock reject
 * list, a FieldShorthand enum, two inflateField arms, two auto-preview
 * predicates, a renderer switch arm, an isGroup list, a formatDefault arm, a
 * SELF_SHOWN set on the other origin, and some CSS.
 *
 * Two of those sites failed SILENTLY. `validateValuesAgainstSpec` was a switch
 * with no default and no exhaustiveness assert, so a missing arm meant values
 * were accepted unvalidated from both the agent and the browser; the renderer's
 * switch had no declared return type, so a missing arm rendered nothing. The
 * compiler flagged neither.
 *
 * A mapped Record over the union fixes exactly that: `Registry` cannot be
 * constructed without an entry for every member of `Control`, and the error
 * names the missing type. Each entry is narrowed to its own variant, so
 * `multiselect`'s checkValue sees `control.options` with no cast.
 *
 * Adding a control type is now: a schema in control-schemas.ts, a member of the
 * `Control` union, an entry here, and an entry in the UI's CONTROL_INPUTS. The
 * compiler names the last two.
 */
export interface ControlBehaviour<C extends Control = Control> {
  /**
   * Structural checks the schema cannot express — cross-field rules like
   * "default is within min/max" or "every option answers every axis".
   * `at(...)` prefixes a path with this control's position.
   */
  refine?(control: C, at: (...rest: Array<string | number>) => Array<string | number>, ctx: Ctx): void;

  /**
   * Is this a legal runtime value for this control? Returns the problem, or
   * undefined when it is fine. Used for values pushed by an agent via
   * ui_update AND for values arriving from the browser — the server trusts
   * neither side.
   */
  checkValue(control: C, value: unknown): string | undefined;

  /**
   * Array-valued types must COPY, or a caller that mutates its values mutates
   * the spec's defaults with it. Omitted for scalar types.
   */
  clone?(value: ControlValue): ControlValue;

  /**
   * Does anything on screen change when this moves? Only orders the controls,
   * which is a weaker question than "can we draw it" — a control can be
   * illustrated by its own renderer without the helper knowing how to draw it.
   */
  illustrated?(control: C): boolean;

  /**
   * Why a bound `{{placeholder}}` may not name this control, if it may not.
   * Interpolating a list or a boolean into a sentence is never what was meant.
   *
   * Returns the clause that FOLLOWS "{{x}} is a <type>", so the composed
   * message reads as one sentence.
   */
  bindRejection?(control: C): string | undefined;

  /**
   * ui_ask shorthand: the default to supply when the author omitted one.
   * `draft` is the partially-inflated field, so bounds are visible.
   */
  askDefault(draft: Record<string, unknown>): unknown;

  /** ui_ask shorthand: fill in whatever else the type needs to be valid. */
  askFixup?(draft: Record<string, unknown>): void;

  /** Display form of `default`, for the "reset to suggested" affordance. */
  formatDefault(control: C): string;

  /**
   * True when the SHELL already illustrates this type, so the preview frame's
   * state line must not echo it. Mirrors SELF_SHOWN in vocab.js; a test asserts
   * the two agree.
   */
  selfShown: boolean;

  /**
   * True when the control renders as a GROUP of inputs rather than one, so its
   * label must not be a `<label for=…>` pointing at nothing.
   */
  groupLabelled?: boolean;
}

type Registry = { [K in ControlType]: ControlBehaviour<ControlOfType<K>> };

/* ------------------------------------------------------------ shared arms */

/** slider and number differ only in which bounds are required by the schema. */
function refineNumeric(
  control: ControlOfType<"slider"> | ControlOfType<"number">,
  at: (...rest: Array<string | number>) => Array<string | number>,
  ctx: Ctx,
): void {
  const { min, max } = control;
  if (min !== undefined && max !== undefined && min >= max) {
    ctx.addIssue({ code: "custom", path: at("min"), message: `min (${min}) must be less than max (${max})` });
  }
  if (min !== undefined && control.default < min) {
    ctx.addIssue({
      code: "custom",
      path: at("default"),
      message: `default ${control.default} is below min ${min}`,
    });
  }
  if (max !== undefined && control.default > max) {
    ctx.addIssue({
      code: "custom",
      path: at("default"),
      message: `default ${control.default} is above max ${max}`,
    });
  }
}

function checkNumericValue(
  control: ControlOfType<"slider"> | ControlOfType<"number">,
  value: unknown,
): string | undefined {
  if (typeof value !== "number" || Number.isNaN(value)) return `expected a number, got ${describeValue(value)}`;
  if (control.min !== undefined && value < control.min) return `${value} is below min ${control.min}`;
  if (control.max !== undefined && value > control.max) return `${value} is above max ${control.max}`;
  return undefined;
}

/**
 * A compare's value IS a select's value: one option value, a plain string.
 * That is the whole cost argument for the control, and it holds here.
 */
function checkChosenOption(
  control: ControlOfType<"select"> | ControlOfType<"compare">,
  value: unknown,
): string | undefined {
  const allowed = control.options.map((o) => o.value);
  if (typeof value !== "string" || !allowed.includes(value)) {
    return `expected one of ${allowed.join(", ")}, got ${describeValue(value)}`;
  }
  return undefined;
}

function labelFor(options: ReadonlyArray<{ value: string; label?: string | undefined }>, value: unknown): string {
  return options.find((o) => o.value === value)?.label ?? String(value);
}

const num = (value: unknown, fallback: number): number => (typeof value === "number" ? value : fallback);

/**
 * A number with a magnitude you can see: a length or a type size.
 *
 * Used for ORDERING only. Whether a preview frame opens, and what goes in it,
 * is `planPreview`'s single answer — this registry deliberately no longer has
 * an opinion on that, because two opinions is how a control named "type_scale"
 * ended up in the frame's font bucket while the gate said there was nothing to
 * show and the frame never opened.
 */
const drawableMagnitude = (control: ControlOfType<"slider"> | ControlOfType<"number">): boolean =>
  isSizeish(control) || isFontish(control);

/**
 * First option value of a draft field. Options have already been normalized to
 * `{ value }` objects by the time askDefault runs.
 */
function firstOptionValue(draft: Record<string, unknown>): string {
  const options = draft["options"];
  const first = Array.isArray(options) ? options[0] : undefined;
  if (first && typeof first === "object" && "value" in first) return String((first as { value: unknown }).value);
  return "";
}

/* -------------------------------------------------------------- registry */

export const CONTROLS: Registry = {
  slider: {
    refine: refineNumeric,
    checkValue: checkNumericValue,
    illustrated: drawableMagnitude,
    askDefault: (d) => (num(d["min"], 0) + num(d["max"], 1)) / 2,
    askFixup: (d) => {
      // A slider REQUIRES bounds; a shorthand field that named none still has
      // to produce a parseable control rather than a schema error.
      if (typeof d["min"] !== "number") d["min"] = 0;
      if (typeof d["max"] !== "number") d["max"] = 1;
    },
    formatDefault: (c) => String(c.default),
    selfShown: false,
  },

  number: {
    refine: refineNumeric,
    checkValue: checkNumericValue,
    illustrated: drawableMagnitude,
    askDefault: (d) => num(d["min"], 0),
    formatDefault: (c) => String(c.default),
    selfShown: false,
  },

  select: {
    refine: (control, at, ctx) => {
      const values = control.options.map((o) => o.value);
      if (!values.includes(control.default)) {
        ctx.addIssue({
          code: "custom",
          path: at("default"),
          message: `default "${control.default}" is not one of the options (${values.join(", ")})`,
        });
      }
    },
    checkValue: checkChosenOption,
    // An easing curve is a shape the helper CAN draw (easingCurve): the name
    // "easeOutBack" tells you nothing you can feel, the plotted curve does.
    illustrated: isEasingSelect,
    askDefault: (d) => firstOptionValue(d),
    formatDefault: (c) => labelFor(c.options, c.default),
    selfShown: false,
  },

  toggle: {
    checkValue: (_c, value) =>
      typeof value === "boolean" ? undefined : `expected true or false, got ${describeValue(value)}`,
    bindRejection: () =>
      ', and "true" is not what you want in the text — use a select whose option VALUES are the alternative lines, and bind that',
    askDefault: () => false,
    formatDefault: (c) => String(c.default),
    selfShown: false,
  },

  color: {
    checkValue: (_c, value) =>
      typeof value === "string" && HEX_COLOR.test(value)
        ? undefined
        : `expected a hex colour like #ef9f27, got ${describeValue(value)}`,
    illustrated: () => true,
    askDefault: () => "#888888",
    formatDefault: (c) => c.default,
    selfShown: false,
  },

  text: {
    checkValue: (_c, value) => (typeof value === "string" ? undefined : `expected a string, got ${describeValue(value)}`),
    askDefault: () => "",
    formatDefault: (c) => c.default,
    // The input shows its own text; a state line repeating it says nothing.
    selfShown: true,
  },

  vector: {
    refine: (control, at, ctx) => {
      if (control.default.length !== control.size) {
        ctx.addIssue({
          code: "custom",
          path: at("default"),
          message: `expected ${control.size} numbers to match size, got ${control.default.length}`,
        });
      }
      if (control.labels && control.labels.length !== control.size) {
        ctx.addIssue({
          code: "custom",
          path: at("labels"),
          message: `expected ${control.size} labels to match size, got ${control.labels.length}`,
        });
      }
    },
    checkValue: (control, value) => {
      if (!Array.isArray(value) || value.length !== control.size || value.some((n) => typeof n !== "number")) {
        return `expected ${control.size} numbers, got ${describeValue(value)}`;
      }
      return undefined;
    },
    clone: (value) => [...(value as number[])],
    illustrated: () => true,
    bindRejection: () => ", which is a list, not a string — show the selection with a `sample` block instead",
    // 2 unless 3 was asked for: a 2-vector is the common case (a position, a
    // size), and guessing 3 makes the author delete a component.
    askDefault: (d) => Array.from({ length: d["size"] === 3 ? 3 : 2 }, () => 0),
    askFixup: (d) => {
      // A vector inferred from `default: [x, y]` carries its arity in the
      // default, so derive `size` rather than making the author repeat it.
      if (d["size"] !== 2 && d["size"] !== 3) {
        const fromDefault = Array.isArray(d["default"]) ? d["default"].length : 0;
        d["size"] = fromDefault === 3 ? 3 : 2;
        if (!Array.isArray(d["default"])) d["default"] = [0, 0];
      }
    },
    formatDefault: (c) => `[${c.default.join(", ")}]`,
    selfShown: false,
  },

  compare: {
    refine: refineCompare,
    checkValue: checkChosenOption,
    // The matrix IS the illustration, and it is on the control itself.
    illustrated: () => true,
    askDefault: (d) => firstOptionValue(d),
    formatDefault: (c) => labelFor(c.options, c.default),
    selfShown: true,
    groupLabelled: true,
  },

  multiselect: {
    refine: refineMultiSelect,
    checkValue: (control, value) => {
      const allowed = control.options.map((o) => o.value);
      if (!Array.isArray(value) || value.some((v) => typeof v !== "string")) {
        return `expected an array of option values, got ${describeValue(value)}`;
      }
      const unknown = value.filter((v) => !allowed.includes(v as string));
      if (unknown.length) {
        return `not options of this control: ${unknown.join(", ")} (allowed: ${allowed.join(", ")})`;
      }
      if (new Set(value).size !== value.length) return "the same option is selected more than once";
      return undefined;
    },
    clone: (value) => [...(value as string[])],
    // Only when the weights are something MEASURED — otherwise the running
    // total is "2 of 3 selected", which the control already says.
    illustrated: (c) => c.options.some((o) => o.weight !== undefined),
    bindRejection: () => ", which is a list, not a string — show the selection with a `sample` block instead",
    askDefault: () => [],
    formatDefault: (c) =>
      c.default.length ? c.default.map((v) => labelFor(c.options, v)).join(", ") : "nothing selected",
    selfShown: true,
    groupLabelled: true,
  },
};

/** Every declared control type, derived rather than restated. */
export const CONTROL_TYPES = Object.keys(CONTROLS) as ControlType[];

/**
 * The behaviour for a control whose type this build does not know.
 *
 * Reachable only through data that bypassed the parser — a session directory
 * written by a newer build, say. Rejecting the VALUE is the safe answer:
 * accepting it unvalidated is the exact silent failure the registry exists to
 * make impossible.
 */
const UNKNOWN_CONTROL: ControlBehaviour = {
  checkValue: (control) => `"${control.type}" is not a control type this build understands`,
  askDefault: () => "",
  formatDefault: () => "",
  selfShown: false,
};

/**
 * Widened lookup, for the many call sites holding a `Control` rather than one
 * narrowed variant. The cast is safe by construction — `CONTROLS` is total over
 * `ControlType` — and the fallback covers data that never went through the
 * parser at all.
 */
export function behaviourFor(control: Control): ControlBehaviour {
  return (CONTROLS as Record<string, ControlBehaviour | undefined>)[control.type] ?? UNKNOWN_CONTROL;
}

/* ------------------------------------------------- the two long validators */

/**
 * The forced-parallelism check. A missing cell is not a nicety: it is the exact
 * place where prose would have let the agent argue one side and go quiet on the
 * other, so it is reported per option, per axis, by name.
 */
function refineCompare(
  control: ControlOfType<"compare">,
  at: (...rest: Array<string | number>) => Array<string | number>,
  ctx: Ctx,
): void {
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
      ctx.addIssue({
        code: "custom",
        path: at("options", o, "value"),
        message: `duplicate option value "${option.value}"`,
      });
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

function refineMultiSelect(
  control: ControlOfType<"multiselect">,
  at: (...rest: Array<string | number>) => Array<string | number>,
  ctx: Ctx,
): void {
  const values = new Set<string>();
  control.options.forEach((option, o) => {
    if (values.has(option.value)) {
      ctx.addIssue({
        code: "custom",
        path: at("options", o, "value"),
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
        path: at("default", d),
        message: `"${value}" is not one of the options (${[...values].join(", ")})`,
      });
    }
    if (chosen.has(value)) {
      ctx.addIssue({ code: "custom", path: at("default", d), message: `"${value}" is selected twice` });
    }
    chosen.add(value);
  });
}
