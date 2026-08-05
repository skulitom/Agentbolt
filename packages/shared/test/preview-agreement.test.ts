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
