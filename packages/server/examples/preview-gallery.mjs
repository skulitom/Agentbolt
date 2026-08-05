// Every generated-preview view at once, so a change to the view vocabulary can
// be judged by looking at it rather than by reading a diff.
//
//   node packages/server/examples/preview-gallery.mjs
//
// No previewModule: everything on screen is inferred by planPreview from the
// controls alone. The two motion groups are the point — one easing select used
// to claim the widget's only motion panel and every other one fell through to
// the state line, so a widget tuning both an enter and an exit transition could
// show you exactly one of them.
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

    { id: "retries", label: "Retries", type: "number", default: 3, group: "Behaviour" },
    { id: "verbose", label: "Verbose logging", type: "toggle", default: false, group: "Behaviour" },
  ],
};

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

setInterval(() => {}, 1 << 30);
