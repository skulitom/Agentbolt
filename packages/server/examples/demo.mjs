// A real AgentUI widget about a real AgentUI decision.
//
// Examples in this repo are dogfood: each one asks something we genuinely need
// answered, so building the tool and using the tool are the same activity and
// the demos double as real test cases.
//
// This one tunes the shell's own visual tokens. Whatever you send back gets
// applied to packages/ui/src/styles.css.
//
//   node packages/server/examples/demo.mjs
//
// Ctrl-C ends it (the server dies with this process, exactly as it does when an
// agent exits).
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const serverEntry = path.resolve(here, "..", "dist", "mcp.js");
const repoRoot = path.resolve(here, "..", "..", "..");

const previewModule = fs.readFileSync(path.join(here, "theme-preview.mjs"), "utf8");

const spec = {
  title: "AgentUI shell — visual tuning",
  description: "The preview below is a real widget rendered with your settings. Judge it by that.",
  preview: { height: 340 },
  content: [
    {
      kind: "markdown",
      value: "**How should widgets look?** Move anything — the preview updates as you drag.",
    },
  ],
  controls: [
    {
      id: "accent",
      label: "Accent colour",
      type: "color",
      default: "#185fa5",
      description: "Primary buttons, focus rings, the “agent is waiting” pill.",
      group: "Colour",
    },
    {
      id: "density",
      label: "Density",
      type: "select",
      default: "comfortable",
      group: "Layout",
      options: [
        { value: "compact", label: "Compact — more on screen" },
        { value: "comfortable", label: "Comfortable — current" },
        { value: "roomy", label: "Roomy — easier to hit" },
      ],
    },
    {
      id: "content_width",
      label: "Content width",
      type: "slider",
      min: 560,
      max: 1100,
      step: 20,
      default: 760,
      unit: "px",
      group: "Layout",
      description: "How wide the widget column runs before it stops growing.",
    },
    {
      id: "corner_radius",
      label: "Corner radius",
      type: "slider",
      min: 0,
      max: 16,
      step: 1,
      default: 7,
      unit: "px",
      group: "Layout",
    },
    {
      id: "base_font_size",
      label: "Base font size",
      type: "slider",
      min: 13,
      max: 17,
      step: 0.5,
      default: 15,
      unit: "px",
      group: "Layout",
    },
    {
      id: "show_seq",
      label: "Show the event cursor in the footer",
      type: "toggle",
      default: false,
      group: "Behaviour",
      description: "Useful when debugging the wait loop; noise the rest of the time.",
    },
    {
      id: "notes",
      label: "Anything else about how this reads",
      type: "text",
      multiline: true,
      default: "",
      placeholder: "e.g. the diff block is too loud, groups need more separation",
      group: "Behaviour",
    },
  ],
};

const client = new Client({ name: "agentui-demo", version: "0.0.0" });
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
process.stderr.write(`\n  DEMO READY  ${url}\n  sessionId   ${sessionId}\n\n`);

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
    process.stderr.write(`\n  FINALIZED ${JSON.stringify(body.output.values, null, 2)}\n`);
    process.stderr.write(`  comments: ${JSON.stringify(body.output.comments)}\n\n`);
  }
}, 1500);
