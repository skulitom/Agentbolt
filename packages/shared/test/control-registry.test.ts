import { describe, expect, it } from "vitest";
import { CONTROLS, CONTROL_TYPES, behaviourFor } from "../src/spec/control-registry.js";
import { Control, type ControlType } from "../src/spec/control-schemas.js";
import { parseSpec, type InterfaceSpec } from "../src/spec.js";
import { validateValuesAgainstSpec, normalizeValues, defaultsFromSpec } from "../src/spec/values.js";
import { SELF_SHOWN } from "../src/vocab.js";

/**
 * The registry exists to make one class of mistake impossible: adding a control
 * type and forgetting one of the ~15 places that had to learn about it. Two of
 * those places used to fail SILENTLY —
 *
 *   - `validateValuesAgainstSpec` was a switch with no default, so a missing arm
 *     meant values were accepted UNVALIDATED from both the agent and the browser
 *   - the renderer's switch had no declared return type, so a missing arm just
 *     rendered nothing
 *
 * The mapped `Registry` type catches a missing entry at compile time. These
 * tests catch what a type cannot: an entry that exists but does nothing, and a
 * schema variant added to the union with no registry entry behind it.
 */

/** One valid and one deliberately-wrong value for every control type. */
const FIXTURES: {
  [K in ControlType]: { control: Record<string, unknown>; valid: unknown; invalid: unknown; because: RegExp };
} = {
  slider: {
    control: { id: "c", label: "C", type: "slider", min: 0, max: 10, default: 5 },
    valid: 7,
    invalid: 99,
    because: /above max/,
  },
  number: {
    control: { id: "c", label: "C", type: "number", default: 3 },
    valid: 12,
    invalid: "twelve",
    because: /expected a number/,
  },
  select: {
    control: { id: "c", label: "C", type: "select", options: [{ value: "a" }, { value: "b" }], default: "a" },
    valid: "b",
    invalid: "sideways",
    because: /expected one of/,
  },
  toggle: {
    control: { id: "c", label: "C", type: "toggle", default: false },
    valid: true,
    invalid: "yes",
    because: /true or false/,
  },
  color: {
    control: { id: "c", label: "C", type: "color", default: "#ff0000" },
    valid: "#00ff00",
    invalid: "red",
    because: /hex colour/,
  },
  text: {
    control: { id: "c", label: "C", type: "text", default: "" },
    valid: "hello",
    invalid: 42,
    because: /expected a string/,
  },
  vector: {
    control: { id: "c", label: "C", type: "vector", size: 3, default: [0, 0, 0] },
    valid: [1, 2, 3],
    invalid: [1, 2],
    because: /expected 3 numbers/,
  },
  compare: {
    control: {
      id: "c",
      label: "C",
      type: "compare",
      axes: [
        { id: "files", label: "Files", kind: "text" },
        { id: "wire", label: "Wire", kind: "bool" },
      ],
      options: [
        { value: "poll", cells: { files: "2", wire: false } },
        { value: "push", cells: { files: "5", wire: true } },
      ],
      default: "poll",
    },
    valid: "push",
    invalid: "neither",
    because: /expected one of/,
  },
  multiselect: {
    control: {
      id: "c",
      label: "C",
      type: "multiselect",
      options: [{ value: "a" }, { value: "b" }],
      default: [],
    },
    valid: ["a", "b"],
    invalid: ["a", "z"],
    because: /not options of this control/,
  },
};

function specFor(type: ControlType): InterfaceSpec {
  const parsed = parseSpec({ title: "T", controls: [FIXTURES[type].control] });
  if (!parsed.ok) throw new Error(`${type} fixture does not parse:\n${parsed.message}`);
  return parsed.spec;
}

describe("the control registry is total over the union", () => {
  it("has an entry for every variant of the Control union", () => {
    // The mapped type catches "entry missing for a declared type". This catches
    // the other direction: a schema variant added to the union with no entry,
    // which TypeScript would report against the registry rather than by name.
    const declared = Control.options.map((option) => option.shape.type.value).sort();
    expect(Object.keys(CONTROLS).sort()).toEqual(declared);
  });

  it("covers every type in this file's fixture table", () => {
    // If this fails, someone added a control type without adding it below — and
    // every value-validation assertion in this file would silently skip it.
    expect(Object.keys(FIXTURES).sort()).toEqual([...CONTROL_TYPES].sort());
  });

  it("agrees with the preview origin about which types the shell already shows", () => {
    const fromRegistry = CONTROL_TYPES.filter((type) => CONTROLS[type].selfShown).sort();
    expect(fromRegistry).toEqual([...SELF_SHOWN].sort());
  });
});

