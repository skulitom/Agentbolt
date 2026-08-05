import { describe, expect, it } from "vitest";
import { shouldAutoPreview } from "../src/auto-preview.js";
import { evaluateSample, type SampleBlock } from "../src/sample.js";
import { defaultsFromSpec, parseSpec, type InterfaceSpec } from "../src/spec.js";

/**
 * The ten questions from docs/visualising-decisions.md, as an agent would
 * actually author them.
 *
 * These are here because the documented patterns are a promise: a widget the
 * skill tells an agent to write must parse, first time, with no retry. A schema
 * change that quietly breaks one of them breaks the docs, and a broken example
 * is worse than no example — the agent copies it, gets a validation error, and
 * concludes the tool is unreliable.
 */

function ok(input: unknown, label: string): InterfaceSpec {
  const result = parseSpec(input);
  if (!result.ok) throw new Error(`${label} does not parse:\n${result.message}`);
  return result.spec;
}

const sessionTtl = {
  title: "Should ui_close delete the session directory?",
  content: [
    {
      kind: "markdown",
      value:
        '**Should `ui_close` delete the session directory?** "Always" and "never" are the ends of one dial: *delete when the session is older than N days*.',
    },
    {
      kind: "sample",
      label: "Session directories on this machine",
      source: "du -sk ~/.agentui/sessions/* && stat -c %Y — run just now, 14 dirs",
      rule: "age_d > keep_days",
      verdict: ["deleted on close", "kept"],
      field: "age_d",
      unit: "d",
      total: { field: "kb", unit: " kB", label: "reclaimed" },
      rows: [
        { label: "muzzle-flash", age_d: 0.01, kb: 96, note: "you adjusted this 20 min ago" },
        { label: "theme-preview", age_d: 0.2, kb: 140 },
        { label: "changelog approval", age_d: 1.1, kb: 88 },
        { label: "three.js demo", age_d: 8, kb: 4180, note: "holds assets/shot.png" },
        { label: "usability probe", age_d: 23, kb: 72 },
      ],
    },
  ],
  controls: [
    { id: "keep_days", type: "slider", label: "Keep for", min: 0, max: 30, step: 1, default: 7, unit: "d" },
  ],
  actions: [
    { id: "use_ttl", label: "Use this TTL", style: "primary" },
    { id: "never", label: "Never delete" },
    { id: "always", label: "Always delete on close" },
  ],
  preview: { auto: false },
};

const staleness = {
  title: 'When does a session stop counting as "waiting for you"?',
  content: [
    { kind: "markdown", value: "**How long before a session counts as stale?** Drag until the line falls where you'd want it." },
    {
      kind: "sample",
      label: "Your open sessions, by time since last touch",
      source: "~/.agentui/sessions/*/state.json — updatedAt, read just now (11 open)",
      rule: "idle_h > stale_hours",
      verdict: ["marked stale", "stays listed"],
      field: "idle_h",
      unit: "h",
      rows: [
        { label: "muzzle-flash", idle_h: 0.33, note: "you were dragging this before lunch" },
        { label: "theme-preview", idle_h: 5.2 },
        { label: "shader params", idle_h: 6.8 },
        { label: "changelog approval", idle_h: 27 },
        { label: "last week's three.js demo", idle_h: 190 },
      ],
    },
  ],
  controls: [
    { id: "stale_hours", type: "slider", label: "Stale after", min: 1, max: 48, step: 1, default: 4, unit: "h" },
  ],
  preview: { auto: false },
};

const buildFirst = {
  title: "Which improvement first?",
  controls: [
    {
      id: "first",
      type: "compare",
      label: "Build first",
      axes: [
        { id: "covers", label: "Questions it converts", kind: "text" },
        { id: "cost", label: "Est. build", kind: "scale", scale: ["S", "M", "L"], better: "low" },
        { id: "schema", label: "New schema", kind: "text" },
        { id: "blocked", label: "Blocks anything else?", kind: "bool", better: "low" },
      ],
      options: [
        { value: "sample", label: "sample block", cells: { covers: "1, 2, 5, 10", cost: "M", schema: "1 block kind", blocked: false } },
        { value: "compare", label: "compare control", cells: { covers: "3, 5, 7", cost: "M", schema: "1 control, value stays string", blocked: false } },
        { value: "bind", label: "bind on content", cells: { covers: "6, 8", cost: "S", schema: "1 boolean", blocked: false } },
        { value: "multi", label: "multiselect", cells: { covers: "9", cost: "M", schema: "ControlValue gains string[]", blocked: true } },
      ],
      default: "sample",
    },
  ],
};

