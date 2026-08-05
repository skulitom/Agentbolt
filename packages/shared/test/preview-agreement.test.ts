import { describe, expect, it } from "vitest";
// The browser helper, imported under node. Legal because neither preview.js nor
// motion.js touches the DOM at module scope — see the note at the top of
// motion.js, which ui/test/motion.test.ts already relies on.
import { VIEW_IMPLS } from "../../ui/public/agentui/preview.js";
import { shouldAutoPreview } from "../src/auto-preview.js";
import { PREVIEW_VIEWS, planPreview } from "../src/preview-plan.js";
import { parseSpec, type InterfaceSpec } from "../src/spec.js";

function build(controls: unknown[], extra: Record<string, unknown> = {}): InterfaceSpec {
  const parsed = parseSpec({ title: "T", controls, ...extra });
  if (!parsed.ok) throw new Error(parsed.message);
  return parsed.spec;
}

const slider = (over: Record<string, unknown>) => ({
  id: "x",
  label: "X",
  type: "slider",
  min: 0,
  max: 100,
  default: 10,
  ...over,
});

const easing = (over: Record<string, unknown> = {}) => ({
  id: "easing",
  label: "Easing",
  type: "select",
  default: "linear",
  options: [{ value: "linear" }, { value: "ease-out" }, { value: "spring" }],
  ...over,
});

const viewsOf = (spec: InterfaceSpec) => planPreview(spec).panels.map((p) => p.view);

/**
 * The gate and the contents are now ONE function.
 *
 * `shouldAutoPreview` (shell) and the frame's panel list used to be separate
 * implementations that this file compared, and the comparison caught a real
 * disagreement: a control named "type_scale" landed in the frame's font bucket
 * while the gate said there was nothing to show, so the frame never opened.
 * Both sides call `planPreview` now, so what is left to test is the plan itself
 * — and the one seam that remains, between the vocabulary and its DOM.
 */
describe("every view in the vocabulary has an implementation", () => {
  it("PREVIEW_VIEWS and VIEW_IMPLS describe the same set", () => {
    // A view declared with `draws: true` and no implementation is worse than a
    // missing view: it opens a frame, and the frame then renders nothing.
    expect(Object.keys(VIEW_IMPLS).sort()).toEqual(Object.keys(PREVIEW_VIEWS).sort());
  });

  it("marks exactly the state line as not worth opening a frame for", () => {
    const notDrawing = Object.keys(PREVIEW_VIEWS).filter((name) => !PREVIEW_VIEWS[name]!.draws);
    expect(notDrawing).toEqual(["state"]);
  });
});

describe("the gate follows the plan", () => {
  const cases: Array<{ name: string; spec: InterfaceSpec; views: string[] }> = [
    { name: "a colour", spec: build([{ id: "tint", label: "Tint", type: "color", default: "#ef9f27" }]), views: ["swatch", "card"] },
    { name: "a slider with a unit", spec: build([slider({ unit: "px" })]), views: ["ruler"] },
    { name: "a spatial name with no unit", spec: build([slider({ id: "corner_radius", label: "Corner radius" })]), views: ["ruler", "card"] },
    { name: "a type scale by name only", spec: build([slider({ id: "type_scale", label: "Type scale" })]), views: ["type", "card"] },
    { name: "an abstract number", spec: build([{ id: "retries", label: "Retries", type: "number", default: 3 }]), views: ["state"] },
    { name: "a bare duration name", spec: build([{ id: "retry_delay", label: "Retry delay", type: "number", default: 400 }]), views: ["state"] },
    { name: "an easing select", spec: build([easing()]), views: ["motion"] },
    { name: "an easing-named select with non-easing options", spec: build([easing({ default: "fast", options: [{ value: "fast" }, { value: "slow" }] })]), views: ["state"] },
    { name: "a toggle", spec: build([{ id: "verbose", label: "Verbose", type: "toggle", default: false }]), views: ["state"] },
    { name: "free text", spec: build([{ id: "notes", label: "Notes", type: "text", default: "" }]), views: [] },
    { name: "nothing at all", spec: build([], { content: [{ kind: "markdown", value: "just prose" }] }), views: [] },
  ];

  for (const { name, spec, views } of cases) {
    it(`${name} plans ${views.length ? views.join(" + ") : "nothing"}`, () => {
      expect(viewsOf(spec)).toEqual(views);
      // The gate is exactly "does any planned view draw", so a plan of only
      // `state` must not open a frame: a pane whose entire content is
      // "Retries: 3" is the failure this tool exists to prevent.
      expect(shouldAutoPreview(spec)).toBe(views.some((v) => PREVIEW_VIEWS[v]!.draws));
    });
  }

  it("honours an agent declining the generated preview, which the plan cannot see", () => {
    // `preview.auto` is the one input to the gate that is NOT a property of the
    // controls, so this is the one legitimate divergence.
    const spec = build([slider({ id: "gap", label: "Gap", unit: "px" })], { preview: { auto: false } });
    expect(shouldAutoPreview(spec)).toBe(false);
    expect(planPreview(spec).drawsSomething).toBe(true);
  });
});

