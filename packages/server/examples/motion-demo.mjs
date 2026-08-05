// A real AgentUI widget about a real AgentUI decision — this one tunes the
// shell's own motion tokens (--t-fast / --t-quick / --t-gentle and the easing),
// introduced in the same change that added the motion utilities the preview
// runs on. Building the tool and using the tool are the same activity.
//
// It also dogfoods the animate-on-push path: ~12s in, the script pushes a
// value with ui_update. In the shell the control card flashes; in the preview
// the readout GLIDES to the new number (meta.source === "agent"), while your
// own drags always track 1:1.
//
//   node packages/server/examples/motion-demo.mjs
//
// Whatever you send back belongs in packages/ui/src/styles.css (and mirrored
// in packages/ui/public/agentui/motion.js — see the header there).
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const serverEntry = path.resolve(here, "..", "dist", "mcp.js");
const repoRoot = path.resolve(here, "..", "..", "..");

const previewModule = fs.readFileSync(path.join(here, "motion-preview.mjs"), "utf8");

const spec = {
  title: "AgentUI shell — motion tuning",
  description: "The preview replays a press, a hover and a reveal with your values. Judge by feel, not by the numbers.",
  preview: { height: 420 },
  content: [
    {
      kind: "markdown",
      value: "**How should the shell move?** Three durations, one easing — every transition in the shell derives from them.",
    },
  ],
  controls: [
    {
      id: "press_duration",
      label: "Press feedback",
      type: "slider",
      min: 40,
      max: 200,
      step: 10,
      default: 90,
      unit: "ms",
      description: "Button press/release. Too slow reads as lag, not response.",
      group: "Durations",
    },
    {
      id: "hover_duration",
      label: "Hover states",
      type: "slider",
      min: 80,
      max: 300,
      step: 10,
      default: 140,
      unit: "ms",
      group: "Durations",
    },
    {
      id: "reveal_duration",
      label: "Reveals and settles",
      type: "slider",
      min: 120,
      max: 400,
      step: 10,
      default: 220,
      unit: "ms",
      description: "Action buttons appearing, status changes, the spec-update fade.",
      group: "Durations",
    },
    {
      id: "easing",
      label: "Easing",
      type: "select",
      default: "ease-out",
      group: "Character",
      options: [
        { value: "linear", label: "linear — mechanical" },
        { value: "ease-out", label: "ease-out — fast start, soft landing" },
        { value: "ease-in-out", label: "ease-in-out — symmetric" },
        { value: "ease-out-back", label: "ease-out-back — a little overshoot" },
      ],
    },
  ],
};

const client = new Client({ name: "agentui-motion-demo", version: "0.0.0" });
const transport = new StdioClientTransport({
  command: process.execPath,
  args: [serverEntry],
  cwd: repoRoot,
  env: { ...process.env, AGENT_UI_NO_OPEN: "1", AGENT_UI_LOG_LEVEL: "warn" },
  stderr: "inherit",
});

await client.connect(transport);

const created = await client.callTool({ name: "ui_create", arguments: { spec, previewModule, open: false } });
if (created.isError) {
  process.stderr.write(`${created.content?.[0]?.text}\n`);
  process.exit(1);
}

const { url, sessionId } = created.structuredContent;
process.stderr.write(`\n  MOTION DEMO READY  ${url}\n  sessionId          ${sessionId}\n\n`);

// The scripted push. Watch the "Reveals and settles" card flash in the shell
// and the preview readout glide — that is meta.source === "agent" at work.
let finished = false;
setTimeout(async () => {
  if (finished) return;
  const pushed = await client.callTool({
    name: "ui_update",
    arguments: { sessionId, values: { reveal_duration: 320 } },
  });
  if (!pushed.isError) {
    process.stderr.write("  -> pushed reveal_duration: 320 (agent-sourced — it should animate, not snap)\n");
  }
}, 12_000);

let cursor = created.structuredContent.lastEventSeq ?? 0;
setInterval(async () => {
  const state = await client.callTool({ name: "ui_get_state", arguments: { sessionId, sinceSeq: cursor } });
  const body = state.structuredContent;
  if (!body?.session) return;
  cursor = body.session.lastEventSeq;
  for (const event of body.events ?? []) {
    if (event.type === "values_interim") continue; // never wakes an agent; never printed
    process.stderr.write(`  <- ${event.type} ${JSON.stringify(event.payload ?? {}).slice(0, 200)}\n`);
  }
  if (body.output) {
    finished = true;
    process.stderr.write(`\n  FINALIZED ${JSON.stringify(body.output.values, null, 2)}\n`);
    process.stderr.write("  Apply: --t-fast/--t-quick/--t-gentle in packages/ui/src/styles.css,\n");
    process.stderr.write("  and keep packages/ui/public/agentui/motion.js `durations` in step.\n\n");
  }
}, 1500);
