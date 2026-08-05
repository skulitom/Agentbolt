import { describe, expect, it } from "vitest";
// The browser helper, imported under node. Legal because neither preview.js nor
// motion.js touches the DOM at module scope — see the note at the top of
// motion.js, which ui/test/motion.test.ts already relies on.
import { planPreview } from "../../ui/public/agentui/preview.js";
import { shouldAutoPreview } from "../src/auto-preview.js";
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

/**
 * THE test the regex-scraping ones were a proxy for.
 *
 * Two separate pieces of code answer two halves of one question: `shouldAutoPreview`
 * in shared decides whether a preview frame appears at all, and `planPreview` in
 * the browser helper decides what goes inside it. They used to agree only because
 * their hint regexes were hand-copied and a test compared the regex SOURCE — which
 * says nothing about the decision the regexes feed.
 *
 * Disagreement in one direction opens a half-screen pane reading "Nothing here
 * has a visual effect to preview." In the other, a widget that had something to
 * show never gets a frame. Both are the "looks informative, says nothing"
 * failure this tool exists to avoid, and neither is visible from a unit test of
 * either side alone.
 */
describe("the two halves of the auto-preview decision agree", () => {
  const cases: Array<{ name: string; spec: InterfaceSpec }> = [
    { name: "a colour", spec: build([{ id: "tint", label: "Tint", type: "color", default: "#ef9f27" }]) },
    { name: "a slider with a unit", spec: build([slider({ unit: "px" })]) },
    { name: "a slider with a unit but an abstract name", spec: build([slider({ id: "amount", label: "Amount", unit: "x" })]) },
    { name: "a spatial name with no unit", spec: build([slider({ id: "corner_radius", label: "Corner radius" })]) },
    { name: "gap", spec: build([slider({ id: "gap", label: "Gap" })]) },
    { name: "padding", spec: build([slider({ id: "padding", label: "Padding" })]) },
    { name: "offset", spec: build([slider({ id: "offset", label: "Offset" })]) },
    { name: "a font size", spec: build([slider({ id: "font_size", label: "Font size", unit: "px" })]) },
    { name: "a type scale by name only", spec: build([slider({ id: "type_scale", label: "Type scale" })]) },
    { name: "an abstract number", spec: build([{ id: "retries", label: "Retries", type: "number", default: 3 }]) },
    { name: "a bare duration name", spec: build([{ id: "retry_delay", label: "Retry delay", type: "number", default: 400 }]) },
    { name: "a duration with a unit", spec: build([slider({ id: "retry_delay", label: "Retry delay", unit: "ms" })]) },
    { name: "an easing select", spec: build([easing()]) },
    { name: "an easing select with camelCase names", spec: build([easing({ default: "easeOutBack", options: [{ value: "easeOutBack" }, { value: "cubic-bezier(0.2, 0, 0, 1)" }] })]) },
    { name: "an easing-named select with non-easing options", spec: build([easing({ default: "fast", options: [{ value: "fast" }, { value: "slow" }] })]) },
    { name: "easing vocabulary under a non-easing name", spec: build([easing({ id: "mode", label: "Mode" })]) },
    { name: "the motion trio", spec: build([easing(), slider({ id: "duration", label: "Duration", unit: "ms" }), slider({ id: "delay", label: "Delay", unit: "ms" })]) },
    { name: "a toggle", spec: build([{ id: "verbose", label: "Verbose", type: "toggle", default: false }]) },
    { name: "a plain select", spec: build([{ id: "mode", label: "Mode", type: "select", options: [{ value: "a" }, { value: "b" }], default: "a" }]) },
    { name: "free text", spec: build([{ id: "notes", label: "Notes", type: "text", default: "" }]) },
    { name: "a vector, which the helper cannot draw", spec: build([{ id: "pos", label: "Position", type: "vector", size: 3, default: [1, 2, 3] }]) },
    {
      name: "a multiselect",
      spec: build([{ id: "files", label: "Files", type: "multiselect", options: [{ value: "a", weight: 1 }, { value: "b", weight: 2 }], default: [] }]),
    },
    {
      name: "a compare",
      spec: build([
        {
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
        },
      ]),
    },
    { name: "a colour beside a toggle", spec: build([{ id: "verbose", label: "Verbose", type: "toggle", default: false }, { id: "tint", label: "Tint", type: "color", default: "#000000" }]) },
    { name: "nothing at all", spec: build([], { content: [{ kind: "markdown", value: "just prose" }] }) },
  ];

  for (const { name, spec } of cases) {
    it(`${name}: a frame opens exactly when there is something to put in it`, () => {
      const plan = planPreview(spec);
      // `shouldAutoPreview` is the gate; `plan.drawsSomething` is whether the
      // helper has anything to draw. Equal, or a pane opens with nothing in it
      // — or a widget that had something to show never gets a frame.
      expect(shouldAutoPreview(spec), name).toBe(plan.drawsSomething);
    });
  }

  it("does not open a frame that would only contain a state line", () => {
    // A toggle lands in `others`, which renders as a text line — real content
    // for a frame that already exists, never a reason to open one.
    const spec = build([{ id: "verbose", label: "Verbose", type: "toggle", default: false }]);
    expect(planPreview(spec).others).toHaveLength(1);
    // Content for a frame, but not a reason to open one.
    expect(planPreview(spec).empty).toBe(false);
    expect(planPreview(spec).drawsSomething).toBe(false);
    expect(shouldAutoPreview(spec)).toBe(false);
  });

  it("leaves shell-illustrated types out of the state line", () => {
    // text/compare/multiselect are drawn by the shell's own inputs. Echoing
    // them in the frame says the same thing twice, and worse.
    const spec = build([
      { id: "notes", label: "Notes", type: "text", default: "" },
      { id: "tint", label: "Tint", type: "color", default: "#000000" },
    ]);
    expect(planPreview(spec).others).toHaveLength(0);
  });

  it("lets the easing select claim the duration and delay controls", () => {
    const spec = build([
      easing(),
      slider({ id: "duration", label: "Duration", unit: "ms" }),
      slider({ id: "delay", label: "Delay", unit: "ms" }),
    ]);
    const plan = planPreview(spec);
    expect(plan.motion).toHaveLength(3);
    expect(plan.durationCtl?.id).toBe("duration");
    expect(plan.delayCtl?.id).toBe("delay");
    // Claimed controls must leave the ruler: the replay IS the duration's
    // visualization, and a bar for the same number says less, twice.
    expect(plan.sizes).toHaveLength(0);
    expect(plan.others).toHaveLength(0);
  });

  it("honours an agent declining the generated preview, which the plan cannot see", () => {
    // `preview.auto` is the one input to the gate that is NOT a property of the
    // controls, so this is the one legitimate disagreement.
    const spec = build([slider({ id: "gap", label: "Gap", unit: "px" })], { preview: { auto: false } });
    expect(shouldAutoPreview(spec)).toBe(false);
    expect(planPreview(spec).drawsSomething).toBe(true);
  });
});
