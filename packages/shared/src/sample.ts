import type { ContentBlock, Control, ControlValue, Values } from "./spec.js";

/**
 * The `sample` block: one comparison, replayed against rows the reader already
 * recognises.
 *
 * This is the only primitive that structurally cannot lie about scale, because
 * it has no scale. A bar for "6 hours" encodes nothing; five session names with
 * a line drawn between the two that would be marked stale and the three that
 * would not is a verdict the reader can disagree with. That is the whole idea.
 *
 * Everything here is pure and runs against LOCAL values, so it repaints at 60fps
 * during a drag without a single extra byte on the wire and with no path to
 * waking a waiting agent.
 */

export type SampleBlock = Extract<ContentBlock, { kind: "sample" }>;
export type SampleRow = SampleBlock["rows"][number];

export type SampleOp = ">" | ">=" | "<" | "<=" | "==" | "!=";

/**
 * `<row field> <op> <control id>` — one regex, no parser, no precedence table.
 *
 * The asymmetry is the design, not a simplification: the left side is ALWAYS a
 * row field and the right side is ALWAYS a control id. There is no merged scope,
 * so a row field cannot silently shadow a control of the same name and compare
 * the wrong two things with no error — which is exactly what an expression
 * language would have to define, and get wrong invisibly.
 */
const RULE = /^([a-z][a-z0-9_]*)\s*(>=|<=|==|!=|>|<)\s*([a-z][a-z0-9_]*)$/;

export const RULE_SHAPE =
  'a rule is exactly "<row_field> <op> <control_id>", e.g. "idle_h > stale_hours" (ops: > >= < <= == !=)';

export interface ParsedRule {
  /** A field present on every row. */
  field: string;
  op: SampleOp;
  /** A declared control id. */
  control: string;
  /** True for `> >= < <=`: numbers on both sides, rows sortable, line drawable. */
  ordering: boolean;
}

export function parseSampleRule(rule: string): ParsedRule | undefined {
  const match = RULE.exec(rule.trim());
  if (!match) return undefined;
  const op = match[2] as SampleOp;
  return { field: match[1]!, op, control: match[3]!, ordering: op !== "==" && op !== "!=" };
}

/** Display form of a cell. Rounded, because float artefacts read as precision. */
export function sampleText(value: unknown): string {
  if (value === undefined || value === null) return "";
  if (typeof value === "number") {
    if (!Number.isFinite(value)) return String(value);
    return Number.isInteger(value) ? String(value) : String(Math.round(value * 10000) / 10000);
  }
  if (typeof value === "boolean") return value ? "true" : "false";
  return String(value);
}

export interface EvaluatedRow {
  label: string;
  note: string | undefined;
  /** Whether the rule holds for this row at the current control value. */
  matched: boolean;
  /** The display field's value, formatted, with no unit attached. */
  text: string;
  /** 0..1 along the axis. undefined when there is no honest axis to plot on. */
  fraction: number | undefined;
  /** Set when the value runs off the end of the axis, so the bar can say so. */
  beyond: "low" | "high" | undefined;
}

/**
 * The line the bars are drawn against.
 *
 * Chosen to be the RANGE OF ANSWERS AVAILABLE TO YOU — the slider's own
 * min/max — rather than the spread of the data, whenever the rule compares the
 * field being shown. That is what makes the picture answer the question: a row
 * past the end of the axis is one your choice cannot affect either way, and
 * seeing three rows bunched at 5-7h with nothing until 27h tells you instantly
 * that anywhere in that gap is the same decision.
 *
 * Falls back to the data's own range when there is no declared one, and to no
 * axis at all when the values are not numbers — a bar you cannot trust is worse
 * than a column of numbers you can.
 */
export interface SampleAxis {
  min: number;
  max: number;
  minText: string;
  maxText: string;
  /** 0..1 position of the live control value, when it is on this dimension. */
  threshold: number | undefined;
  /** True when the axis came from the control's declared bounds. */
  fromControl: boolean;
}

export interface EvaluatedSample {
  op: SampleOp;
  ordering: boolean;
  controlId: string;
  /** The live control value, formatted the same way as the rows. */
  controlText: string;
  /** The field shown in the value column: `field`, or the rule's field. */
  field: string;
  rows: EvaluatedRow[];
  matchCount: number;
  axis: SampleAxis | undefined;
  /** Live sum of `total.field` over matching rows. */
  total: { label: string; text: string; unit: string | undefined } | undefined;
}

/**
 * Applies the rule to every row at the current values.
 *
 * Returns undefined only for a rule that does not parse, which the spec parser
 * already rejects — the renderer treats it as "draw nothing" rather than
 * throwing inside React.
 */
