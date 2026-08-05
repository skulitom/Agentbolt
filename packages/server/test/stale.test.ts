import { afterEach, beforeEach, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import fsp from "node:fs/promises";
import { parseSpec, type InterfaceSpec, type SessionState } from "@agentui/shared";
import { SessionManager } from "../src/core/session-manager.js";
import { sessionsDir } from "../src/core/paths.js";
import { DEFAULT_STALE_AFTER_HOURS, STALE_AFTER_ENV, stalenessOf } from "../src/core/staleness.js";

/**
 * Leftover sessions.
 *
 * hydrate() restores every non-closed session, which is what lets an agent whose
 * MCP server restarted mid-question pick up exactly where it left off. During
 * dogfooding that same behaviour brought back demo sessions from days earlier,
 * all listed as "active" — the dashboard claimed seven people were waiting when
 * nobody was.
 *
 * The policy: keep hydrating everything, but derive staleness from idle time and
 * demote it in both list surfaces. These tests pin down the two things that make
 * that safe — a genuinely-resumable session must stay in the waiting group, and
 * a stale one must remain answerable rather than quietly closed.
 */

let dataDir: string;
let spec: InterfaceSpec;

process.env["AGENT_UI_LOG_LEVEL"] = "silent";

const HOUR_MS = 3_600_000;

beforeEach(async () => {
  dataDir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-stale-"));
  process.env["AGENT_UI_DATA_DIR"] = dataDir;
  delete process.env[STALE_AFTER_ENV];

  const parsed = parseSpec({
    title: "T",
    controls: [{ id: "alpha", label: "Alpha", type: "slider", min: 0, max: 1, default: 0.5 }],
  });
  if (!parsed.ok) throw new Error(parsed.message);
  spec = parsed.spec;
});

afterEach(async () => {
  delete process.env["AGENT_UI_DATA_DIR"];
  delete process.env[STALE_AFTER_ENV];
  await fsp.rm(dataDir, { recursive: true, force: true }).catch(() => {});
});

/**
 * Writes a session to disk and back-dates its state.json, standing in for a run
 * that happened `idleHours` ago. Rewriting the file rather than faking the clock
 * is deliberate: the timestamp a restart actually reads is the one on disk.
 */
async function seedSession(title: string, idleHours: number): Promise<string> {
  const sessions = new SessionManager();
  const session = await sessions.create({ spec: { ...spec, title } });
  await session.writeInitial();

  const file = path.join(sessionsDir(), session.id, "state.json");
  const state = JSON.parse(await fsp.readFile(file, "utf8")) as SessionState;
  const at = new Date(Date.now() - idleHours * HOUR_MS).toISOString();
  await fsp.writeFile(file, JSON.stringify({ ...state, createdAt: at, updatedAt: at }, null, 2), "utf8");
  return session.id;
}

async function hydrated(): Promise<SessionManager> {
  const sessions = new SessionManager();
  await sessions.hydrate();
  return sessions;
}

describe("staleness policy", () => {
  it("still restores a session the agent left a minute ago, as a live one", async () => {
    // The deliberate resumption case: an MCP restart, not a new day.
    const id = await seedSession("Just asked", 1 / 60);
    const sessions = await hydrated();

    const restored = sessions.get(id);
    expect(restored?.status).toBe("active");
    expect(sessions.pending().map((s) => s.id)).toEqual([id]);
    expect(sessions.stale()).toEqual([]);
    expect(sessions.summarize(restored!).stale).toBe(false);
  });

  it("restores a session from an earlier run too, but does not call it live", async () => {
    const id = await seedSession("Yesterday's demo", 30);
    const sessions = await hydrated();

    // Still there and still open — hydration is not where things get dropped.
    expect(sessions.get(id)?.status).toBe("active");
    expect(sessions.stale().map((s) => s.id)).toEqual([id]);
    // ...but not something anyone is waiting on.
    expect(sessions.pending()).toEqual([]);
  });

  it("keeps the dashboard badge and the agent's list telling the same story", async () => {
    const fresh = await seedSession("Live", 0.2);
    const old = await seedSession("Leftover", 72);
    const sessions = await hydrated();

    // Both surfaces read summaries()/pending() rather than classifying for
    // themselves, so agreement is the shape of the data, not a convention.
    const rows = sessions.summaries();
    expect(rows.filter((r) => r.status === "active" && !r.stale).map((r) => r.sessionId)).toEqual([fresh]);
    expect(rows.filter((r) => r.stale).map((r) => r.sessionId)).toEqual([old]);
    expect(sessions.pending()).toHaveLength(1);
  });

  it("sorts what is waiting above what is left over", async () => {
    const old = await seedSession("Leftover", 50);
    const fresh = await seedSession("Live", 0.1);
    const sessions = await hydrated();

    expect(sessions.summaries().map((r) => r.sessionId)).toEqual([fresh, old]);
  });

  it("leaves a stale session fully answerable — it is a label, not a lifecycle change", async () => {
    const id = await seedSession("Never answered", 96);
    const sessions = await hydrated();
    const session = sessions.get(id)!;

    // Someone wanders back to a bookmarked URL days later. It must still work.
    expect(session.isOpen).toBe(true);
    const outbound = await sessions.handleClientMessage(id, {
      type: "finalize",
      values: { alpha: 0.9 },
      touched: ["alpha"],
    });

    expect(outbound.some((o) => o.message.type === "finalized")).toBe(true);
    const output = await session.output();
    expect(output?.values["alpha"]).toBe(0.9);
    expect(output?.touched).toEqual(["alpha"]);
  });

  it("measures idleness from the last change, not from creation", async () => {
    const id = await seedSession("Long session", 12);
    const sessions = await hydrated();
    const session = sessions.get(id)!;
    expect(sessions.summarize(session).stale).toBe(true);

    // Somebody has been adjusting this all afternoon. Old is not abandoned.
    session.setValues({ alpha: 0.7 }, "committed", ["alpha"]);
    expect(sessions.summarize(session).stale).toBe(false);
    expect(sessions.pending().map((s) => s.id)).toEqual([id]);
  });

  it("clear-all closes the leftovers and nothing else", async () => {
    const fresh = await seedSession("Live", 0.1);
    const old = await seedSession("Leftover", 40);
    const older = await seedSession("Older leftover", 200);
    const sessions = await hydrated();

    const cleared = await sessions.closeStale();
    expect(cleared.sort()).toEqual([old, older].sort());
    expect(sessions.list().map((s) => s.id)).toEqual([fresh]);

    // Closed for real, so a restart does not resurrect them.
    expect((await hydrated()).list().map((s) => s.id)).toEqual([fresh]);
  });

  it("honours the configured threshold, and 0 turns staleness off", async () => {
    const id = await seedSession("Two hours ago", 2);

    process.env[STALE_AFTER_ENV] = "1";
    expect((await hydrated()).stale().map((s) => s.id)).toEqual([id]);

    process.env[STALE_AFTER_ENV] = "0";
    expect((await hydrated()).stale()).toEqual([]);
    expect((await hydrated()).pending().map((s) => s.id)).toEqual([id]);
  });

  it("falls back to the default when the threshold is nonsense rather than staling everything", async () => {
    process.env[STALE_AFTER_ENV] = "soon";
    const fresh = await seedSession("Live", 0.1);
    await seedSession("Leftover", DEFAULT_STALE_AFTER_HOURS + 1);

    const sessions = await hydrated();
    expect(sessions.pending().map((s) => s.id)).toEqual([fresh]);
    expect(sessions.stale()).toHaveLength(1);
  });

  it("declines to guess when a timestamp is unreadable", () => {
    // A truncated or hand-edited state.json says nothing about whether anyone is
    // waiting, and "stale" is a claim. Better to keep showing it than to invent one.
    expect(stalenessOf("not a date", Date.now()).stale).toBe(false);
    expect(stalenessOf("not a date", Date.now()).idleMs).toBeUndefined();
    expect(stalenessOf("", Date.now()).idleLabel).toBe("unknown");
  });

  it("still skips closed sessions entirely", async () => {
    const id = await seedSession("Done with", 50);
    const first = await hydrated();
    await first.close(id);

    const sessions = await hydrated();
    expect(sessions.list()).toEqual([]);
    expect(sessions.stale()).toEqual([]);
  });
});
