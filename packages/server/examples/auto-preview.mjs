// A widget with NO previewModule at all.
//
// This is the payoff of the built-in helpers: an agent writes a plain spec, and
// the human still gets live visual feedback, because a control you cannot see
// the effect of is worse than a text question. Anything with a magnitude gets a
// visual; the toggle and the select get a compact state line instead of being
// forced into a picture that would say nothing.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");

const client = new Client({ name: "agentui-autopreview", version: "0.0.0" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [path.resolve(here, "..", "dist", "mcp.js")],
    cwd: repoRoot,
    env: { ...process.env, AGENT_UI_NO_OPEN: "1", AGENT_UI_LOG_LEVEL: "warn" },
    stderr: "inherit",
  }),
);

const created = await client.callTool({
  name: "ui_create",
  arguments: {
    open: false,
    // Note: no previewModule.
    spec: {
      title: "Toast notifications",
      description: "No preview module was written for this widget — the preview below is generated.",
      preview: { height: 380 },
      controls: [
        { id: "background", label: "Background", type: "color", default: "#1d9e75", group: "Look" },
        { id: "accent_bar", label: "Accent bar", type: "color", default: "#0f6e56", group: "Look" },
        { id: "corner_radius", label: "Corner radius", type: "slider", min: 0, max: 24, step: 1, default: 10, unit: "px", group: "Look" },
        { id: "text_size", label: "Text size", type: "slider", min: 11, max: 20, step: 0.5, default: 14, unit: "px", group: "Look" },
        { id: "visible_duration", label: "Visible for", type: "slider", min: 500, max: 8000, step: 100, default: 4000, unit: "ms", group: "Timing" },
        { id: "slide_distance", label: "Slide distance", type: "slider", min: 0, max: 120, step: 4, default: 40, unit: "px", group: "Timing" },
        { id: "pause_on_hover", label: "Pause on hover", type: "toggle", default: true, group: "Behaviour" },
        { id: "position", label: "Position", type: "select", default: "bottom-right", group: "Behaviour",
          options: [{ value: "top-right" }, { value: "bottom-right" }, { value: "bottom-centre" }] },
      ],
    },
  },
});

const { url, sessionId } = created.structuredContent;
process.stderr.write(`\n  NO previewModule was supplied.\n  ${url}\n  sessionId ${sessionId}\n\n`);

setInterval(() => {}, 1 << 30);
