import { describe, expect, it } from "vitest";
import { controlHasVisual, shouldAutoPreview } from "../src/auto-preview.js";
import { groupControls, parseSpec, type InterfaceSpec } from "../src/spec.js";

function build(controls: unknown[]): InterfaceSpec {
  const parsed = parseSpec({ title: "T", controls });
  if (!parsed.ok) throw new Error(parsed.message);
  return parsed.spec;
}

/**
 * The rule: anything with a MAGNITUDE earns a preview, because you cannot judge
 * "760px" or "#b0367a" from the number. Self-describing controls do not, because
 * forcing them into a picture adds chrome that says nothing.
 */
describe("shouldAutoPreview", () => {
  it("previews colours", () => {
    expect(shouldAutoPreview(build([{ id: "tint", label: "Tint", type: "color", default: "#ef9f27" }]))).toBe(true);
  });

  it("previews anything carrying a unit", () => {
    expect(
      shouldAutoPreview(build([{ id: "delay", label: "Delay", type: "slider", min: 0, max: 10, default: 1, unit: "ms" }])),
    ).toBe(true);
  });

  it("previews dimension-like names even without a unit", () => {
    for (const id of ["content_width", "corner_radius", "gap", "padding", "offset", "duration"]) {
      expect(shouldAutoPreview(build([{ id, label: id, type: "slider", min: 0, max: 10, default: 1 }]))).toBe(true);
    }
  });

  it("does NOT preview self-describing controls", () => {
    expect(
      shouldAutoPreview(
        build([
          { id: "verbose", label: "Verbose", type: "toggle", default: false },
          { id: "mode", label: "Mode", type: "select", options: [{ value: "a" }, { value: "b" }], default: "a" },
          { id: "notes", label: "Notes", type: "text", default: "" },
        ]),
      ),
    ).toBe(false);
  });

  it("does NOT preview an abstract number with no unit or dimensional meaning", () => {
    expect(
      shouldAutoPreview(build([{ id: "retries", label: "Retries", type: "number", default: 3 }])),
    ).toBe(false);
  });

  it("previews the whole widget if any single control qualifies", () => {
    expect(
      shouldAutoPreview(
        build([
          { id: "verbose", label: "Verbose", type: "toggle", default: false },
          { id: "tint", label: "Tint", type: "color", default: "#000000" },
        ]),
      ),
    ).toBe(true);
  });
});

describe("groupControls visualFirst", () => {
  it("sinks groups the preview cannot illustrate below ones it can", () => {
    const spec = build([
      { id: "verbose", label: "Verbose", type: "toggle", default: false, group: "Behaviour" },
      { id: "tint", label: "Tint", type: "color", default: "#000000", group: "Colour" },
      { id: "gap", label: "Gap", type: "slider", min: 0, max: 20, default: 4, unit: "px", group: "Layout" },
    ]);
    const ordered = groupControls(spec, { visualFirst: true, isVisual: controlHasVisual });
    expect(ordered.map((g) => g.group)).toEqual(["Colour", "Layout", "Behaviour"]);
  });

  it("keeps authored order when there is no preview to order around", () => {
    const spec = build([
      { id: "verbose", label: "Verbose", type: "toggle", default: false, group: "Behaviour" },
      { id: "tint", label: "Tint", type: "color", default: "#000000", group: "Colour" },
    ]);
    expect(groupControls(spec).map((g) => g.group)).toEqual(["Behaviour", "Colour"]);
  });

  it("is stable within each half", () => {
    const spec = build([
      { id: "a_flag", label: "A", type: "toggle", default: false, group: "First non-visual" },
      { id: "b_flag", label: "B", type: "toggle", default: false, group: "Second non-visual" },
      { id: "tint", label: "Tint", type: "color", default: "#000000", group: "Visual" },
    ]);
    const ordered = groupControls(spec, { visualFirst: true, isVisual: controlHasVisual });
    expect(ordered.map((g) => g.group)).toEqual(["Visual", "First non-visual", "Second non-visual"]);
  });
});
