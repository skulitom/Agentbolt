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

/**
 * The compare validator earns its keep here. Prose lets an agent write two
 * sentences about approach A and none about B; a matrix has a hole where B's
 * cell goes, and these tests are what make the parser refuse to ship the hole.
 */
describe("compare", () => {
  const compare = (over: Record<string, unknown> = {}) => ({
    title: "T",
    controls: [
      {
        id: "approach",
        label: "Approach",
        type: "compare",
        axes: [
          { id: "files", label: "Files touched", kind: "text" },
          { id: "wire", label: "New protocol message", kind: "bool", better: "low" },
        ],
        options: [
          { value: "poll", cells: { files: "2", wire: false } },
          { value: "push", cells: { files: "5", wire: true } },
        ],
        default: "poll",
        ...over,
      },
    ],
  });

  it("accepts a filled matrix and keeps the value a plain string", () => {
    const spec = ok(compare());
    expect(defaultsFromSpec(spec)).toEqual({ approach: "poll" });
    expect(validateValuesAgainstSpec(spec, { approach: "push" })).toEqual([]);
    expect(validateValuesAgainstSpec(spec, { approach: "nope" })).toHaveLength(1);
  });

  it("names the option and the axis when a cell is missing", () => {
    const message = err(
      compare({ options: [{ value: "poll", cells: { files: "2", wire: false } }, { value: "push", cells: { files: "5" } }] }),
    );
    expect(message).toContain("controls[0].options[1].cells");
    expect(message).toContain('missing cell for axis "wire"');
  });

  it("catches a cell for an axis that does not exist", () => {
    const message = err(
      compare({
        options: [
          { value: "poll", cells: { files: "2", wire: false, speed: "fast" } },
          { value: "push", cells: { files: "5", wire: true } },
        ],
      }),
    );
    expect(message).toMatch(/no axis with id "speed"/);
  });

  it("sends a one-axis compare back to select", () => {
    expect(err(compare({ axes: [{ id: "files", label: "Files", kind: "text" }] }))).toMatch(/use that instead/);
  });

  it("requires a scale axis to declare its scale, and its cells to be in it", () => {
    const axes = [
      { id: "files", label: "Files", kind: "text" },
      { id: "cost", label: "Build", kind: "scale", better: "low" },
    ];
    expect(err(compare({ axes }))).toMatch(/needs its `scale`/);

    const scaled = axes.map((a) => (a.id === "cost" ? { ...a, scale: ["S", "M", "L"] } : a));
    expect(
      err(
        compare({
          axes: scaled,
          options: [
            { value: "poll", cells: { files: "2", cost: "S" } },
            { value: "push", cells: { files: "5", cost: "enormous" } },
          ],
        }),
      ),
    ).toMatch(/not one of axis "cost"'s scale/);
  });

  it("keeps bool axes boolean in both directions", () => {
    expect(
      err(
        compare({
          options: [
            { value: "poll", cells: { files: "2", wire: "no" } },
            { value: "push", cells: { files: "5", wire: true } },
          ],
        }),
      ),
    ).toMatch(/cells are true or false/);
    expect(
      err(
        compare({
          axes: [
            { id: "files", label: "Files", kind: "text" },
            { id: "wire", label: "Wire", kind: "text" },
          ],
          options: [
            { value: "poll", cells: { files: "2", wire: false } },
            { value: "push", cells: { files: "5", wire: true } },
          ],
        }),
      ),
    ).toMatch(/cells are strings/);
  });

  it("does not ask for cells an axis generates from its template", () => {
    const spec = ok(
      compare({
        axes: [
          { id: "files", label: "Files", kind: "text" },
          { id: "install", label: "Install", kind: "code", template: "npm i {}" },
        ],
        options: [{ value: "poll", cells: { files: "2" } }, { value: "push", cells: { files: "5" } }],
      }),
    );
    expect(spec.controls).toHaveLength(1);
    expect(
      err(
        compare({
          axes: [
            { id: "files", label: "Files", kind: "text" },
            { id: "install", label: "Install", kind: "code", template: "npm i {}" },
          ],
          options: [
            { value: "poll", cells: { files: "2", install: "npm i poll" } },
            { value: "push", cells: { files: "5" } },
          ],
        }),
      ),
    ).toMatch(/generates its cells from `template`/);
  });

  it("catches a default that is not an option", () => {
    expect(err(compare({ default: "watch" }))).toMatch(/not one of the options/);
  });
});

describe("multiselect", () => {
  const files = {
    title: "T",
    controls: [
      {
        id: "files",
        label: "Files",
        type: "multiselect",
        weightUnit: "lines",
        options: [
          { value: "core/waiters.ts", weight: 180 },
          { value: "core/session.ts", weight: 240 },
          { value: "mcp/tools.ts", weight: 210 },
        ],
        default: ["core/waiters.ts", "mcp/tools.ts"],
      },
    ],
  };

  it("round-trips a string[] through defaults, validation and normalize", () => {
    const spec = ok(files);
    expect(defaultsFromSpec(spec)).toEqual({ files: ["core/waiters.ts", "mcp/tools.ts"] });
    expect(validateValuesAgainstSpec(spec, { files: [] })).toEqual([]);
    expect(normalizeValues(spec, { files: ["core/session.ts"] })).toEqual({ files: ["core/session.ts"] });
    // Junk falls back to the default rather than throwing, like every other type.
    expect(normalizeValues(spec, { files: ["nope"] })).toEqual({ files: ["core/waiters.ts", "mcp/tools.ts"] });
  });

  it("copies the default, so a caller cannot mutate the spec", () => {
    const spec = ok(files);
    (defaultsFromSpec(spec)["files"] as string[]).push("x");
    expect(defaultsFromSpec(spec)["files"]).toHaveLength(2);
  });

  it("rejects unknown values, duplicates and non-arrays", () => {
    const spec = ok(files);
    expect(validateValuesAgainstSpec(spec, { files: ["mcp/heartbeat.ts"] })[0]?.message).toMatch(/not options/);
    expect(validateValuesAgainstSpec(spec, { files: ["mcp/tools.ts", "mcp/tools.ts"] })[0]?.message).toMatch(/more than once/);
    expect(validateValuesAgainstSpec(spec, { files: "mcp/tools.ts" })).toHaveLength(1);
  });

  it("catches a default that is not one of the options", () => {
    expect(err({ ...files, controls: [{ ...files.controls[0], default: ["nope"] }] })).toMatch(/not one of the options/);
  });
});

describe("sample blocks", () => {
  const sample = (over: Record<string, unknown> = {}, controls?: unknown[]) => ({
    title: "T",
    content: [
      {
        kind: "sample",
        label: "Open sessions",
        source: "~/.agentui/sessions/*/state.json — read just now",
        rule: "idle_h > stale_hours",
        rows: [
          { label: "muzzle-flash", idle_h: 0.33 },
          { label: "changelog approval", idle_h: 27 },
        ],
        ...over,
      },
    ],
    controls: controls ?? [
      { id: "stale_hours", label: "Stale after", type: "slider", min: 1, max: 48, default: 4, unit: "h" },
    ],
  });

  it("accepts a well-formed block", () => {
    expect(ok(sample()).content[0]).toMatchObject({ kind: "sample", rule: "idle_h > stale_hours" });
  });

  it("teaches the rule shape when it does not parse", () => {
    const message = err(sample({ rule: "idle_h > 5" }));
    expect(message).toContain("content[0].rule");
    expect(message).toMatch(/<row_field> <op> <control_id>/);
  });

  it("insists the right-hand side is a declared control", () => {
    expect(err(sample({ rule: "idle_h > nope" }))).toMatch(/not a declared control id \(stale_hours\)/);
  });

  it("catches a field no row carries", () => {
    expect(err(sample({ rule: "missing > stale_hours" }))).toMatch(/no "missing" on this row/);
  });

  it("checks numbers at parse time instead of putting NaN on screen", () => {
    expect(
      err(sample({ rows: [{ label: "a", idle_h: "ages" }, { label: "b", idle_h: 3 }] })),
    ).toMatch(/needs a number here/);
  });

  it("will not order against something that is not a number", () => {
    expect(
      err(
        sample({ rule: "idle_h > mode" }, [
          { id: "mode", label: "Mode", type: "select", options: [{ value: "a" }, { value: "b" }], default: "a" },
        ]),
      ),
    ).toMatch(/use == or != to match it as a string/);
  });

  it("checks an explicit field and the total field too", () => {
    expect(err(sample({ field: "kb" }))).toMatch(/no "kb" on this row/);
    expect(err(sample({ total: { field: "kb" } }))).toMatch(/no "kb" on this row/);
  });

  it("refuses a table that is really a sentence", () => {
    expect(err(sample({ rows: [{ label: "only one", idle_h: 1 }] }))).toMatch(/sentence, not a table/);
  });

  it("requires a source, because a fabricated table is the failure mode", () => {
    expect(err(sample({ source: "" }))).toMatch(/content\[0\]\.source/);
  });
});

describe("bind", () => {
  const bound = (value: string, controls?: unknown[]) => ({
    title: "T",
    content: [{ kind: "code", bind: true, value }],
    controls: controls ?? [{ id: "pkg", label: "Package", type: "text", default: "agentui" }],
  });

  it("accepts a placeholder naming a declared control", () => {
    expect(ok(bound('import "{{pkg}}"')).content[0]).toMatchObject({ bind: true });
  });

  it("names the valid ids when a placeholder names nothing", () => {
    expect(err(bound("{{package}}"))).toMatch(/names no control \(declared: pkg\)/);
  });

  it("catches a transform that does not exist", () => {
    expect(err(bound("{{pkg:shouty}}"))).toMatch(/unknown transform/);
  });

  it("sends a toggle to the select-of-snippets pattern instead", () => {
    expect(
      err(bound("{{verbose}}", [{ id: "verbose", label: "Verbose", type: "toggle", default: false }])),
    ).toMatch(/option VALUES are the alternative lines/);
  });

  it("sends a list-valued control to a sample block", () => {
    expect(
      err(
        bound("{{files}}", [
          {
            id: "files",
            label: "Files",
            type: "multiselect",
            options: [{ value: "a" }, { value: "b" }],
            default: [],
          },
        ]),
      ),
    ).toMatch(/`sample` block instead/);
  });

  it("says nothing about braces in an UNBOUND block", () => {
    // Otherwise a widget about a Helm chart or a GitHub Action is unauthorable.
    const spec = ok({
      title: "T",
      content: [{ kind: "code", value: "image: {{ .Values.image }}\nkey: ${{ secrets.TOKEN }}" }],
      controls: [{ id: "pkg", label: "Package", type: "text", default: "agentui" }],
    });
    expect(spec.content[0]).toMatchObject({ bind: false });
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

/**
 * Three fields that parsed but did nothing.
 *
 * `html` blocks validated and then rendered a fixed notice string; `layout.columns`
 * was read by no one; and `layout.previewPosition` defaulted to "top" while the
 * only consumer defaulted to "right", so declaring `columns` — the field that did
 * nothing — silently moved the preview frame. Rejecting them by name is what
 * turns a silent lie into an error an agent can act on.
 */
describe("vocabulary that never worked is now rejected, not silently accepted", () => {
  it("rejects an html content block and names the kinds that do exist", () => {
    const message = err({
      title: "T",
      content: [{ kind: "html", value: "<b>hi</b>" }],
    });
    expect(message).toMatch(/markdown/);
    expect(message).toMatch(/content/);
  });

  it("rejects `layout` as an unknown key rather than absorbing it", () => {
    expect(err({ title: "T", content: [{ kind: "markdown", value: "x" }], layout: { columns: 2 } })).toMatch(
      /layout/,
    );
  });

  it("defaults the preview to the right-hand split, matching what the renderer draws", () => {
    const spec = ok({ title: "T", content: [{ kind: "markdown", value: "x" }], preview: {} });
    expect(spec.preview?.position).toBe("right");
  });

  it("still accepts an explicit stacked preview", () => {
    const spec = ok({ title: "T", content: [{ kind: "markdown", value: "x" }], preview: { position: "top" } });
    expect(spec.preview?.position).toBe("top");
  });

  it("leaves preview undefined when it was never mentioned, so the renderer picks the default", () => {
    const spec = ok({ title: "T", content: [{ kind: "markdown", value: "x" }] });
    expect(spec.preview).toBeUndefined();
  });
});