const grouping = {
  title: "Should non-previewable controls be grouped separately?",
  content: [
    { kind: "markdown", value: "**Same widget, two arrangements** — screenshots taken just now at 1280×800." },
    { kind: "image", path: "/abs/path/grouped-sunk.png", caption: "A — today" },
    { kind: "image", path: "/abs/path/grouped-labelled.png", caption: 'B — separate labelled section, "No live preview"' },
  ],
  actions: [{ id: "a", label: "Keep A", style: "primary" }, { id: "b", label: "Switch to B" }],
};

const libraryScope = {
  title: "Where should ui_save default to?",
  content: [
    { kind: "markdown", value: "**Which scope should saving default to?** Here is that claim replayed against what you actually saved." },
    {
      kind: "sample",
      label: "Your saved widgets, and where they were reused",
      source: "~/.agentui/library/index.json + .agent-ui/library/index.json — usedInProjects, read just now",
      rule: "needs != scope",
      verdict: ["would have been rebuilt from scratch", "found on reuse"],
      rows: [
        { label: "muzzle-flash", needs: "global", note: "saved in AgentUI, reused in rasterizer (3×)" },
        { label: "theme-probe", needs: "project", note: "saved and reused in AgentUI only" },
        { label: "shader-params", needs: "global", note: "saved in shaderbench, reused in rasterizer" },
        { label: "diff-approve", needs: "global", note: "reused in 4 repos" },
      ],
    },
  ],
  controls: [
    {
      id: "scope",
      type: "select",
      label: "ui_save defaults to",
      options: [
        { value: "global", label: "~/.agentui/library — every project" },
        { value: "project", label: ".agent-ui/library — this repo, travels if committed" },
      ],
      default: "global",
    },
  ],
  actions: [{ id: "use", label: "Use this default", style: "primary" }],
};

const packageName = {
  title: "Name for the MCP server package",
  content: [
    { kind: "markdown", value: "**What should this package be called?** The four places it actually appears update as you type." },
    {
      kind: "code",
      bind: true,
      language: "ts",
      filename: "packages/server/src/mcp.ts",
      value: 'import { uiAsk, uiWait } from "{{pkg}}";\nconst client = new {{pkg:pascal}}Client();',
    },
    { kind: "code", bind: true, language: "bash", value: "npx {{pkg}} install --client claude-code\n{{pkg}} --version\n{{pkg}} doctor" },
    {
      kind: "code",
      bind: true,
      language: "json",
      filename: ".mcp.json",
      value: '{\n  "mcpServers": {\n    "{{pkg}}": {\n      "command": "npx",\n      "args": ["-y", "{{pkg}}@latest", "serve"]\n    }\n  }\n}',
    },
    { kind: "code", bind: true, value: "packages/\n  {{pkg}}/\n  {{pkg}}-ui/\n  {{pkg}}-shared/\nnode_modules/.bin/{{pkg}}" },
  ],
  controls: [
    {
      id: "pkg",
      type: "text",
      label: "Package name",
      default: "agentui-mcp",
      description: "the npm name, the binary, and the key in .mcp.json",
    },
  ],
  actions: [{ id: "use_it", label: "Use this name", style: "primary" }],
};

const previewData = {
  title: "How should the preview frame get its data?",
  controls: [
    {
      id: "approach",
      type: "compare",
      label: "How should the preview frame get its data?",
      axes: [
        { id: "files", label: "Files touched", kind: "text" },
        { id: "wire", label: "New protocol message", kind: "bool", better: "low" },
        { id: "offline", label: "Works with no network", kind: "bool", better: "high" },
        { id: "risk", label: "Blast radius if wrong", kind: "scale", scale: ["one widget", "all previews", "the wake path"], better: "low" },
        { id: "call", label: "What the module writes", kind: "code" },
      ],
      options: [
        {
          value: "poll",
          label: "Poll from the frame",
          summary: "the module fetches its own data",
          cells: { files: "preview.ts, preview.js", wire: false, offline: false, risk: "all previews", call: "await fetch('/s/'+id+'/rows')" },
        },
        {
          value: "push",
          label: "Push in `init`",
          summary: "rows ride along with the spec",
          cells: {
            files: "preview-protocol.ts, PreviewFrame.tsx, session.ts",
            wire: true,
            offline: true,
            risk: "all previews",
            call: "setup({ spec, values, data })",
          },
        },
        {
          value: "inline",
          label: "Render in the shell",
          summary: "no frame involved",
          cells: { files: "ContentRenderer.tsx", wire: false, offline: true, risk: "one widget", call: "// nothing — it is a content block" },
        },
      ],
      default: "inline",
    },
  ],
};

