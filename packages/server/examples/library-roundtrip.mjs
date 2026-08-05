// The widget library, exercised end to end.
//
// Saves the shell-tuning widget, reuses it by name, and then deliberately
// hand-authors a near-duplicate to prove the reuse hint fires at exactly the
// moment an agent would otherwise rebuild something it already has.
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");
const previewModule = fs.readFileSync(path.join(here, "theme-preview.mjs"), "utf8");

const client = new Client({ name: "agentui-library-demo", version: "0.0.0" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [path.resolve(here, "..", "dist", "mcp.js")],
    cwd: repoRoot,
    env: { ...process.env, AGENT_UI_NO_OPEN: "1", AGENT_UI_LOG_LEVEL: "warn" },
    stderr: "inherit",
  }),
);

const out = (label, result) =>
  process.stderr.write(`\n── ${label} ──\n${result.content?.[0]?.text ?? "(no text)"}\n`);

const themeSpec = {
  title: "AgentUI shell — visual tuning",
  preview: { height: 340 },
  controls: [
    { id: "accent", label: "Accent colour", type: "color", default: "#185fa5", group: "Colour" },
    { id: "density", label: "Density", type: "select", default: "comfortable", group: "Layout",
      options: [{ value: "compact" }, { value: "comfortable" }, { value: "roomy" }] },
    { id: "content_width", label: "Content width", type: "slider", min: 560, max: 1100, step: 20, default: 760, unit: "px", group: "Layout" },
    { id: "corner_radius", label: "Corner radius", type: "slider", min: 0, max: 16, step: 1, default: 7, unit: "px", group: "Layout" },
    { id: "base_font_size", label: "Base font size", type: "slider", min: 13, max: 17, step: 0.5, default: 15, unit: "px", group: "Layout" },
  ],
};

// 1 — create, adjust as if the human had, then save.
const created = await client.callTool({ name: "ui_create", arguments: { spec: themeSpec, previewModule, open: false } });
const sessionId = created.structuredContent.sessionId;

await client.callTool({
  name: "ui_update",
  arguments: { sessionId, values: { content_width: 1100, corner_radius: 16, base_font_size: 14, density: "roomy" } },
});

out(
  "ui_save",
  await client.callTool({
    name: "ui_save",
    arguments: {
      sessionId,
      name: "Shell theme",
      slug: "shell-theme",
      description: "Tune the widget shell: accent, density, width, radius, type size",
      tags: ["ui", "theme"],
      triggers: ["shell theme", "widget styling", "how widgets look"],
      overwrite: true,
    },
  }),
);

// 2 — find it the way an agent would.
out("ui_library({ query: 'how widgets look' })",
  await client.callTool({ name: "ui_library", arguments: { query: "how widgets look" } }));

// 3 — the anti-forgetting hint: author a near-duplicate from scratch.
out(
  "ui_create with a hand-authored near-duplicate",
  await client.callTool({
    name: "ui_create",
    arguments: {
      open: false,
      spec: {
        title: "Widget appearance settings",
        controls: themeSpec.controls.map((c) => ({ ...c })),
      },
    },
  }),
);

// 4 — reuse by name; it should reopen at the values left above, not the defaults.
const reused = await client.callTool({ name: "ui_create", arguments: { template: "shell-theme", open: false } });
out("ui_create({ template: 'shell-theme' })", reused);
process.stderr.write(
  `\n  reopened at: ${JSON.stringify(reused.structuredContent.values)}\n` +
    `  (authored defaults were width 760, radius 7, font 15, density comfortable)\n\n`,
);

setInterval(() => {}, 1 << 30);
