import { afterEach, beforeEach, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import fsp from "node:fs/promises";
import { UNSUPPORTED_CLIENT_CLAMP_MS, withHeartbeat } from "../src/mcp/heartbeat.js";
import { SessionManager } from "../src/core/session-manager.js";
import { Waiters } from "../src/core/waiters.js";
import { parseSpec, type InterfaceSpec } from "@agentui/shared";

process.env["AGENT_UI_LOG_LEVEL"] = "silent";

let dataDir: string;
let sessions: SessionManager;
let waiters: Waiters;
let spec: InterfaceSpec;

beforeEach(async () => {
  dataDir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-async-"));
  process.env["AGENT_UI_DATA_DIR"] = dataDir;
  sessions = new SessionManager();
  waiters = new Waiters(sessions);

  const parsed = parseSpec({
    title: "T",
    controls: [{ id: "alpha", label: "Alpha", type: "slider", min: 0, max: 1, default: 0.5 }],
  });
  if (!parsed.ok) throw new Error(parsed.message);
  spec = parsed.spec;
});

afterEach(async () => {
  waiters.disposeAll();
  delete process.env["AGENT_UI_DATA_DIR"];
  await fsp.rm(dataDir, { recursive: true, force: true }).catch(() => {});
});

async function newSession() {
  const session = await sessions.create({ spec });
  session.events.onAppend((event) => waiters.notify(session.id, event));
  return session;
}

describe("waiters", () => {
  it("a slider drag does NOT wake a waiting agent", async () => {
    const session = await newSession();
    const cursor = session.events.lastSeq;

    const wait = waiters.wait({ sessionId: session.id, sinceSeq: cursor, wakeOn: ["values"], timeoutMs: 300 });
    // A real drag: many interim frames, no commit.
    for (let i = 0; i < 40; i++) session.setValues({ alpha: i / 40 }, "interim");

    const result = await wait;
    expect(result.reason).toBe("timeout");
    expect(result.events).toHaveLength(0);
  });

  it("a committed change DOES wake it", async () => {
    const session = await newSession();
    const wait = waiters.wait({
      sessionId: session.id,
      sinceSeq: session.events.lastSeq,
      wakeOn: ["values"],
      timeoutMs: 2000,
    });
    session.setValues({ alpha: 0.9 }, "committed", ["alpha"]);

    const result = await wait;
    expect(result.reason).toBe("event");
    expect(result.events.map((e) => e.type)).toEqual(["values_committed"]);
  });

  it("returns immediately when the answer already happened (no needless blocking)", async () => {
    const session = await newSession();
    const cursor = session.events.lastSeq;
    session.addComment("already said this");

    const started = process.hrtime.bigint();
    const result = await waiters.wait({ sessionId: session.id, sinceSeq: cursor, wakeOn: ["comment"], timeoutMs: 60_000 });
    const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;

    expect(result.reason).toBe("event");
    expect(elapsedMs).toBeLessThan(100);
  });

  it("delivers every qualifying event since the cursor, not just the triggering one", async () => {
    const session = await newSession();
    const cursor = session.events.lastSeq;
    session.addComment("one");
    session.addComment("two");

    const result = await waiters.wait({ sessionId: session.id, sinceSeq: cursor, wakeOn: ["comment"], timeoutMs: 500 });
    expect(result.events).toHaveLength(2);
  });

  it("wakes on ANY session when no sessionId is given", async () => {
    const first = await newSession();
    const second = await newSession();
    const wait = waiters.wait({ sinceSeq: 0, wakeOn: ["comment"], timeoutMs: 2000 });
    second.addComment("over here");

    const result = await wait;
    expect(result.sessionId).toBe(second.id);
    expect(result.sessionId).not.toBe(first.id);
  });

  it("releases waiters when the session closes, so an agent is never stuck", async () => {
    const session = await newSession();
    const wait = waiters.wait({
      sessionId: session.id,
      sinceSeq: session.events.lastSeq,
      wakeOn: ["finalize"],
      timeoutMs: 5000,
    });
    await sessions.close(session.id);

    const result = await wait;
    expect(result.reason).toBe("closed");
  });

  it("times out as a normal result rather than an error", async () => {
    const session = await newSession();
    const result = await waiters.wait({
      sessionId: session.id,
      sinceSeq: session.events.lastSeq,
      wakeOn: ["values"],
      timeoutMs: 120,
    });
    expect(result.reason).toBe("timeout");
    expect(result.lastEventSeq).toBe(session.events.lastSeq);
  });

  it("does not leak parked waiters", async () => {
    const session = await newSession();
    await waiters.wait({ sessionId: session.id, sinceSeq: 99, wakeOn: ["values"], timeoutMs: 60 });
    expect(waiters.parkedCount).toBe(0);
  });
});

describe("heartbeat", () => {
  it("honours the full timeout when the client supports progress", async () => {
    const sent: unknown[] = [];
    const { effectiveTimeoutMs } = await withHeartbeat(
      { _meta: { progressToken: "t" }, sendNotification: async (n) => void sent.push(n) },
      300_000,
      async (ms) => ms,
    );
    expect(effectiveTimeoutMs).toBe(300_000);
  });

  it("clamps hard when there is no progress token, since nothing can keep the call alive", async () => {
    const { effectiveTimeoutMs } = await withHeartbeat({}, 300_000, async (ms) => ms);
    expect(effectiveTimeoutMs).toBe(UNSUPPORTED_CLIENT_CLAMP_MS);
    expect(effectiveTimeoutMs).toBeLessThan(60_000);
  });

  it("never inflates a timeout the caller asked to be short", async () => {
    const { effectiveTimeoutMs } = await withHeartbeat({}, 5_000, async (ms) => ms);
    expect(effectiveTimeoutMs).toBe(5_000);
  });

  it("stops sending progress once the call finishes", async () => {
    const sent: unknown[] = [];
    await withHeartbeat(
      { _meta: { progressToken: 1 }, sendNotification: async (n) => void sent.push(n) },
      1000,
      async () => "done",
    );
    const after = sent.length;
    await new Promise((r) => setTimeout(r, 60));
    expect(sent.length).toBe(after);
  });
});