const errorWording = {
  title: "Wording for the install-conflict error",
  content: [
    { kind: "markdown", value: "**Is this clear?** Edit the ✗ line; everything around it is the real output from `install/install.ts`." },
    {
      kind: "code",
      bind: true,
      language: "text",
      filename: "terminal, 80 cols",
      value:
        "$ agentui install --client cursor\n  ok  found Cursor at ~/.cursor/mcp.json\n  x   {{message}}\n{{hint}}\n\n  1 of 2 clients configured. Run `agentui doctor` to check.",
    },
  ],
  controls: [
    {
      id: "message",
      type: "text",
      multiline: true,
      label: "The error line",
      default: 'an "agentui" server is already registered — refusing to overwrite it',
    },
    {
      id: "hint",
      type: "select",
      label: "Follow-up line",
      options: [
        { value: "", label: "no follow-up" },
        { value: "      Re-run with --force to replace it.", label: "suggest --force" },
        { value: "      It points at /old/path/mcp.js — re-run with --force to replace.", label: "suggest --force, and say what's there" },
      ],
      default: "      Re-run with --force to replace it.",
    },
  ],
  actions: [{ id: "ship", label: "Use this", style: "primary" }, { id: "neither", label: "Neither — I'll say why" }],
};

/**
 * Deliberately NOT the plan's "which files should the waiters refactor touch".
 *
 * That question fails a test the plan did not state and should have: it can only
 * be answered by someone who will go and read the repo. Which files a refactor
 * touches is the agent's job to work out; what the person can answer is which
 * OUTCOMES are worth the time. Same control, same live total, a question the
 * reader is actually the right party for.
 */
const alsoDoThese = {
  title: "Also do these while I'm in here?",
  content: [
    { kind: "markdown", value: "**Anything else while I'm in this file?** Times are my estimate; unchecked things I leave exactly as they are." },
  ],
  controls: [
    {
      id: "extras",
      type: "multiselect",
      label: "Extras",
      weightUnit: "min",
      options: [
        { value: "contrast", label: "Fix the dark-mode contrast on buttons", note: "fails WCAG AA right now", weight: 5 },
        { value: "mobile", label: "Make the settings page usable on a phone", weight: 25 },
        { value: "loading", label: "Add a loading state to the export button", note: "it looks frozen for ~2s", weight: 8 },
        { value: "empty", label: "Write an empty state for the history list", weight: 12 },
        { value: "shortcuts", label: "Keyboard shortcuts for the toolbar", weight: 40 },
      ],
      default: ["contrast", "loading"],
    },
  ],
  actions: [{ id: "go", label: "Do these", style: "primary" }],
};

const retryBudget = {
  title: "Retry budget for the MCP fetch",
  content: [
    { kind: "markdown", value: "**How many retries?** What matters is how long we stall before giving up, against the 60 s tool timeout." },
    {
      kind: "sample",
      label: "What each retry count costs, worst case",
      source: "computed: sum of 500ms * 2^k for k in 0..n  (exponential backoff, no jitter)",
      rule: "n == retries",
      field: "stall_s",
      unit: "s",
      rows: [
        { n: 0, label: "no retry", stall_s: 0, note: "one request; a blip fails the call" },
        { n: 1, label: "1 retry", stall_s: 0.5 },
        { n: 2, label: "2 retries", stall_s: 1.5 },
        { n: 3, label: "3 retries", stall_s: 3.5, note: "covers a restart of the far side" },
        { n: 4, label: "4 retries", stall_s: 7.5 },
        { n: 5, label: "5 retries", stall_s: 15.5 },
        { n: 6, label: "6 retries", stall_s: 31.5 },
        { n: 7, label: "7 retries", stall_s: 63.5, note: "past the 60 s tool timeout — you get a confusing error instead of a retry" },
      ],
    },
  ],
  controls: [{ id: "retries", type: "slider", label: "Retries", min: 0, max: 7, step: 1, default: 3 }],
};

