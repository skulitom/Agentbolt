import { afterEach, beforeEach, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import fsp from "node:fs/promises";
import { parseSpec, type InterfaceSpec } from "@agentui/shared";
import { Session } from "../src/core/session.js";

/**
 * Provenance: which controls the human actually engaged with.
 *
 * This exists because an agent that cannot tell a decision from an untouched
 * default will confidently act on values nobody chose — which happened for real
 * during development, with an accent colour that came from a test script rather
 * than from the user.
 */

let dir: string;
let spec: InterfaceSpec;

// Event-log appends are fire-and-forget by design; teardown removes the temp dir
// while some are still in flight, which is expected and not worth logging.
process.env["AGENT_UI_LOG_LEVEL"] = "silent";

beforeEach(async () => {
  dir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-touched-"));
  const parsed = parseSpec({
    title: "T",
    controls: [
      { id: "alpha", label: "Alpha", type: "slider", min: 0, max: 1, default: 0.5 },
      { id: "beta", label: "Beta", type: "toggle", default: false },
      { id: "gamma", label: "Gamma", type: "text", default: "" },
    ],
  });
  if (!parsed.ok) throw new Error(parsed.message);
  spec = parsed.spec;
});

afterEach(async () => {
  await fsp.rm(dir, { recursive: true, force: true }).catch(() => {});
});

function newSession(): Session {
  return new Session({ sessionId: "s1", dir, spec });
}

describe("touched provenance", () => {
  it("starts with nothing touched", () => {
    expect(newSession().touched).toEqual([]);
  });

  it("records controls the human engaged with", () => {
    const session = newSession();
    session.setValues({ alpha: 0.8 }, "committed", ["alpha"]);
    expect(session.touched).toEqual(["alpha"]);
  });

  it("REPLACES rather than merges, so a reset can be expressed", () => {
    const session = newSession();
    session.setValues({ alpha: 0.8 }, "committed", ["alpha"]);
    session.setValues({ beta: true }, "committed", ["alpha", "beta"]);
    expect(session.touched.sort()).toEqual(["alpha", "beta"]);

    // The human resets alpha back to the suggestion: it must stop counting as a
    // decision. A merging implementation would keep it forever.
    session.setValues({ alpha: 0.5 }, "committed", ["beta"]);
    expect(session.touched).toEqual(["beta"]);

    // Reset all.
    session.setValues({ alpha: 0.5, beta: false }, "committed", []);
    expect(session.touched).toEqual([]);
  });

  it("emits the post-reset set on the event, not a half-updated one", () => {
    const session = newSession();
    session.setValues({ alpha: 0.8 }, "committed", ["alpha"]);
    session.setValues({ alpha: 0.5 }, "committed", []);

    const events = session.events.since(0);
    const committed = events.filter((e) => e.type === "values_committed");
    const last = committed[committed.length - 1]?.payload as { touched: string[] };
    expect(last.touched).toEqual([]);
  });

  it("splits output into touched and untouched so an agent can tell them apart", async () => {
    const session = newSession();
    session.setValues({ alpha: 0.9 }, "committed", ["alpha"]);
    const output = await session.finalize();

    expect(output.touched).toEqual(["alpha"]);
    expect(output.untouched.sort()).toEqual(["beta", "gamma"]);
    // The untouched values are still present — they are just not decisions.
    expect(output.values["beta"]).toBe(false);
  });

  it("survives a restart, so a reconnecting tab greys the right controls", async () => {
    const session = newSession();
    await session.writeInitial();
    session.setValues({ alpha: 0.9 }, "committed", ["alpha"]);
    // Force the debounced state write to land before we read it back.
    await session.finalize();

    const restored = await Session.hydrate(dir, "s1");
    expect(restored?.touched).toEqual(["alpha"]);
  });

  it("leaves touched alone when the client does not report it", () => {
    const session = newSession();
    session.setValues({ alpha: 0.8 }, "committed", ["alpha"]);
    // e.g. values pushed by the agent via ui_update — not a human decision.
    session.setValues({ beta: true }, "committed");
    expect(session.touched).toEqual(["alpha"]);
  });
});