/**
 * One easing select used to claim the widget's only motion panel, and every
 * other one fell through to the state line — so a widget tuning both an enter
 * and an exit transition could show you exactly one of them, which is precisely
 * the comparison it existed to support.
 */
describe("motion groups", () => {
  const trio = (prefix: string) => [
    easing({ id: `${prefix}_easing`, label: `${prefix} easing` }),
    slider({ id: `${prefix}_duration`, label: `${prefix} duration`, unit: "ms" }),
    slider({ id: `${prefix}_delay`, label: `${prefix} delay`, unit: "ms" }),
  ];

  it("gives each easing select its own panel", () => {
    const spec = build([...trio("enter"), ...trio("exit")]);
    expect(viewsOf(spec)).toEqual(["motion", "motion"]);
  });

  it("claims a duration and delay per group, so neither is drawn twice", () => {
    const spec = build([...trio("enter"), ...trio("exit")]);
    const [first, second] = planPreview(spec).panels;
    expect(first?.controls.map((c) => c.id)).toEqual(["enter_easing", "enter_duration", "enter_delay"]);
    expect(second?.controls.map((c) => c.id)).toEqual(["exit_easing", "exit_duration", "exit_delay"]);
    // A claimed duration must not also appear in the ruler.
    expect(viewsOf(spec)).not.toContain("ruler");
  });

  it("leaves an unclaimed duration to the ruler, since nothing is replaying it", () => {
    const spec = build([slider({ id: "timeout", label: "Timeout", unit: "ms" })]);
    expect(viewsOf(spec)).toEqual(["ruler"]);
  });
});

/**
 * Declared views: the middle ground between accepting inference and writing a
 * whole previewModule.
 *
 * The failure this has to avoid is a heading with nothing under it. A view that
 * cannot draw the control it was given, or a panel below the view's minimum,
 * would render as an empty section — so both are parse errors naming the
 * control and the view, rather than something you discover by looking.
 */
describe("declared preview.views", () => {
  const twoColours = [
    { id: "accent", label: "Accent", type: "color", default: "#ef9f27" },
    { id: "surface", label: "Surface", type: "color", default: "#1c1c1a" },
  ];

  it("replaces inference entirely rather than adding to it", () => {
    // Inference alone would give swatch + card here.
    expect(viewsOf(build(twoColours))).toEqual(["swatch", "card"]);
    const declared = build(twoColours, { preview: { views: [{ view: "swatch", controls: ["accent"] }] } });
    expect(viewsOf(declared)).toEqual(["swatch"]);
    expect(planPreview(declared).panels[0]?.controls.map((c) => c.id)).toEqual(["accent"]);
  });

  it("keeps the author's order, not the spec's", () => {
    const spec = build(twoColours, { preview: { views: [{ view: "swatch", controls: ["surface", "accent"] }] } });
    expect(planPreview(spec).panels[0]?.controls.map((c) => c.id)).toEqual(["surface", "accent"]);
  });

  it("uses the declared title over the view's own label", () => {
    const spec = build(twoColours, { preview: { views: [{ view: "swatch", controls: ["accent"], title: "Brand" }] } });
    expect(planPreview(spec).panels[0]?.title).toBe("Brand");
  });

  it("lets an author split one bucket into several panels", () => {
    // Not expressible by inference at all: two swatch panels over the same type.
    const spec = build(twoColours, {
      preview: {
        views: [
          { view: "swatch", controls: ["accent"], title: "Brand" },
          { view: "swatch", controls: ["surface"], title: "Chrome" },
        ],
      },
    });
    expect(viewsOf(spec)).toEqual(["swatch", "swatch"]);
    expect(planPreview(spec).panels.map((p) => p.title)).toEqual(["Brand", "Chrome"]);
  });

  it("marks declared panels as declared, so nothing treats them as guesses", () => {
    const spec = build(twoColours, { preview: { views: [{ view: "swatch", controls: ["accent"] }] } });
    expect(planPreview(spec).panels[0]?.declared).toBe(true);
    expect(planPreview(build(twoColours)).panels[0]?.declared).toBe(false);
  });

  it("rejects a view that cannot draw the control it was given", () => {
    const result = parseSpec({
      title: "T",
      controls: [{ id: "verbose", label: "Verbose", type: "toggle", default: false }],
      preview: { views: [{ view: "swatch", controls: ["verbose"] }] },
    });
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.message).toMatch(/"swatch" view draws color/);
    expect(result.message).toMatch(/verbose/);
  });

  it("rejects a control id that does not exist", () => {
    const result = parseSpec({
      title: "T",
      controls: twoColours,
      preview: { views: [{ view: "swatch", controls: ["nope"] }] },
    });
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.message).toMatch(/not a declared control id/);
  });

  it("rejects an unknown view by name", () => {
    const result = parseSpec({
      title: "T",
      controls: twoColours,
      preview: { views: [{ view: "hologram", controls: ["accent"] }] },
    });
    expect(result.ok).toBe(false);
  });

  it("still opens a frame for a declared state line only if something draws", () => {
    const spec = build([{ id: "retries", label: "Retries", type: "number", default: 3 }], {
      preview: { views: [{ view: "state", controls: ["retries"] }] },
    });
    expect(viewsOf(spec)).toEqual(["state"]);
    expect(shouldAutoPreview(spec)).toBe(false);
  });
});

