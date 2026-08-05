import { describe, expect, it } from "vitest";
import { evaluateSample, parseSampleRule, type SampleBlock } from "../src/sample.js";
import { parseSpec, type Control, type Values } from "../src/spec.js";

/**
 * A `sample` is the one primitive that cannot lie about scale — so these tests
 * pin the two things that make that true: the rule grammar is a single regex
 * with no scope to shadow, and the axis is the range of answers the control can
 * actually produce rather than whatever spread the agent happened to paste.
 */

interface Fixture {
  block: SampleBlock;
  controls: Control[];
}

function fixture(over: Record<string, unknown> = {}, controls: unknown[] = []): Fixture {
  const parsed = parseSpec({
    title: "T",
    content: [
      {
        kind: "sample",
        label: "Sessions",
        source: "read just now",
        rule: "idle_h > stale_hours",
        rows: [
          { label: "muzzle-flash", idle_h: 0.33 },
          { label: "theme-preview", idle_h: 5.2 },
          { label: "shader params", idle_h: 6.8 },
          { label: "changelog approval", idle_h: 27 },
          { label: "three.js demo", idle_h: 190 },
        ],
        ...over,
      },
    ],
    controls: controls.length
      ? controls
      : [{ id: "stale_hours", label: "Stale after", type: "slider", min: 1, max: 48, default: 4, unit: "h" }],
  });
  if (!parsed.ok) throw new Error(parsed.message);
  return { block: parsed.spec.content[0] as SampleBlock, controls: parsed.spec.controls };
}

const run = (f: Fixture, values: Values) => evaluateSample(f.block, f.controls, values)!;

describe("parseSampleRule", () => {
  it("reads <row field> <op> <control id>", () => {
    expect(parseSampleRule("idle_h > stale_hours")).toEqual({
      field: "idle_h",
      op: ">",
      control: "stale_hours",
      ordering: true,
    });
  });

  it("tolerates missing spaces and marks equality as non-ordering", () => {
    expect(parseSampleRule("needs!=scope")).toMatchObject({ field: "needs", op: "!=", control: "scope", ordering: false });
  });

  it("prefers the two-character operators", () => {
    expect(parseSampleRule("a >= b")?.op).toBe(">=");
    expect(parseSampleRule("a <= b")?.op).toBe("<=");
  });

  it("rejects anything that is not exactly one comparison", () => {
    // No literals, no arithmetic, no dotted paths, no second clause: every one
    // of these is the beginning of an expression language, which is the thing
    // this grammar exists to refuse.
    for (const rule of ["idle_h > 5", "a.b > c", "a + b > c", "a > b > c", "Idle > x", "> x", "a >"]) {
      expect(parseSampleRule(rule), rule).toBeUndefined();
    }
  });
});

describe("evaluateSample — ordering rules", () => {
  it("sorts, marks and counts against the live control value", () => {
    const result = run(fixture(), { stale_hours: 6 });
    expect(result.rows.map((r) => r.label)).toEqual([
      "muzzle-flash",
      "theme-preview",
      "shader params",
      "changelog approval",
      "three.js demo",
    ]);
    expect(result.rows.map((r) => r.matched)).toEqual([false, false, true, true, true]);
    expect(result.matchCount).toBe(3);
  });

  it("moves the cut as the control moves — the whole point of the block", () => {
    // 1..48 is the slider's own range, so 6h sits a touch above a tenth along.
    expect(run(fixture(), { stale_hours: 6 }).axis?.threshold).toBeCloseTo((6 - 1) / 47, 5);
    expect(run(fixture(), { stale_hours: 24 }).axis?.threshold).toBeCloseTo((24 - 1) / 47, 5);
  });

  it("plots against the ANSWER SPACE, not the spread of the data", () => {
    // Scaling to the data would make the 190h row 100% wide by construction and
    // squash the three rows the decision is actually about into nothing.
    const axis = run(fixture(), { stale_hours: 6 }).axis!;
    expect(axis).toMatchObject({ min: 1, max: 48, fromControl: true });
  });

  it("pins rows the slider cannot reach, at either end", () => {
    const rows = run(fixture(), { stale_hours: 6 }).rows;
    expect(rows[0]).toMatchObject({ label: "muzzle-flash", beyond: "low", fraction: 0 });
    expect(rows[4]).toMatchObject({ label: "three.js demo", beyond: "high", fraction: 1 });
    expect(rows[2]).toMatchObject({ label: "shader params", beyond: undefined });
    expect(rows[2]!.fraction).toBeCloseTo((6.8 - 1) / 47, 5);
  });

  it("falls back to the data's range when the control declares none", () => {
    const axis = run(
      fixture({ rule: "idle_h > cutoff" }, [{ id: "cutoff", label: "Cutoff", type: "number", default: 4 }]),
      { cutoff: 4 },
    ).axis!;
    expect(axis).toMatchObject({ min: 0, max: 190, fromControl: false });
  });

  it("draws no cut when the bars measure something the rule does not compare", () => {
    // `field` sorting by one thing while the rule compares another means a line
    // across the bars would put two different dimensions on one axis.
    const scattered = fixture(
      {
        rule: "size > limit",
        field: "age",
        rows: [
          { label: "a", size: 10, age: 1 },
          { label: "b", size: 1, age: 2 },
          { label: "c", size: 10, age: 3 },
        ],
      },
      [{ id: "limit", label: "Limit", type: "slider", min: 0, max: 20, default: 5 }],
    );
    const result = run(scattered, { limit: 5 });
    expect(result.matchCount).toBe(2);
    expect(result.axis?.threshold).toBeUndefined();
    // The bars are still honest — they just measure `age`, on age's own range.
    expect(result.axis).toMatchObject({ min: 0, max: 3 });
  });

  it("sums the total over matching rows only", () => {
    const withTotal = fixture(
      {
        rule: "age_d > keep_days",
        field: "age_d",
        total: { field: "kb", unit: " kB", label: "reclaimed" },
        rows: [
          { label: "muzzle-flash", age_d: 0.01, kb: 96 },
          { label: "three.js demo", age_d: 8, kb: 4180 },
          { label: "usability probe", age_d: 23, kb: 72 },
        ],
      },
      [{ id: "keep_days", label: "Keep for", type: "slider", min: 0, max: 30, default: 7, unit: "d" }],
    );
    expect(run(withTotal, { keep_days: 7 }).total).toEqual({ label: "reclaimed", text: "4252", unit: " kB" });
    expect(run(withTotal, { keep_days: 30 }).total?.text).toBe("0");
  });
});