export function evaluateSample(
  block: SampleBlock,
  controls: ReadonlyArray<Control>,
  values: Values,
): EvaluatedSample | undefined {
  const rule = parseSampleRule(block.rule);
  if (!rule) return undefined;

  const control = controls.find((c) => c.id === rule.control);
  const controlValue = values[rule.control];
  const field = block.field ?? rule.field;

  const matches = (row: SampleRow): boolean => {
    const left = (row as Record<string, unknown>)[rule.field];
    if (rule.ordering) {
      const a = Number(left);
      const b = Number(controlValue);
      if (!Number.isFinite(a) || !Number.isFinite(b)) return false;
      switch (rule.op) {
        case ">":
          return a > b;
        case ">=":
          return a >= b;
        case "<":
          return a < b;
        default:
          return a <= b;
      }
    }
    // Strings, so a select's value can be matched against a row's classification
    // without either side having to know the other's type.
    const equal = sampleText(left) === sampleText(controlValue);
    return rule.op === "==" ? equal : !equal;
  };

  // Ordering rules sort; equality rules keep the agent's order, which is usually
  // the order of the thing being looked up (0,1,2,3 retries).
  const ordered = block.rows.map((row, index) => ({ row, index }));
  if (rule.ordering) {
    ordered.sort((a, b) => {
      const x = Number((a.row as Record<string, unknown>)[field]);
      const y = Number((b.row as Record<string, unknown>)[field]);
      if (Number.isFinite(x) && Number.isFinite(y) && x !== y) return x - y;
      return a.index - b.index;
    });
  }

  const numeric = (row: SampleRow): number => Number((row as Record<string, unknown>)[field]);
  // The axis only carries the threshold when the rule compares the same thing
  // the bars measure. With `field` pointed elsewhere the bars are still real,
  // but a line across them would compare two different dimensions.
  const sameDimension = rule.ordering && field === rule.field;
  const axis = axisFor(ordered.map(({ row }) => numeric(row)), sameDimension ? control : undefined, controlValue);

  const rows: EvaluatedRow[] = ordered.map(({ row }) => {
    const value = numeric(row);
    return {
      label: row.label,
      note: row.note,
      matched: matches(row),
      text: sampleText((row as Record<string, unknown>)[field]),
      fraction: axis ? clamp((value - axis.min) / (axis.max - axis.min)) : undefined,
      beyond: !axis || !Number.isFinite(value) ? undefined : value > axis.max ? "high" : value < axis.min ? "low" : undefined,
    };
  });

  const matchCount = rows.filter((r) => r.matched).length;

  let total: EvaluatedSample["total"];
  if (block.total) {
    const sum = ordered.reduce((acc, { row }, i) => {
      if (!rows[i]!.matched) return acc;
      const n = Number((row as Record<string, unknown>)[block.total!.field]);
      return Number.isFinite(n) ? acc + n : acc;
    }, 0);
    total = {
      label: block.total.label ?? block.total.field,
      text: sampleText(sum),
      unit: block.total.unit,
    };
  }

  return {
    op: rule.op,
    ordering: rule.ordering,
    controlId: rule.control,
    controlText: sampleText(controlValue),
    field,
    rows,
    matchCount,
    axis,
    total,
  };
}

const clamp = (n: number): number => (Number.isFinite(n) ? Math.max(0, Math.min(1, n)) : 0);

/**
 * Picks the axis, preferring the control's declared range over the data's.
 *
 * This is the same lesson as the ruler: scaling by the observed maximum makes
 * the largest row 100% wide by construction, which looks like information and
 * is not. The slider's own bounds are a real declaration, and they are also the
 * more USEFUL frame — they show the answer space, so a value past the end reads
 * as "your choice cannot change this one".
 */
function axisFor(
  values: number[],
  control: Control | undefined,
  controlValue: ControlValue | undefined,
): SampleAxis | undefined {
  const finite = values.filter((n) => Number.isFinite(n));
  if (finite.length < 2) return undefined;

  let min: number;
  let max: number;
  let fromControl = false;
  if (
    control &&
    (control.type === "slider" || control.type === "number") &&
    typeof control.min === "number" &&
    typeof control.max === "number" &&
    control.max > control.min
  ) {
    ({ min, max } = control);
    fromControl = true;
  } else {
    min = Math.min(0, ...finite);
    max = Math.max(...finite);
  }
  if (!(max > min)) return undefined;

  const live = Number(controlValue);
  return {
    min,
    max,
    minText: sampleText(min),
    maxText: sampleText(max),
    threshold: control && Number.isFinite(live) ? clamp((live - min) / (max - min)) : undefined,
    fromControl,
  };
}
