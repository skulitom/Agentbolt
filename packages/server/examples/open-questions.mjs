// Real open questions about AgentUI, asked through AgentUI.
//
// The point of this example is the SHAPE, not the content: it calls ui_ask
// WITHOUT wait, gets a sessionId back in milliseconds, and exits. The agent goes
// off and does other work; the answers are collected later with ui_wait or
// ui_get_state against that id. Blocking on a human is the thing this tool
// exists to avoid.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const serverEntry = path.resolve(here, "..", "dist", "mcp.js");
const repoRoot = path.resolve(here, "..", "..", "..");

const client = new Client({ name: "agentui-open-questions", version: "0.0.0" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [serverEntry],
    cwd: repoRoot,
    env: { ...process.env, AGENT_UI_NO_OPEN: "1", AGENT_UI_LOG_LEVEL: "warn" },
    stderr: "inherit",
  }),
);

const started = Date.now();

const asked = await client.callTool({
  name: "ui_ask",
  arguments: {
    question: "Three decisions I'd rather not make for you",
    details:
      "None of these block me — I'm carrying on with the widget library while you think. " +
      "Anything you leave alone stays marked as my suggestion rather than your decision.",
    open: false,
    fields: [
      {
        id: "preview_helpers",
        label: "Built-in preview helpers",
        type: "select",
        default: "some",
        description:
          "Right now every preview is hand-written DOM. Helpers (swatch grid, spacing ruler, type scale) would give common widgets live feedback for free — at the cost of magic you can't see.",
        options: [
          { value: "some", label: "A few obvious ones, everything else hand-written" },
          { value: "none", label: "None — keep it explicit, agents write their own" },
          { value: "many", label: "A rich set — most widgets should never write DOM" },
        ],
      },
      {
        id: "nonvisual_controls",
        label: "Controls with nothing to preview",
        type: "select",
        default: "plain",
        description:
          "Things like a debug toggle have no meaningful visual. My view: don't force them into a preview.",
        options: [
          { value: "plain", label: "Leave them plain — not everything needs a preview" },
          { value: "group", label: "Group them separately, under the previewable ones" },
          { value: "force", label: "Always show something, even if it's just a state label" },
        ],
      },
      {
        id: "stale_sessions",
        label: "Widgets left over from earlier runs",
        type: "select",
        default: "age_out",
        description:
          "Old sessions come back as 'active' on restart and pile up in the dashboard. A genuinely resumable one must still work.",
        options: [
          { value: "age_out", label: "Auto-close anything older than a few hours" },
          { value: "group", label: "Keep them, grouped under 'from earlier runs'" },
          { value: "keep", label: "Leave as-is, I'll close them myself" },
        ],
      },
    ],
    options: [
      { label: "Save these", style: "primary" },
      { label: "Use your judgement", style: "default" },
    ],
  },
});

const elapsed = Date.now() - started;
const out = asked.structuredContent;

process.stderr.write(
  [
    "",
    `  ui_ask returned in ${elapsed}ms without blocking`,
    `  status      ${out.status}   answered: ${out.answered}`,
    `  QUESTIONS   ${out.url}`,
    `  sessionId   ${out.sessionId}`,
    "",
    "  The agent is now free. Collect later with:",
    `    ui_wait({ sessionId: "${out.sessionId}" })`,
    "",
  ].join("\n"),
);

// Deliberately does NOT wait for an answer.
//
// The process stays alive only because the MCP server is its child and the
// server owns the session — in a real run that server lives as long as the
// agent's session, which is precisely why the agent can ask and walk away.
// Nothing here is blocked on the human: the question is live, and this holder
// is idle.
setInterval(() => {}, 1 << 30);
