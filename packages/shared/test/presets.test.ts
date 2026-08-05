import { describe, expect, it } from "vitest";
import { AskInput, compileAskChecked, slugify } from "../src/presets.js";

function compile(input: unknown) {
  const parsed = AskInput.parse(input);
  const result = compileAskChecked(parsed);
  if (!result.ok) throw new Error(`expected ask to compile, got:\n${result.message}`);
  return result.spec;
}

describe("compileAsk", () => {
  it("turns a bare question into a valid widget", () => {
    const spec = compile({ question: "Ship it?" });
    expect(spec.title).toBe("Ship it?");
    expect(spec.content[0]).toMatchObject({ kind: "markdown" });
    expect(spec.actions).toHaveLength(1);
    expect(spec.actions[0]?.id).toBe("submit");
  });

  it("turns string options into action buttons, first one primary", () => {
    const spec = compile({ question: "Ship?", options: ["Ship it", "Hold"] });
    expect(spec.actions.map((a) => [a.id, a.label, a.style])).toEqual([
      ["ship_it", "Ship it", "primary"],
      ["hold", "Hold", "default"],
    ]);
  });

  it("keeps explicit option ids and styles", () => {
    const spec = compile({
      question: "Delete?",
      options: [{ id: "cancel", label: "Keep it" }, { label: "Delete", style: "danger" }],
    });
    expect(spec.actions[0]).toMatchObject({ id: "cancel", style: "primary" });
    expect(spec.actions[1]).toMatchObject({ id: "delete", style: "danger" });
  });

  it("disambiguates options that slugify to the same id", () => {
    const spec = compile({ question: "?", options: ["Go!", "Go?"] });
    expect(new Set(spec.actions.map((a) => a.id)).size).toBe(2);
  });

  it("infers field types and labels from shorthand", () => {
    const spec = compile({
      question: "Configure",
      fields: [
        { id: "service_name" },
        { id: "retries", default: 3 },
        { id: "verbose", default: true },
        { id: "accent", default: "#ef9f27" },
        { id: "mode", options: ["fast", "slow"] },
        { id: "offset", default: [1, 2] },
      ],
    });
    const byId = Object.fromEntries(spec.controls.map((c) => [c.id, c]));
    expect(byId["service_name"]).toMatchObject({ type: "text", label: "Service name", default: "" });
    expect(byId["retries"]).toMatchObject({ type: "number", default: 3 });
    expect(byId["verbose"]).toMatchObject({ type: "toggle", default: true });
    expect(byId["accent"]).toMatchObject({ type: "color", default: "#ef9f27" });
    expect(byId["mode"]).toMatchObject({ type: "select", default: "fast" });
    expect(byId["offset"]).toMatchObject({ type: "vector", size: 2 });
  });

  it("passes the two new control types through shorthand", () => {
    // A new control type is invisible to ui_ask until it is in FieldShorthand's
    // enum — easy to miss, and it silently rejects the whole call when missed.
    const spec = compile({
      question: "Which?",
      fields: [
        {
          id: "approach",
          type: "compare",
          axes: [
            { id: "files", label: "Files", kind: "text" },
            { id: "wire", label: "Wire", kind: "bool" },
          ],
          options: [
            { value: "poll", cells: { files: "2", wire: false } },
            { value: "push", cells: { files: "5", wire: true } },
          ],
        },
        { id: "targets", type: "multiselect", options: ["a", "b"] },
      ],
    });
    const byId = Object.fromEntries(spec.controls.map((c) => [c.id, c]));
    // No default given: the first option for a compare, and nothing pre-checked
    // for a multiselect, because a proposal nobody wrote is not a proposal.
    expect(byId["approach"]).toMatchObject({ type: "compare", default: "poll" });
    expect(byId["targets"]).toMatchObject({ type: "multiselect", default: [] });
  });

  it("reads an array of strings as a selection and an array of numbers as a position", () => {
    const spec = compile({
      question: "?",
      fields: [
        { id: "files", options: ["a", "b", "c"], default: ["a", "c"] },
        { id: "offset", default: [1, 2] },
      ],
    });
    const byId = Object.fromEntries(spec.controls.map((c) => [c.id, c]));
    expect(byId["files"]).toMatchObject({ type: "multiselect", default: ["a", "c"] });
    expect(byId["offset"]).toMatchObject({ type: "vector", size: 2 });
  });

  it("gives a bare slider usable bounds and a midpoint default", () => {
    const spec = compile({ question: "?", fields: [{ id: "gain", type: "slider" }] });
    expect(spec.controls[0]).toMatchObject({ type: "slider", min: 0, max: 1, default: 0.5 });
  });

  it("respects explicit slider bounds when inferring the default", () => {
    const spec = compile({ question: "?", fields: [{ id: "gain", type: "slider", min: 10, max: 20 }] });
    expect(spec.controls[0]).toMatchObject({ default: 15 });
  });

  it("accepts content as a bare string, a block, or a mixed array", () => {
    const spec = compile({
      question: "Review",
      content: ["some prose", { kind: "diff", value: "@@ -1 +1 @@\n-a\n+b" }],
    });
    // index 0 is the question heading
    expect(spec.content.slice(1).map((b) => b.kind)).toEqual(["markdown", "diff"]);
  });

  it("labels the submit button by whether there is anything to fill in", () => {
    expect(compile({ question: "?" }).actions[0]?.label).toBe("OK");
    expect(compile({ question: "?", fields: [{ id: "a" }] }).actions[0]?.label).toBe("Submit");
  });

  it("can suppress the comment box", () => {
    expect(compile({ question: "?", allowComment: false }).comment.enabled).toBe(false);
    expect(compile({ question: "?" }).comment.enabled).toBe(true);
  });

  it("surfaces spec errors from shorthand that cannot be salvaged", () => {
    const result = compileAskChecked(AskInput.parse({ question: "?", fields: [{ id: "Bad Id" }] }));
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toMatch(/snake_case/);
  });
});

describe("slugify", () => {
  it("produces valid control ids from arbitrary labels", () => {
    expect(slugify("Ship it!")).toBe("ship_it");
    expect(slugify("  Hold  ")).toBe("hold");
    expect(slugify("2 fast")).toMatch(/^[a-z]/);
    expect(slugify("!!!")).toMatch(/^[a-z]/);
  });
});
