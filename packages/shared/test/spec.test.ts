import { describe, expect, it } from "vitest";
import {
  defaultsFromSpec,
  groupControls,
  normalizeValues,
  parseSpec,
  validateValuesAgainstSpec,
  type InterfaceSpec,
} from "../src/spec.js";

function ok(input: unknown): InterfaceSpec {
  const result = parseSpec(input);
  if (!result.ok) throw new Error(`expected spec to parse, got:\n${result.message}`);
  return result.spec;
}

function err(input: unknown): string {
  const result = parseSpec(input);
  if (result.ok) throw new Error("expected spec to fail validation");
  return result.message;
}

describe("InterfaceSpec", () => {
  it("fills defaults so an agent can write the minimum", () => {
    const spec = ok({
      title: "Tuning",
      controls: [{ id: "gain", label: "Gain", type: "slider", min: 0, max: 2, default: 1 }],
    });
    expect(spec.schemaVersion).toBe(1);
    expect(spec.controls[0]).toMatchObject({ step: 0.01 });
    expect(spec.content).toEqual([]);
    expect(spec.comment.enabled).toBe(true);
  });

  it("extracts defaults, copying vectors so callers cannot alias the spec", () => {
    const spec = ok({
      title: "T",
      controls: [
        { id: "gain", label: "Gain", type: "slider", min: 0, max: 2, default: 1 },
        { id: "offset", label: "Offset", type: "vector", size: 3, default: [1, 2, 3] },
        { id: "on", label: "On", type: "toggle", default: true },
      ],
    });
    const values = defaultsFromSpec(spec);
    expect(values).toEqual({ gain: 1, offset: [1, 2, 3], on: true });

    (values["offset"] as number[])[0] = 99;
    expect(defaultsFromSpec(spec)["offset"]).toEqual([1, 2, 3]);
  });

  it("buckets controls by group in first-appearance order", () => {
    const spec = ok({
      title: "T",
      controls: [
        { id: "a", label: "A", type: "toggle", default: false, group: "Motion" },
        { id: "b", label: "B", type: "toggle", default: false },
        { id: "c", label: "C", type: "toggle", default: false, group: "Motion" },
        { id: "d", label: "D", type: "toggle", default: false, group: "Colour" },
      ],
    });
    expect(groupControls(spec).map((g) => [g.group, g.controls.map((c) => c.id)])).toEqual([
      ["Motion", ["a", "c"]],
      ["", ["b"]],
      ["Colour", ["d"]],
    ]);
  });
});

describe("spec error messages", () => {
  it("reports the offending path, not just the union", () => {
    const message = err({
      title: "T",
      controls: [
        { id: "a", label: "A", type: "toggle", default: false },
        { id: "b", label: "B", type: "slider", min: 0, max: 1, default: "nope" },
      ],
    });
    expect(message).toContain("controls[1]");
    expect(message).toMatch(/number/);
  });

  it("catches a misspelled property instead of silently dropping it", () => {
    const message = err({
      title: "T",
      controls: [{ id: "a", label: "A", type: "slider", minimum: 0, max: 1, default: 0.5 }],
    });
    expect(message).toMatch(/minimum/);
  });

  it("rejects non-snake_case ids with an explanation", () => {
    expect(err({ title: "T", controls: [{ id: "Muzzle-Flash", label: "x", type: "toggle", default: false }] })).toMatch(
      /snake_case/,
    );
  });

  it("catches duplicate control ids and names the earlier one", () => {
    const message = err({
      title: "T",
      controls: [
        { id: "gain", label: "A", type: "toggle", default: false },
        { id: "gain", label: "B", type: "toggle", default: false },
      ],
    });
    expect(message).toContain("duplicate control id");
    expect(message).toContain("controls[0]");
  });

  it("catches a default outside its own range", () => {
    expect(
      err({ title: "T", controls: [{ id: "g", label: "G", type: "slider", min: 0, max: 1, default: 5 }] }),
    ).toMatch(/above max/);
  });

  it("catches min >= max", () => {
    expect(
      err({ title: "T", controls: [{ id: "g", label: "G", type: "slider", min: 1, max: 0, default: 0.5 }] }),
    ).toMatch(/must be less than max/);
  });

  it("catches a select default that is not an option", () => {
    expect(
      err({
        title: "T",
        controls: [{ id: "m", label: "M", type: "select", options: [{ value: "a" }, { value: "b" }], default: "c" }],
      }),
    ).toMatch(/not one of the options/);
  });

  it("catches a vector default that does not match its size", () => {
    expect(
      err({ title: "T", controls: [{ id: "v", label: "V", type: "vector", size: 3, default: [1, 2] }] }),
    ).toMatch(/expected 3 numbers/);
  });

  it("rejects an empty widget", () => {
    expect(err({ title: "T" })).toMatch(/at least one of/);
  });

  it("requires an image block to carry a source", () => {
    expect(err({ title: "T", content: [{ kind: "image", alt: "x" }] })).toMatch(/path.*dataUri|dataUri/);
  });

  it("always points at the docs", () => {
    expect(err({ title: "T" })).toMatch(/ui_library|skill/);
  });
});

describe("validateValuesAgainstSpec", () => {
  const spec = ok({
    title: "T",
    controls: [
      { id: "gain", label: "Gain", type: "slider", min: 0, max: 2, default: 1 },
      { id: "mode", label: "Mode", type: "select", options: [{ value: "fast" }, { value: "slow" }], default: "fast" },
      { id: "tint", label: "Tint", type: "color", default: "#ff0000" },
      { id: "size", label: "Size", type: "vector", size: 2, default: [0, 0] },
      { id: "on", label: "On", type: "toggle", default: true },
    ],
  });

  it("accepts good values", () => {
    expect(
      validateValuesAgainstSpec(spec, { gain: 1.5, mode: "slow", tint: "#00ff00", size: [1, 2], on: false }),
    ).toEqual([]);
  });

  it("rejects out-of-range, wrong-type and unknown keys", () => {
    const problems = validateValuesAgainstSpec(spec, { gain: 9, mode: "sideways", tint: "red", nope: 1 });
    const ids = problems.map((p) => p.id).sort();
    expect(ids).toEqual(["gain", "mode", "nope", "tint"]);
    expect(problems.find((p) => p.id === "gain")?.message).toMatch(/above max/);
    expect(problems.find((p) => p.id === "nope")?.message).toMatch(/no control/);
  });

  it("rejects a vector of the wrong arity", () => {
    expect(validateValuesAgainstSpec(spec, { size: [1, 2, 3] })).toHaveLength(1);
  });

  it("normalizes by filling gaps and discarding junk rather than throwing", () => {
    const normalized = normalizeValues(spec, { gain: 99, mode: "slow" } as never);
    expect(normalized["gain"]).toBe(1); // out of range -> falls back to the default
    expect(normalized["mode"]).toBe("slow");
    expect(normalized["tint"]).toBe("#ff0000");
  });
});
