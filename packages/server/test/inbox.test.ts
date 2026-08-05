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

  /**
   * Dismissing.
   *
   * Answering used to be the only way a row could leave the inbox, which meant
   * the questions you had decided against were exactly the ones that stayed —
   * and a list whose top item is something you are ignoring is a list you stop
   * reading. Dismissal has to end the session for real (or an agent keeps
   * waiting on a row nobody can see) and has to say WHY, so "the user declined"
   * is not delivered as the agent's own ui_close.
   */
  it("dismissing an open question wakes a parked ui_wait and says it was declined", async () => {
    const asked = await client.callTool({
      name: "ui_ask",
      arguments: { question: "Rename the module?", options: ["Rename", "Leave it"] },
    });
    const { sessionId } = asked.structuredContent as { sessionId: string };

    const waiting = client.callTool({ name: "ui_wait", arguments: { sessionId, timeoutMs: 8000 } });

    const response = await fetch(`${origin}/api/session/${sessionId}/dismiss`, { method: "POST" });
    expect(await response.json()).toMatchObject({ ok: true, wasWaiting: true });

    const woken = await waiting;
    const structured = woken.structuredContent as { reason: string; events: Array<Record<string, unknown>> };
    expect(structured.reason).toBe("closed");
    // The distinction the agent acts on: declined, not merely closed.
    expect(askedNote(woken)).toContain("dismissed");
    expect(structured.events.some((e) => e["type"] === "closed")).toBe(true);
  }, 20_000);

  /**
   * The reason box.
   *
   * "Not now", "wrong question" and "I already did it" are three different
   * instructions, and none of them is a button the agent thought to offer — so
   * the sentence the user types on the way out is worth more than the dismissal
   * itself. It rides on the `closed` event rather than arriving as a comment,
   * which is what makes it survive the narrowest wake filter there is: an agent
   * waiting only on `finalize` gets handed only finalize-group events, and a
   * separate comment event would be dropped on precisely the wake it explains.
   */
  it("carries the user's reason through to an agent waiting only on finalize", async () => {
    const asked = await client.callTool({
      name: "ui_ask",
      arguments: { question: "Drop the legacy exporter?", options: ["Drop it", "Keep"] },
    });
    const { sessionId } = asked.structuredContent as { sessionId: string };

    const waiting = client.callTool({
      name: "ui_wait",
      arguments: { sessionId, wakeOn: ["finalize"], timeoutMs: 8000 },
    });

    await fetch(`${origin}/api/session/${sessionId}/dismiss`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ note: "mid-release, ask me on Monday" }),
    });

    const woken = await waiting;
    expect(askedNote(woken)).toContain("mid-release, ask me on Monday");
    const { events } = woken.structuredContent as {
      events: Array<{ type: string; payload?: { reason?: string; note?: string } }>;
    };
    const closed = events.find((event) => event.type === "closed");
    expect(closed?.payload).toMatchObject({ reason: "dismissed", note: "mid-release, ask me on Monday" });
  }, 20_000);

  it("stays a dismissal when no reason is given", async () => {
    const asked = await client.callTool({ name: "ui_ask", arguments: { question: "Quiet one?", options: ["Ok"] } });
    const { sessionId } = asked.structuredContent as { sessionId: string };

    const waiting = client.callTool({ name: "ui_wait", arguments: { sessionId, timeoutMs: 8000 } });
    await fetch(`${origin}/api/session/${sessionId}/dismiss`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      // Whitespace is not a reason; it must not reach the agent as one.
      body: JSON.stringify({ note: "   " }),
    });

    const { events } = (await waiting).structuredContent as {
      events: Array<{ type: string; payload?: Record<string, unknown> }>;
    };
    const closed = events.find((event) => event.type === "closed");
    expect(closed?.payload).toEqual({ reason: "dismissed" });
  }, 20_000);

  it("takes the dismissed row out of the list, permanently", async () => {
    const asked = await client.callTool({ name: "ui_ask", arguments: { question: "Gone?", options: ["Yes"] } });
    const { sessionId } = asked.structuredContent as { sessionId: string };

    await fetch(`${origin}/api/session/${sessionId}/dismiss`, { method: "POST" });

    const body = (await (await fetch(`${origin}/api/sessions`)).json()) as {
      sessions: Array<{ sessionId: string }>;
    };
    expect(body.sessions.map((s) => s.sessionId)).not.toContain(sessionId);
  });

  /**
   * A finished session has already handed over its answer, so clearing its row
   * is tidying rather than refusal — and must not be reported to the agent as a
   * question the user turned down.
   */
  it("clearing a finished row is not reported as a refusal", async () => {
    const asked = await client.callTool({ name: "ui_ask", arguments: { question: "Done?", options: ["Ok"] } });
    const { sessionId } = asked.structuredContent as { sessionId: string };
    await fetch(`${origin}/api/session/${sessionId}/answer`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ actionId: "ok" }),
    });

    const dismissed = await fetch(`${origin}/api/session/${sessionId}/dismiss`, { method: "POST" });
    expect(await dismissed.json()).toMatchObject({ ok: true, wasWaiting: false });
  });

  it("404s a dismissal of something that is not there", async () => {
    const response = await fetch(`${origin}/api/session/nosuchsession/dismiss`, { method: "POST" });
    expect(response.status).toBe(404);
  });

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
