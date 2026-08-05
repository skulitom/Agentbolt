import { afterEach, beforeEach, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import fsp from "node:fs/promises";
import { SessionManager } from "../src/core/session-manager.js";
import { AgentUiContext } from "../src/mcp/context.js";
import { parseSpec, type InterfaceSpec } from "@agentui/shared";

process.env["AGENT_UI_LOG_LEVEL"] = "silent";

/**
 * The restart path. Hydration rebuilds sessions from disk, but two things do
 * not live in state.json and were once silently dropped across a restart:
 * comments (rebuilt from the event log), and the event wiring that wakes a
 * parked ui_wait (re-attached by ctx.hydrate's watch-all).
 */

let dataDir: string;
let spec: InterfaceSpec;

/**
 * Waits for a condition on disk, rather than sleeping and hoping.
 *
 * This was `setTimeout(150)`, and it failed roughly one run in six — but only
 * when all three packages ran in parallel, never when the file ran alone, which
 * is the most annoying shape a flake can have.
 *
 * TWO separate fire-and-forget writes feed a restart, and a sleep bets on both:
 * `EventLog.append` persists with `void appendLine(...)`, and `setValues`
 * flushes state.json with `void this.#stateWriter.flush(...)`. Comments are
 * rebuilt from the event log, but VALUES come from state.json — so a test that
 * waited only for the log could hydrate a session whose values were still the
 * defaults. Both are deliberate (the in-memory copy is authoritative and the
 * disk catches up); the test just has to say which it is waiting for.
 */
async function waitFor(what: string, check: () => Promise<boolean>): Promise<void> {
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timed out after 5s waiting for ${what}`);
}

async function eventLineCount(sessionId: string): Promise<number> {
  const file = path.join(dataDir, "sessions", sessionId, "events.ndjson");
  const text = await fsp.readFile(file, "utf8").catch(() => "");
  return text.split("\n").filter(Boolean).length;
}

async function stateOnDisk(sessionId: string): Promise<{ values?: Record<string, unknown> } | undefined> {
  const file = path.join(dataDir, "sessions", sessionId, "state.json");
  const text = await fsp.readFile(file, "utf8").catch(() => "");
  try {
    return text ? (JSON.parse(text) as { values?: Record<string, unknown> }) : undefined;
  } catch {
    return undefined;
  }
}

beforeEach(async () => {
  dataDir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-restart-"));
  process.env["AGENT_UI_DATA_DIR"] = dataDir;

  const parsed = parseSpec({
    title: "T",
    controls: [{ id: "alpha", label: "Alpha", type: "slider", min: 0, max: 1, default: 0.5 }],
  });
  if (!parsed.ok) throw new Error(parsed.message);
  spec = parsed.spec;
});

afterEach(async () => {
  delete process.env["AGENT_UI_DATA_DIR"];
  await fsp.rm(dataDir, { recursive: true, force: true }).catch(() => {});
});

describe("restart", () => {
  it("comments made before a restart survive into the finalize output", async () => {
    const before = new SessionManager();
    const session = await before.create({ spec });
    session.addComment("first thoughts");
    session.setValues({ alpha: 0.8 }, "committed", ["alpha"]);
    session.addComment("second thoughts");
    // Comments come back from the event log; the 0.8 comes from state.json.
    // Waiting for only one of the two is what made this flaky.
    await waitFor("both comments and the committed value to reach disk", async () => {
      const state = await stateOnDisk(session.id);
      return (await eventLineCount(session.id)) >= 4 && state?.values?.["alpha"] === 0.8;
    });

    const after = new SessionManager();
    await after.hydrate();
    const restored = after.get(session.id);
    expect(restored).toBeDefined();
    if (!restored) return;

    expect(restored.comments.map((c) => c.text)).toEqual(["first thoughts", "second thoughts"]);

    const output = await restored.finalize();
    expect(output.comments.map((c) => c.text)).toEqual(["first thoughts", "second thoughts"]);
    expect(output.values["alpha"]).toBe(0.8);
  });

  it("a parked ui_wait wakes for a session restored by ctx.hydrate", async () => {
    // The "previous process" creates a session and dies.
    const before = new SessionManager();
    const session = await before.create({ spec });
    await waitFor("the session to be written", async () => (await eventLineCount(session.id)) >= 1);

    // The "next process" hydrates through the context, which must re-watch.
    const ctx = new AgentUiContext();
    await ctx.hydrate();
    const restored = ctx.sessions.get(session.id);
    expect(restored).toBeDefined();
    if (!restored) return;

    const wait = ctx.waiters.wait({
      sessionId: session.id,
      sinceSeq: restored.events.lastSeq,
      wakeOn: ["comment"],
      timeoutMs: 3000,
    });
    restored.addComment("hello after restart");

    const result = await wait;
    expect(result.reason).toBe("event");
    expect(result.events.map((e) => e.type)).toEqual(["comment"]);

    ctx.waiters.disposeAll();
  });

  it("watch is idempotent: watching twice must not double-deliver", async () => {
    const ctx = new AgentUiContext();
    const session = await ctx.sessions.create({ spec });
    ctx.watch(session.id);
    ctx.watch(session.id); // e.g. created, then swept again by a hydrate-style watch-all

    // A double registration is invisible through a waiter (the second notify
    // finds it already released), so spy on the notify fan-out itself: each
    // watch registration calls waiters.notify once per event.
    let notifies = 0;
    const original = ctx.waiters.notify.bind(ctx.waiters);
    ctx.waiters.notify = (sessionId, event) => {
      notifies++;
      original(sessionId, event);
    };
    session.addComment("once");
    expect(notifies).toBe(1);
  });
});

/**
 * spec.json is written by whichever build created the session, and is read back
 * by whichever build restarts. When the vocabulary shrinks — `html` blocks and
 * `layout` both went away — a leftover session on disk holds a spec this build
 * cannot render. It used to be cast straight through, so it hydrated fine and
 * then produced a control the renderer had no arm for.
 */
describe("hydrating a spec written by an older build", () => {
  async function writeSessionOnDisk(id: string, spec: unknown): Promise<void> {
    const dir = path.join(dataDir, "sessions", id);
    await fsp.mkdir(dir, { recursive: true });
    await fsp.writeFile(path.join(dir, "spec.json"), JSON.stringify(spec), "utf8");
    await fsp.writeFile(
      path.join(dir, "state.json"),
      JSON.stringify({
        status: "active",
        revision: 1,
        values: {},
        touched: [],
        createdAt: new Date(0).toISOString(),
        updatedAt: new Date(0).toISOString(),
        lastEventSeq: 0,
      }),
      "utf8",
    );
  }

  it("skips a session whose spec no longer parses, instead of admitting it", async () => {
    await writeSessionOnDisk("stale-html", {
      schemaVersion: 1,
      title: "Written by an older build",
      content: [{ kind: "html", value: "<b>gone</b>" }],
      controls: [],
      actions: [],
    });

    const manager = new SessionManager();
    await manager.hydrate();
    expect(manager.get("stale-html")).toBeUndefined();
  });

  it("skips one unparseable session without taking the readable ones with it", async () => {
    const live = new SessionManager();
    const good = await live.create({ spec });
    await waitFor("the readable session to be written", async () => (await eventLineCount(good.id)) >= 1);
    await writeSessionOnDisk("stale-layout", {
      schemaVersion: 1,
      title: "Also older",
      content: [{ kind: "markdown", value: "x" }],
      controls: [],
      actions: [],
      layout: { columns: 2, previewPosition: "top" },
    });

    const manager = new SessionManager();
    const restored = await manager.hydrate();
    expect(manager.get("stale-layout")).toBeUndefined();
    expect(manager.get(good.id)).toBeDefined();
    expect(restored).toBe(1);
  });
});
