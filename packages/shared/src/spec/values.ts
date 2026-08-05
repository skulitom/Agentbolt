import { behaviourFor } from "./control-registry.js";
import type { Control, ControlValue, Values } from "./control-schemas.js";
import type { InterfaceSpec } from "./interface.js";

/**
 * Runtime values: defaults, validation, normalization, and the grouping the
 * renderer lays out from.
 *
 * Everything type-specific here dispatches through the control registry. The
 * switch this file used to hold had no `default` and no exhaustiveness assert,
 * so a control type nobody added an arm for was accepted UNVALIDATED — from the
 * agent and from the browser alike.
 */

function cloneValue(control: Control, value: ControlValue): ControlValue {
  const clone = behaviourFor(control).clone;
  return clone ? clone(value) : value;
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
    const message = behaviourFor(control).checkValue(control, record[control.id]);
    if (message) problems.push({ id: control.id, message });
  }
  return problems;
}

/** Coerces values to be safe to render: fills missing, drops unknown, clamps. */
export function normalizeValues(spec: InterfaceSpec, values: Values): Values {
  const defaults = defaultsFromSpec(spec);
  const out: Values = { ...defaults };
  for (const control of spec.controls) {
    const incoming = values[control.id];
    if (incoming === undefined) continue;
    if (behaviourFor(control).checkValue(control, incoming) === undefined) {
      out[control.id] = cloneValue(control, incoming);
    }
  }
  return out;
}