describe("evaluateSample — equality rules", () => {
  const lookup = () =>
    fixture(
      {
        rule: "n == retries",
        field: "stall_s",
        unit: "s",
        rows: [
          { label: "no retry", n: 0, stall_s: 0 },
          { label: "3 retries", n: 3, stall_s: 3.5 },
          { label: "7 retries", n: 7, stall_s: 63.5 },
        ],
      },
      [{ id: "retries", label: "Retries", type: "slider", min: 0, max: 7, step: 1, default: 3 }],
    );

  it("keeps the agent's row order and lights the matching row", () => {
    const result = run(lookup(), { retries: 3 });
    expect(result.ordering).toBe(false);
    expect(result.rows.map((r) => r.label)).toEqual(["no retry", "3 retries", "7 retries"]);
    expect(result.rows.map((r) => r.matched)).toEqual([false, true, false]);
    expect(result.rows[1]?.text).toBe("3.5");
  });

  it("still plots the consequence, so the cost of each choice is a shape", () => {
    // A lookup has no threshold to draw, but "7 retries stalls 18x longer than
    // 3" is the entire point of the table and is invisible as three numbers.
    const result = run(lookup(), { retries: 3 });
    expect(result.axis).toMatchObject({ min: 0, max: 63.5 });
    expect(result.axis?.threshold).toBeUndefined();
    expect(result.rows[1]?.fraction).toBeCloseTo(3.5 / 63.5, 5);
    expect(result.rows[2]?.fraction).toBe(1);
  });

  it("compares as strings, so a select value can match a row's classification", () => {
    const scopes = fixture(
      {
        rule: "needs != scope",
        verdict: ["would have been rebuilt", "found on reuse"],
        rows: [
          { label: "muzzle-flash", needs: "global" },
          { label: "theme-probe", needs: "project" },
        ],
      },
      [
        {
          id: "scope",
          label: "Scope",
          type: "select",
          options: [{ value: "global" }, { value: "project" }],
          default: "global",
        },
      ],
    );
    expect(run(scopes, { scope: "global" }).rows.map((r) => r.matched)).toEqual([false, true]);
    expect(run(scopes, { scope: "project" }).matchCount).toBe(1);
  });

  it("draws no bars at all when the values are not numbers", () => {
    // A bar you cannot trust is worse than a column of words you can.
    const scopes = fixture(
      { rule: "needs != scope", rows: [{ label: "a", needs: "global" }, { label: "b", needs: "project" }] },
      [{ id: "scope", label: "Scope", type: "select", options: [{ value: "global" }, { value: "project" }], default: "global" }],
    );
    const result = run(scopes, { scope: "global" });
    expect(result.axis).toBeUndefined();
    expect(result.rows.every((r) => r.fraction === undefined)).toBe(true);
  });
});

describe("evaluateSample — degrading honestly", () => {
  it("matches nothing rather than guessing when the control value is missing", () => {
    expect(run(fixture(), {}).matchCount).toBe(0);
  });

  it("rounds float artefacts out of the readout", () => {
    const drifty = fixture({
      rows: [
        { label: "a", idle_h: 0.1 + 0.2 },
        { label: "b", idle_h: 9 },
      ],
    });
    expect(run(drifty, { stale_hours: 1 }).rows[0]?.text).toBe("0.3");
  });
});
