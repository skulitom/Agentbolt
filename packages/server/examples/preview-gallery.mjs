// Every generated-preview view at once, so a change to the view vocabulary can
// be judged by looking at it rather than by reading a diff.
//
//   node packages/server/examples/preview-gallery.mjs
//
// No previewModule: every pane is a built-in view. The arrangement is declared
// through `preview.views` — the middle ground between accepting inference and
// writing a hundred lines of DOM — and the two motion groups are the point, in
// that one easing select used to claim the widget's only motion panel while
// every other one fell through to the state line.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const serverEntry = path.resolve(here, "..", "dist", "mcp.js");
const repoRoot = path.resolve(here, "..", "..", "..");

const spec = {
  title: "Preview gallery",
  description: "Every built-in view, inferred from the controls with no preview module.",
  preview: { height: 560 },
  content: [{ kind: "markdown", value: "**Nothing here is hand-drawn.** The panes are inferred from the controls." }],
  controls: [
    { id: "enter_easing", label: "Enter easing", type: "select", default: "ease-out", group: "Enter",
      options: [{ value: "linear" }, { value: "ease-out" }, { value: "spring" }, { value: "ease-out-back" }] },
    { id: "enter_duration", label: "Enter duration", type: "slider", min: 80, max: 600, step: 10, default: 220, unit: "ms", group: "Enter" },
    { id: "enter_delay", label: "Enter delay", type: "slider", min: 0, max: 300, step: 10, default: 0, unit: "ms", group: "Enter" },

    { id: "exit_easing", label: "Exit easing", type: "select", default: "ease-in-out", group: "Exit",
      options: [{ value: "linear" }, { value: "ease-in-out" }, { value: "ease-out" }] },
    { id: "exit_duration", label: "Exit duration", type: "slider", min: 40, max: 400, step: 10, default: 140, unit: "ms", group: "Exit" },

    { id: "accent", label: "Accent", type: "color", default: "#ef9f27", group: "Colour" },
    { id: "surface", label: "Surface", type: "color", default: "#1c1c1a", group: "Colour" },

    { id: "corner_radius", label: "Corner radius", type: "slider", min: 0, max: 32, step: 1, default: 8, unit: "px", group: "Shape" },
    { id: "gap", label: "Gap", type: "slider", min: 0, max: 40, step: 2, default: 12, unit: "px", group: "Shape" },

    { id: "font_size", label: "Body size", type: "slider", min: 11, max: 22, step: 0.5, default: 15, unit: "px", group: "Type" },

    { id: "origin", label: "Origin", type: "vector", size: 2, min: 0, max: 1, default: [0.35, 0.7], group: "Layout" },

    { id: "ramp_from", label: "Ramp from", type: "color", default: "#ef9f27", group: "Ramp" },
    { id: "ramp_to", label: "Ramp to", type: "color", default: "#b0367a", group: "Ramp" },

    { id: "pad_all", label: "Padding", type: "slider", min: 0, max: 40, step: 1, default: 14, unit: "px", group: "Box" },
    { id: "border_width", label: "Border", type: "slider", min: 0, max: 8, step: 1, default: 1, unit: "px", group: "Box" },

    { id: "shadow_y", label: "Shadow offset", type: "slider", min: 0, max: 40, step: 1, default: 6, unit: "px", group: "Elevation" },
    { id: "shadow_blur", label: "Shadow blur", type: "slider", min: 0, max: 80, step: 1, default: 18, unit: "px", group: "Elevation" },
    { id: "shadow_tint", label: "Shadow tint", type: "color", default: "#241a05", group: "Elevation" },

    { id: "retries", label: "Retries", type: "number", default: 3, group: "Behaviour" },
    { id: "verbose", label: "Verbose logging", type: "toggle", default: false, group: "Behaviour" },
  ],
};

// Declared, because gradient/box/shadow are statements of intent: which colours
// form a ramp, and which length is "the padding", are not things to guess at.
spec.preview.views = [
  { view: "motion", controls: ["enter_easing", "enter_duration", "enter_delay"], title: "Enter" },
  { view: "motion", controls: ["exit_easing", "exit_duration"], title: "Exit" },
  { view: "swatch", controls: ["accent", "surface"] },
  { view: "gradient", controls: ["ramp_from", "ramp_to"] },
  { view: "ruler", controls: ["corner_radius", "gap"] },
  { view: "type", controls: ["font_size"] },
  { view: "pad", controls: ["origin"] },
  { view: "box", controls: ["pad_all", "corner_radius", "border_width"] },
  { view: "shadow", controls: ["shadow_y", "shadow_blur", "shadow_tint"] },
  { view: "card", controls: ["accent", "corner_radius", "font_size"] },
  { view: "state", controls: ["retries", "verbose"] },
];

const client = new Client({ name: "agentui-preview-gallery", version: "0.0.0" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [serverEntry],
    cwd: repoRoot,
    env: { ...process.env, AGENT_UI_LOG_LEVEL: "warn" },
    stderr: "inherit",
  }),
);

const created = await client.callTool({ name: "ui_create", arguments: { spec, open: process.env["AGENT_UI_OPEN"] !== "0" } });
if (created.isError) {
  process.stderr.write(`${created.content?.[0]?.text}\n`);
  process.exit(1);
}

const { url, sessionId } = created.structuredContent;
process.stderr.write(
  `\n  PREVIEW GALLERY  ${url}\n  sessionId        ${sessionId}\n\n` +
    "  Expect, top to bottom: two motion replays, colour swatches,\n" +
    "  a size ruler, a type scale, a sample card, and a settings line.\n\n",
);

// Dogfoods the animate-on-push path across every view at once: ~10s in, the
// script pushes values with ui_update. Each pane should GLIDE to the new value
// (meta.source === "agent") — the pad's dot travels, the ramp crossfades, the
// box reflows, the shadow deepens — while your own drags always track 1:1.
setTimeout(async () => {
  const pushed = await client.callTool({
    name: "ui_update",
    arguments: {
      sessionId,
      values: {
        origin: [0.82, 0.18],
        ramp_to: "#2f6fdb",
        pad_all: 30,
        corner_radius: 22,
        font_size: 20,
        shadow_y: 20,
        shadow_blur: 55,
        enter_duration: 420,
      },
    },
  });
  const note = pushed.isError
    ? `  push failed: ${pushed.content?.[0]?.text}`
    : "  -> pushed a value into every view (agent-sourced — they should glide, not snap)";
  process.stderr.write(`${note}\n`);
}, 10_000);

setInterval(() => {}, 1 << 30);
