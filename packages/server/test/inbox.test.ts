import { afterAll, beforeAll, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import fsp from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

/**
 * The economics of asking.
 *
 * Asking costs the agent milliseconds and the human a context switch, so three
 * small questions used to mean three browser tabs — at which point typing them
 * into chat is the better trade and the tool goes unused. These tests pin the
 * two changes that fix that: one tab regardless of how many questions, and
 * one-click answers resolvable without leaving it.
 */

const here = path.dirname(fileURLToPath(import.meta.url));
let client: Client;
let dataDir: string;
let origin: string;

beforeAll(async () => {
  dataDir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-inbox-"));
  client = new Client({ name: "inbox-test", version: "0.0.0" });
  await client.connect(
    new StdioClientTransport({
      command: process.execPath,
      args: [path.resolve(here, "..", "dist", "mcp.js")],
      env: {
        ...(process.env as Record<string, string>),
        AGENT_UI_DATA_DIR: dataDir,
        AGENT_UI_NO_OPEN: "0",
        // Take the real open/don't-open decision without launching browsers.
        AGENT_UI_OPEN_DRY_RUN: "1",
        AGENT_UI_LOG_LEVEL: "silent",
      },
      stderr: "pipe",
    }),
  );
}, 60_000);

afterAll(async () => {
  await client?.close().catch(() => {});
  await fsp.rm(dataDir, { recursive: true, force: true }).catch(() => {});
});

const askedNote = (result: unknown) =>
  ((result as { content?: Array<{ text?: string }> }).content?.[0]?.text ?? "");

describe("cost of asking", () => {
  it("opens one tab for the first question and none after, while the inbox is open", async () => {
    const first = await client.callTool({
      name: "ui_ask",
      arguments: { question: "First?", options: ["Yes", "No"] },
    });
    expect(askedNote(first)).toContain("opened in the browser");

    origin = new URL((first.structuredContent as { url: string }).url).origin;
    // Simulate the inbox being open, which is what its polling signals.
    await fetch(`${origin}/api/sessions?inbox=1`);

    for (const question of ["Second?", "Third?"]) {
      const next = await client.callTool({ name: "ui_ask", arguments: { question, options: ["Yes", "No"] } });
      expect(askedNote(next)).toContain("no new tab");
      expect(askedNote(next)).not.toContain("opened in the browser");
    }
  });

  /**
   * A session page reads /api/sessions too, to show how many other widgets are
   * waiting. If that counted as the inbox being open, then for 8s after the last
   * tab closed no new question would get a tab — precisely when one is needed.
   */
  it("does not treat a plain session-list read as the inbox being open", async () => {
    // Let any presence from earlier in this file lapse.
    await new Promise((r) => setTimeout(r, 8500));
    await fetch(`${origin}/api/sessions`); // no inbox=1

    const asked = await client.callTool({
      name: "ui_ask",
      arguments: { question: "Tab please?", options: ["Yes"] },
    });
    expect(askedNote(asked)).toContain("opened in the browser");
  }, 20_000);

  it("marks a one-click question as answerable from the inbox", async () => {
    await fetch(`${origin}/api/sessions?inbox=1`);
    const body = (await (await fetch(`${origin}/api/sessions`)).json()) as {
      sessions: Array<{ title: string; quickChoices: Array<{ id: string; label: string }> }>;
    };
    const row = body.sessions.find((s) => s.title === "First?");
    expect(row?.quickChoices.map((c) => c.label)).toEqual(["Yes", "No"]);
  });

  it("does NOT offer a quick answer when there is something to look at or fill in", async () => {
    const withDiff = await client.callTool({
      name: "ui_ask",
      arguments: {
        question: "Apply this?",
        content: [{ kind: "diff", value: "@@ -1 +1 @@\n-a\n+b" }],
        options: ["Apply", "Skip"],
      },
    });
    const withFields = await client.callTool({
      name: "ui_ask",
      arguments: { question: "How long?", fields: [{ id: "hours", default: 6 }], options: ["Save"] },
    });

    const ids = [withDiff, withFields].map((r) => (r.structuredContent as { sessionId: string }).sessionId);
    const body = (await (await fetch(`${origin}/api/sessions`)).json()) as {
      sessions: Array<{ sessionId: string; quickChoices: unknown[] }>;
    };
    for (const id of ids) {
      expect(body.sessions.find((s) => s.sessionId === id)?.quickChoices).toEqual([]);
    }
  });

  it("answering from the inbox wakes a parked ui_wait, exactly as the session page would", async () => {
    const asked = await client.callTool({
      name: "ui_ask",
      arguments: { question: "Wake me?", options: ["Do it"] },
    });
    const { sessionId } = asked.structuredContent as { sessionId: string };

    // Park a wait first, so this proves the wake path rather than a late read.
    const waiting = client.callTool({
      name: "ui_wait",
      arguments: { sessionId, timeoutMs: 8000 },
    });

    await fetch(`${origin}/api/session/${sessionId}/answer`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ actionId: "do_it" }),
    });

    const woken = await waiting;
    const structured = woken.structuredContent as { reason: string; output?: { choice?: string } };
    expect(structured.reason).toBe("event");
    expect(structured.output?.choice).toBe("do_it");
  }, 20_000);

  it("rejects an action the widget does not declare", async () => {
    const asked = await client.callTool({ name: "ui_ask", arguments: { question: "Q?", options: ["Only"] } });
    const { sessionId } = asked.structuredContent as { sessionId: string };
    const response = await fetch(`${origin}/api/session/${sessionId}/answer`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ actionId: "not_a_real_action" }),
    });
    expect(response.status).toBe(400);
  });
});
