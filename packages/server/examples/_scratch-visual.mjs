// Scratch: one widget per primitive, held open so the renderers can be looked
// at. Not a dogfood example — delete after checking.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");

const client = new Client({ name: "scratch-visual", version: "0.0.0" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [path.resolve(here, "..", "dist", "mcp.js")],
    cwd: repoRoot,
    env: { ...process.env, AGENT_UI_NO_OPEN: "1", AGENT_UI_LOG_LEVEL: "warn" },
    stderr: "inherit",
  }),
);

const create = async (spec) => {
  const r = await client.callTool({ name: "ui_create", arguments: { spec, open: false } });
  if (r.isError) throw new Error(r.content?.[0]?.text ?? "failed");
  return r.structuredContent.url;
};

const urls = {};

urls.sample = await create({
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
    {
      kind: "sample",
      label: "What each retry count costs, worst case",
      source: "computed: sum of 500ms * 2^k for k in 0..n  (exponential backoff, no jitter)",
      rule: "n == retries",
      field: "stall_s",
      unit: "s",
      verdict: ["what you'd get", ""],
      rows: [
        { n: 0, label: "no retry", stall_s: 0, note: "one request; a blip fails the call" },
        { n: 1, label: "1 retry", stall_s: 0.5 },
        { n: 2, label: "2 retries", stall_s: 1.5 },
        { n: 3, label: "3 retries", stall_s: 3.5, note: "covers a restart of the far side" },
        { n: 5, label: "5 retries", stall_s: 15.5 },
        { n: 7, label: "7 retries", stall_s: 63.5, note: "past the 60 s tool timeout — you get a confusing error instead of a retry" },
      ],
    },
  ],
  controls: [
    { id: "stale_hours", type: "slider", label: "Stale after", min: 1, max: 48, step: 1, default: 4, unit: "h" },
    { id: "retries", type: "slider", label: "Retries", min: 0, max: 7, step: 1, default: 3 },
  ],
  preview: { auto: false },
});

urls.compare = await create({
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
        { value: "poll", label: "Poll from the frame", summary: "the module fetches its own data", cells: { files: "preview.ts, preview.js", wire: false, offline: false, risk: "all previews", call: "await fetch('/s/'+id+'/rows')" } },
        { value: "push", label: "Push in `init`", summary: "rows ride along with the spec", cells: { files: "preview-protocol.ts, PreviewFrame.tsx, session.ts", wire: true, offline: true, risk: "the wake path", call: "setup({ spec, values, data })" } },
        { value: "inline", label: "Render in the shell", summary: "no frame involved", cells: { files: "ContentRenderer.tsx", wire: false, offline: true, risk: "one widget", call: "// nothing — it is a content block" } },
      ],
      default: "inline",
    },
  ],
  actions: [{ id: "go", label: "Use this", style: "primary" }],
});

urls.multiselect = await create({
  title: "Also do these while I'm in here?",
  content: [{ kind: "markdown", value: "**Anything else while I'm in this file?** Times are my estimate; unchecked things I'll leave exactly as they are." }],
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
});

process.stderr.write(`\n${JSON.stringify(urls, null, 1)}\n\n`);
setInterval(() => {}, 1 << 30);
