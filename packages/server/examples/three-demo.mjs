// Proves two things end to end:
//
//  1. Vendored three.js works inside the sandboxed cross-origin preview frame,
//     offline, via the nonced import map.
//  2. A BROKEN preview module surfaces its stack trace to the agent through the
//     event log, and can be fixed with ui_update without a page reload — the
//     loop that makes agent-authored preview code practical rather than a
//     one-shot gamble.
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");
const good = fs.readFileSync(path.join(here, "three-preview.mjs"), "utf8");

// A realistic mistake, not a syntax error: a typo'd import that only fails at runtime.
const broken = good.replace("mesh.material;", "mesh.materal;");

const client = new Client({ name: "agentui-three-demo", version: "0.0.0" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [path.resolve(here, "..", "dist", "mcp.js")],
    cwd: repoRoot,
    env: { ...process.env, AGENT_UI_NO_OPEN: "1", AGENT_UI_LOG_LEVEL: "warn" },
    stderr: "inherit",
  }),
);

const spec = {
  title: "Material and lighting",
  description: "Vendored three.js, running offline in the sandboxed preview frame.",
  preview: { height: 360 },
  controls: [
    { id: "base_colour", label: "Base colour", type: "color", default: "#b0367a", group: "Material" },
    { id: "roughness", label: "Roughness", type: "slider", min: 0, max: 1, step: 0.01, default: 0.35, group: "Material" },
    { id: "metalness", label: "Metalness", type: "slider", min: 0, max: 1, step: 0.01, default: 0.6, group: "Material" },
    { id: "emissive_colour", label: "Emissive", type: "color", default: "#000000", group: "Material" },
    { id: "emissive_strength", label: "Emissive strength", type: "slider", min: 0, max: 3, step: 0.05, default: 0, group: "Material" },
    { id: "light_angle", label: "Key light angle", type: "slider", min: 0, max: 360, step: 1, default: 45, unit: "°", group: "Lighting" },
    { id: "light_height", label: "Key light height", type: "slider", min: -3, max: 6, step: 0.1, default: 2, group: "Lighting" },
    { id: "light_strength", label: "Key light strength", type: "slider", min: 0, max: 8, step: 0.1, default: 3, group: "Lighting" },
  ],
};

const created = await client.callTool({
  name: "ui_create",
  arguments: { spec, previewModule: good, open: false },
});
const { sessionId, url } = created.structuredContent;
process.stderr.write(`\n  three.js widget: ${url}\n  sessionId ${sessionId}\n`);

const mode = process.argv[2];

if (mode === "--break") {
  await new Promise((r) => setTimeout(r, 1500));
  await client.callTool({ name: "ui_update", arguments: { sessionId, previewModule: broken } });
  process.stderr.write("\n  pushed a BROKEN module — watch the banner, and the event log below\n");

  // Poll for the error the way an agent would.
  let cursor = 0;
  for (let i = 0; i < 12; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    const state = await client.callTool({ name: "ui_get_state", arguments: { sessionId, sinceSeq: cursor } });
    const body = state.structuredContent;
    cursor = body.session.lastEventSeq;
    for (const event of body.events ?? []) {
      if (event.type === "preview_error") {
        process.stderr.write(`\n  AGENT SAW THE ERROR: ${event.payload.message}\n`);
        await client.callTool({ name: "ui_update", arguments: { sessionId, previewModule: good } });
        process.stderr.write("  pushed the fix — the frame reloads without a page refresh\n\n");
        i = 99;
        break;
      }
      if (event.type === "preview_event") {
        process.stderr.write(`  preview reported: ${JSON.stringify(event.payload.data)}\n`);
      }
    }
  }
}

setInterval(() => {}, 1 << 30);
