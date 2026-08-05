import { describe, expect, it } from "vitest";
import {
  controlIsIllustrated,
  isEasingSelect,
  shouldAutoPreview,
} from "../src/auto-preview.js";
import { groupControls, parseSpec, type InterfaceSpec } from "../src/spec.js";

function build(controls: unknown[]): InterfaceSpec {
  return buildSpec({ title: "T", controls });
}

function buildSpec(input: Record<string, unknown>): InterfaceSpec {
  const parsed = parseSpec(input);
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

  it("previews spatial names even without a unit", () => {
    for (const id of ["content_width", "corner_radius", "gap", "padding", "offset"]) {
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

/**
 * Two questions that sound like one: "can the built-in helper DRAW this?" gates
 * the frame; "does anything on screen change when this moves?" only orders the
 * controls. Without the split, teaching the second one about compare and sample
 * would open a half-screen preview pane whose entire content is "Retries: 3".
 */
describe("the predicate split", () => {
  const compare = {
    id: "approach",
    label: "Approach",
    type: "compare",
    axes: [
      { id: "files", label: "Files", kind: "text" },
      { id: "wire", label: "Wire", kind: "bool", better: "low" },
    ],
    options: [
      { value: "poll", cells: { files: "2", wire: false } },
      { value: "push", cells: { files: "5", wire: true } },
    ],
    default: "poll",
  };

  it("gives the two callers opposite answers for a compare", () => {
    const spec = build([compare]);
    // The frame does not open for a compare, but the control still counts as
    // illustrated for ORDERING — the matrix is on the control itself.
    expect(shouldAutoPreview(spec)).toBe(false);
    expect(controlIsIllustrated(spec, spec.controls[0]!)).toBe(true);
  });

  it("counts a control a `sample` rule points at, without opening a frame for it", () => {
    const spec = buildSpec({
      title: "T",
      content: [
        {
          kind: "sample",
          label: "Sessions",
          source: "read just now",
          rule: "idle_h > stale_hours",
          rows: [{ label: "a", idle_h: 1 }, { label: "b", idle_h: 9 }],
        },
      ],
      controls: [
        { id: "stale_hours", label: "Stale after", type: "number", default: 4 },
        { id: "note", label: "Note", type: "text", default: "" },
      ],
    });
    expect(shouldAutoPreview(spec)).toBe(false);
    expect(controlIsIllustrated(spec, spec.controls[0]!)).toBe(true);
    expect(controlIsIllustrated(spec, spec.controls[1]!)).toBe(false);
  });

  it("counts the text control a bound block renders — the one type that never got a preview", () => {
    const spec = buildSpec({
      title: "T",
      content: [
        { kind: "code", bind: true, value: 'import "{{pkg}}"' },
        { kind: "markdown", value: "unbound {{other}} stays literal" },
      ],
      controls: [
        { id: "pkg", label: "Package", type: "text", default: "agentui" },
        { id: "other", label: "Other", type: "text", default: "" },
      ],
    });
    expect(shouldAutoPreview(spec)).toBe(false);
    expect(controlIsIllustrated(spec, spec.controls[0]!)).toBe(true);
    // Braces in an UNBOUND block illustrate nothing, because nothing substitutes.
    expect(controlIsIllustrated(spec, spec.controls[1]!)).toBe(false);
  });

  it("counts a multiselect only when its weights are something measured", () => {
    const withWeights = build([
      {
        id: "files",
        label: "Files",
        type: "multiselect",
        options: [{ value: "a", weight: 180 }, { value: "b", weight: 240 }],
        default: [],
      },
    ]);
    const without = build([
      { id: "files", label: "Files", type: "multiselect", options: [{ value: "a" }, { value: "b" }], default: [] },
    ]);
    expect(controlIsIllustrated(withWeights, withWeights.controls[0]!)).toBe(true);
    expect(controlIsIllustrated(without, without.controls[0]!)).toBe(false);
  });
});

describe("groupControls visualFirst", () => {
  it("sinks groups the preview cannot illustrate below ones it can", () => {
    const spec = build([
      { id: "verbose", label: "Verbose", type: "toggle", default: false, group: "Behaviour" },
      { id: "tint", label: "Tint", type: "color", default: "#000000", group: "Colour" },
      { id: "gap", label: "Gap", type: "slider", min: 0, max: 20, default: 4, unit: "px", group: "Layout" },
    ]);
    const ordered = groupControls(spec, { visualFirst: true, isVisual: (c) => controlIsIllustrated(spec, c) });
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
    const ordered = groupControls(spec, { visualFirst: true, isVisual: (c) => controlIsIllustrated(spec, c) });
    expect(ordered.map((g) => g.group)).toEqual(["Visual", "First non-visual", "Second non-visual"]);
  });
});

describe("what earns a preview frame", () => {
  it("no longer claims a duration is a length", () => {
    // Time is not spatial; a bar for "400ms" was a category error. An explicit
    // unit still qualifies, because that is a declaration rather than a guess.
    expect(shouldAutoPreview(build([{ id: "retry_delay", label: "Retry delay", type: "number", default: 400 }]))).toBe(
      false,
    );
    expect(
      shouldAutoPreview(build([{ id: "retry_delay", label: "Retry delay", type: "number", default: 400, unit: "ms" }])),
    ).toBe(true);
  });

  it("opens a frame for a vector, now that `pad` can draw one", () => {
    // This used to assert the opposite, and it was documenting a HOLE rather
    // than a rule: no view could draw a vector, so a widget whose only control
    // was a position opened no frame at all and its numbers were the whole
    // answer. See preview-agreement.test.ts for the panel it gets.
    expect(shouldAutoPreview(build([{ id: "pos", label: "Position", type: "vector", size: 3, default: [1, 2, 3] }]))).toBe(
      true,
    );
  });

  it("lets an agent decline the generated preview outright", () => {
    const spec = parseSpec({
      title: "T",
      preview: { auto: false },
      controls: [{ id: "gap", label: "Gap", type: "slider", min: 0, max: 10, default: 2, unit: "px" }],
    });
    if (!spec.ok) throw new Error(spec.message);
    expect(shouldAutoPreview(spec.spec)).toBe(false);
  });
});

/**
 * Easing detection is deliberately two-factor: the control must be NAMED like
 * an easing choice AND every option must be easing vocabulary. A false
 * positive draws a meaningless curve over a real select; a miss just leaves
 * the state line, which is the acceptable failure.
 */
describe("isEasingSelect", () => {
  const easing = (over: Record<string, unknown> = {}) => ({
    id: "easing",
    label: "Easing",
    type: "select",
    default: "linear",
    options: [{ value: "linear" }, { value: "ease-out" }, { value: "spring" }],
    ...over,
  });
  const first = (controls: unknown[]) => build(controls).controls[0]!;

  it("detects a named select whose options are all easing vocabulary", () => {
    expect(isEasingSelect(first([easing()]))).toBe(true);
  });

  it("accepts cubic-bezier literals and camelCase names", () => {
    const control = first([
      easing({ default: "easeOutBack", options: [{ value: "easeOutBack" }, { value: "cubic-bezier(0.2, 0, 0, 1)" }] }),
    ]);
    expect(isEasingSelect(control)).toBe(true);
  });

  it("rejects an easing-named select with non-easing options", () => {
    expect(isEasingSelect(first([easing({ default: "fast", options: [{ value: "fast" }, { value: "slow" }] })]))).toBe(
      false,
    );
  });

  it("rejects easing vocabulary under a non-easing name", () => {
    expect(isEasingSelect(first([easing({ id: "mode", label: "Mode" })]))).toBe(false);
  });

  it("rejects a single-option select — nothing to choose is nothing to plot", () => {
    expect(isEasingSelect(first([easing({ options: [{ value: "linear" }] })]))).toBe(false);
  });

  it("earns a preview frame on its own, because easingCurve can draw it", () => {
    expect(shouldAutoPreview(build([easing()]))).toBe(true);
  });

});