const TEN: Array<[string, unknown]> = [
  ["1. ui_close deletes the session dir?", sessionTtl],
  ["2. how long before stale?", staleness],
  ["3. which improvement first?", buildFirst],
  ["4. group non-previewable controls?", grouping],
  ["5. global or project library scope?", libraryScope],
  ["6. what should this package be called?", packageName],
  ["7. which of these three approaches?", previewData],
  ["8. is this error message clear?", errorWording],
  ["9. which extras are worth the time?", alsoDoThese],
  ["10. what retry count?", retryBudget],
];

describe("the ten questions, as documented", () => {
  for (const [label, spec] of TEN) {
    it(`${label} parses`, () => {
      expect(ok(spec, label).title.length).toBeGreaterThan(0);
    });
  }
});

describe("what the ten actually do", () => {
  it("draws the staleness line where the slider is, not where the agent guessed", () => {
    const spec = ok(staleness, "2");
    const block = spec.content[1] as SampleBlock;
    const at = (values: Record<string, number>) => evaluateSample(block, spec.controls, values)!;
    // At the agent's suggested 4h, four of five sessions are stale — including
    // theme-preview at 5.2h, which you were using this morning. Dragging to 6h
    // drops it back below the line. THAT movement is the answer, and it is not
    // in the number 4.
    expect(evaluateSample(block, spec.controls, defaultsFromSpec(spec))!.matchCount).toBe(4);
    const at6 = at({ stale_hours: 6 });
    expect(at6.matchCount).toBe(3);
    expect(at6.rows.find((r) => r.label === "theme-preview")?.matched).toBe(false);
    expect(at6.axis?.threshold).toBeCloseTo((6 - 1) / 47, 5);
    expect(at({ stale_hours: 27 }).matchCount).toBe(1);
  });

  it("shows the two sessions no setting can reach, instead of squashing the rest", () => {
    const spec = ok(staleness, "2");
    const block = spec.content[1] as SampleBlock;
    const rows = evaluateSample(block, spec.controls, { stale_hours: 6 })!.rows;
    expect(rows.filter((r) => r.beyond).map((r) => r.label)).toEqual(["muzzle-flash", "last week's three.js demo"]);
  });

  it("reclaims only what the TTL actually deletes", () => {
    const spec = ok(sessionTtl, "1");
    const block = spec.content[1] as SampleBlock;
    expect(evaluateSample(block, spec.controls, { keep_days: 7 })!.total?.text).toBe("4252");
    expect(evaluateSample(block, spec.controls, { keep_days: 0 })!.total?.text).toBe("4576");
  });

  it("turns 7 retries from a bigger number into a bug", () => {
    const spec = ok(retryBudget, "10");
    const block = spec.content[1] as SampleBlock;
    const at7 = evaluateSample(block, spec.controls, { retries: 7 })!;
    expect(at7.rows.filter((r) => r.matched).map((r) => r.text)).toEqual(["63.5"]);
    // And the bar for it is the full width of the plot, next to 3 retries at a
    // twentieth of that — the thing "7" alone cannot tell you.
    expect(at7.rows.at(-1)?.fraction).toBe(1);
  });

  it("declines the generated frame where the table already says everything", () => {
    // `keep_days` carries unit "d", so without preview.auto:false this would
    // spawn a pane drawing a bar next to a table that says more.
    expect(shouldAutoPreview(ok(sessionTtl, "1"))).toBe(false);
    expect(shouldAutoPreview(ok(staleness, "2"))).toBe(false);
    // And the ones with nothing magnitude-shaped never wanted one anyway.
    expect(shouldAutoPreview(ok(packageName, "6"))).toBe(false);
    expect(shouldAutoPreview(ok(previewData, "7"))).toBe(false);
    expect(shouldAutoPreview(ok(alsoDoThese, "9"))).toBe(false);
    expect(shouldAutoPreview(ok(retryBudget, "10"))).toBe(false);
  });
});
