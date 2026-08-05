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

// q1 and q2 are the two questions this tool was worst at, asked the way the
// skill now says to ask them.
//
// The old q1 was "Keep files / Delete them" — two buttons over a policy nobody
// can hold in their head. The old q2 was a bare 1..48h slider, which drew a bar
// that was 100% wide at every value. Both are the same mistake: asking about a
// number instead of about the things the number applies to. Both become a
// `sample`: the rule, replayed against rows on this machine, with the line
// moving as you drag. The rows below are ILLUSTRATIVE and say so in `source` —
// a probe cannot read your sessions, and inventing them silently is exactly the
// failure the field exists to make visible.

// First question opens a tab (nothing is open yet). The probe then keeps the
// inbox "open" the way a real user would, and the next two must NOT get tabs.
await ask("q1: a judgement call", {
  question: "Should ui_close delete the session directory?",
  details:
    '"Always" and "never" are the ends of one dial: *delete when the session is older than N days*. ' +
    "`ui_get_state` returns nothing afterwards; there is no undo.",
  content: [
    {
      kind: "sample",
      label: "Session directories on this machine",
      source: "illustrative — a probe cannot read your ~/.agentui/sessions",
      rule: "age_d > keep_days",
      verdict: ["deleted on close", "kept"],
      field: "age_d",
      unit: "d",
      total: { field: "kb", unit: " kB", label: "reclaimed" },
      rows: [
        { label: "muzzle-flash", age_d: 0.01, kb: 96, note: "you adjusted this 20 min ago" },
        { label: "theme-preview", age_d: 0.2, kb: 140 },
        { label: "changelog approval", age_d: 1.1, kb: 88 },
        { label: "three.js demo", age_d: 8, kb: 4180, note: "holds assets/shot.png" },
        { label: "usability probe", age_d: 23, kb: 72 },
      ],
    },
  ],
  fields: [{ id: "keep_days", type: "slider", label: "Keep for", min: 0, max: 30, step: 1, default: 7, unit: "d" }],
  options: ["Use this TTL", "Never delete", "Always delete on close"],
  // keep_days carries a unit, so without this a bar would spawn beside a table
  // that already says more than the bar could.
  preview: { auto: false },
});
await simulateInboxOpen(new URL(timings[0].url).origin);

await ask("q2: a numeric constant", {
  question: 'When does a session stop counting as "waiting for you"?',
  details:
    "Staleness only changes presentation — a stale session is still open and still answerable (`core/staleness.ts`). " +
    "Drag until the line falls where you'd want it.",
  content: [
    {
      kind: "sample",
      label: "Your open sessions, by time since last touch",
      source: "illustrative — replace with `stat` on your own sessions dir",
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
  ],
  fields: [{ id: "stale_hours", type: "slider", label: "Stale after", min: 1, max: 48, step: 1, default: 4, unit: "h" }],
  preview: { auto: false },
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