/**
 * `vector` was the one control type no view could draw. A widget whose only
 * control was a position therefore opened NO preview frame at all, and its two
 * or three numbers were the entire answer — the exact "a control you cannot see
 * the effect of" failure this tool exists to prevent.
 */
describe("a vector now has somewhere to go", () => {
  const vector = (over: Record<string, unknown> = {}) => ({
    id: "origin",
    label: "Origin",
    type: "vector",
    size: 2,
    default: [0.5, 0.5],
    ...over,
  });

  it("opens a frame for a vector-only widget", () => {
    const spec = build([vector()]);
    expect(viewsOf(spec)).toEqual(["pad"]);
    expect(shouldAutoPreview(spec)).toBe(true);
  });

  it("handles a 3-vector too", () => {
    const spec = build([vector({ size: 3, default: [0, 0, 0] })]);
    expect(planPreview(spec).panels[0]?.controls[0]?.id).toBe("origin");
  });

  it("sits alongside the other panels rather than replacing them", () => {
    const spec = build([vector(), { id: "tint", label: "Tint", type: "color", default: "#000000" }]);
    expect(viewsOf(spec)).toEqual(["swatch", "pad", "card"]);
  });
});

/**
 * Three views are reachable only by declaring them, and that is the design.
 *
 * Which of two colours is "the start of a ramp", and which of four lengths is
 * "the padding", are statements of INTENT. Inferring them would mean drawing a
 * relationship the author never claimed — the same reason a `sample` block has
 * to carry its `source`.
 */
describe("views that are declaration-only", () => {
  const palette = [
    { id: "from", label: "From", type: "color", default: "#ef9f27" },
    { id: "to", label: "To", type: "color", default: "#b0367a" },
  ];

  it("does not infer a gradient from two unrelated colours", () => {
    expect(viewsOf(build(palette))).toEqual(["swatch", "card"]);
  });

  it("draws one when asked", () => {
    const spec = build(palette, { preview: { views: [{ view: "gradient", controls: ["from", "to"] }] } });
    expect(viewsOf(spec)).toEqual(["gradient"]);
  });

  it("refuses a one-stop gradient, which is a swatch", () => {
    const result = parseSpec({
      title: "T",
      controls: palette,
      preview: { views: [{ view: "gradient", controls: ["from"] }] },
    });
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.message).toMatch(/at least 2 controls/);
  });

  it("does not infer a box model from lengths that happen to be named padding", () => {
    const spec = build([
      { id: "padding", label: "Padding", type: "slider", min: 0, max: 40, default: 12, unit: "px" },
      { id: "radius", label: "Radius", type: "slider", min: 0, max: 20, default: 6, unit: "px" },
    ]);
    expect(viewsOf(spec)).toEqual(["ruler", "card"]);
  });

  it("draws a box model when asked", () => {
    const spec = build(
      [
        { id: "padding", label: "Padding", type: "slider", min: 0, max: 40, default: 12, unit: "px" },
        { id: "radius", label: "Radius", type: "slider", min: 0, max: 20, default: 6, unit: "px" },
      ],
      { preview: { views: [{ view: "box", controls: ["padding", "radius"] }] } },
    );
    expect(viewsOf(spec)).toEqual(["box"]);
  });

  it("lets shadow mix lengths and a colour, which no inferred bucket can", () => {
    const spec = build(
      [
        { id: "blur", label: "Blur", type: "slider", min: 0, max: 60, default: 12, unit: "px" },
        { id: "shadow_tint", label: "Shadow tint", type: "color", default: "#00000040".slice(0, 7) },
      ],
      { preview: { views: [{ view: "shadow", controls: ["blur", "shadow_tint"] }] } },
    );
    expect(viewsOf(spec)).toEqual(["shadow"]);
    expect(planPreview(spec).panels[0]?.controls.map((c) => c.type)).toEqual(["slider", "color"]);
  });
});