/**
 * The table that closes silent failure #1. A control type with no working
 * checkValue accepts anything, from the agent and from the browser alike.
 */
describe.each(CONTROL_TYPES)("%s values are actually validated", (type) => {
  const { valid, invalid, because } = FIXTURES[type];

  it("accepts a good value", () => {
    expect(validateValuesAgainstSpec(specFor(type), { c: valid })).toEqual([]);
  });

  it("rejects a bad one, and says why", () => {
    const problems = validateValuesAgainstSpec(specFor(type), { c: invalid });
    expect(problems, `${type} accepted an invalid value — checkValue is not doing anything`).toHaveLength(1);
    expect(problems[0]?.message).toMatch(because);
  });

  it("normalizes a bad value back to the default rather than keeping it", () => {
    const spec = specFor(type);
    const normalized = normalizeValues(spec, { c: invalid } as never);
    expect(normalized["c"]).toEqual(defaultsFromSpec(spec)["c"]);
  });

  it("has a formatDefault that says something", () => {
    const control = specFor(type).controls[0]!;
    expect(behaviourFor(control).formatDefault(control)).toBeTypeOf("string");
  });
});

describe("array-valued types are copied, not shared", () => {
  it.each(["vector", "multiselect"] as const)("%s defaults survive a mutating caller", (type) => {
    const spec = specFor(type);
    const first = defaultsFromSpec(spec)["c"] as unknown[];
    first.push("mutated" as never);
    // Without clone(), the push above would have edited the spec's own default
    // and every later reader would see it.
    expect(defaultsFromSpec(spec)["c"]).not.toHaveLength(first.length);
  });
});

describe("a control type this build does not know", () => {
  it("has its value rejected rather than accepted unvalidated", () => {
    // Reachable only through data that bypassed the parser — a session written
    // by a newer build. Accepting the value is the exact silent failure the
    // registry exists to prevent, so the fallback refuses instead.
    const rogue = { type: "hologram", id: "c", label: "C" } as never;
    expect(behaviourFor(rogue).checkValue(rogue, "anything")).toMatch(/not a control type this build understands/);
  });
});

/**
 * The CLAUDE.md footgun, now closed by construction.
 *
 * `FieldShorthand.type` was a hand-copied enum, and a control type missing from
 * it was invisible to `ui_ask`: the enum rejected the whole call before
 * inflateField ever saw the field, with an error naming the type rather than
 * the enum. It is derived from CONTROL_TYPES now, so this test documents the
 * property rather than defending it.
 */
describe("ui_ask can reach every control type", () => {
  it("accepts exactly the types the registry declares", async () => {
    const { FieldShorthand } = await import("../src/presets.js");
    const accepted = FieldShorthand.shape.type.unwrap().options as string[];
    expect([...accepted].sort()).toEqual([...CONTROL_TYPES].sort());
  });

  it("inflates a bare shorthand of every type into something that parses", async () => {
    const { AskInput, compileAskChecked } = await import("../src/presets.js");
    for (const type of CONTROL_TYPES) {
      // compare and multiselect genuinely need their own structure; the point
      // here is that the SHORTHAND path reaches each type at all.
      const extras: Record<string, unknown> =
        type === "compare"
          ? {
              axes: [
                { id: "a", label: "A", kind: "text" },
                { id: "b", label: "B", kind: "text" },
              ],
              options: [
                { value: "one", cells: { a: "1", b: "1" } },
                { value: "two", cells: { a: "2", b: "2" } },
              ],
            }
          : type === "multiselect" || type === "select"
            ? { options: [{ value: "one" }, { value: "two" }] }
            : {};

      const parsed = AskInput.safeParse({ question: "?", fields: [{ id: "f", type, ...extras }] });
      expect(parsed.success, `${type} was rejected by AskInput`).toBe(true);
      if (!parsed.success) continue;
      const result = compileAskChecked(parsed.data);
      expect(result.ok, `${type}: ${result.ok ? "" : result.message}`).toBe(true);
    }
  });
});
