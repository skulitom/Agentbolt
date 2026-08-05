// Usability probe: exercise the tool the way an agent actually would over the
// course of a task, and measure the friction rather than guess at it.
//
// The question being answered: when an agent has something to ask, is reaching
// for this cheaper than typing a sentence in chat? If not, no amount of polish
// will make it get used.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..", "..");

const client = new Client({ name: "usability-probe", version: "0.0.0" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [path.resolve(here, "..", "dist", "mcp.js")],
    cwd: repoRoot,
    // Take the real open/do-not-open decision, without launching browsers.
    env: { ...process.env, AGENT_UI_NO_OPEN: "0", AGENT_UI_OPEN_DRY_RUN: "1", AGENT_UI_LOG_LEVEL: "error" },
    stderr: "inherit",
  }),
);

const urls = new Set();
const timings = [];

// `open: true` is the real default; the probe measures what actually happens to
// the user, not a sanitised path.
async function ask(label, args) {
  const started = Date.now();
  const r = await client.callTool({ name: "ui_ask", arguments: { ...args } });
  const ms = Date.now() - started;
  if (r.isError) {
    timings.push({ label, ms, error: r.content?.[0]?.text?.slice(0, 120) });
    return;
  }
  urls.add(r.structuredContent.url);
  timings.push({ label, ms, url: r.structuredContent.url, note: r.content?.[0]?.text?.match(/\(([^)]*)\)/)?.[1] });
}

/** Pretend the user has AgentUI open, the way the inbox does by polling. */
async function simulateInboxOpen(origin) {
  await fetch(`${origin}/api/sessions`).catch(() => {});
}

// First question opens a tab (nothing is open yet). The probe then keeps the
// inbox "open" the way a real user would, and the next two must NOT get tabs.
await ask("q1: a judgement call", {
  question: "Should ui_close also delete the session directory by default?",
  options: ["Keep files", "Delete them"],
});
await simulateInboxOpen(new URL(timings[0].url).origin);

await ask("q2: a numeric constant", {
  question: "How long before a session counts as stale?",
  fields: [{ id: "hours", type: "slider", min: 1, max: 48, default: 6, unit: "h" }],
});
await ask("q3: approve a change", {
  question: "Apply this to the changelog?",
  content: [{ kind: "diff", value: "@@ -1 +1,2 @@\n # Changelog\n+- library search" }],
  options: ["Apply", "Skip"],
});

const listed = await client.callTool({ name: "ui_get_state", arguments: {} });
const sessions = listed.structuredContent.sessions ?? [];

console.log(
  JSON.stringify(
    {
      questionsAsked: timings.length,
      tabsForcedOnTheUser: timings.filter((t) => t.note === "opened in the browser").length,
      distinctSessionUrls: urls.size,
      openSessions: sessions.filter((s) => s.status === "active").length,
      timings: timings.map((t) => `${t.label}: ${t.ms}ms${t.error ? " ERROR " + t.error : ""}`),
      urls: [...urls],
    },
    null,
    1,
  ),
);

process.exit(0);
