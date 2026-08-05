import { EventEmitter } from "node:events";
import type { Server } from "node:http";
import { describe, expect, it } from "vitest";
import { PORT_BASE, PORT_SLOTS, candidateOriginsAllProjects, derivedPort, listenOnAvailablePair } from "../src/core/ports.js";

/**
 * A server whose bind outcome is scripted per port, so the scan can be driven
 * through cases that are impossible to arrange for real (EPERM from a security
 * product, a pair whose second half hard-fails).
 */
class FakeServer extends EventEmitter {
  closed = 0;
  constructor(private readonly outcome: (port: number) => "ok" | NodeJS.ErrnoException) {
    super();
  }
  listen(port: number, _host: string): void {
    const result = this.outcome(port);
    // Async, like the real thing: a synchronous emit would fire before the
    // caller has attached its listeners.
    setImmediate(() => (result === "ok" ? this.emit("listening") : this.emit("error", result)));
  }
  close(callback: () => void): void {
    this.closed++;
    setImmediate(callback);
  }
}

function errno(code: string): NodeJS.ErrnoException {
  return Object.assign(new Error(`bind ${code}`), { code });
}

const asServer = (fake: FakeServer) => fake as unknown as Server;

/**
 * Ports are derived from the project so the inbox has a permanent address.
 *
 * With the old scan-from-a-base policy the port depended on what else happened
 * to be running, so it moved between sessions — during one afternoon of testing
 * it climbed from 4517 to 4531 — and a moving URL cannot be bookmarked or
 * pinned, which is what stops the inbox becoming somewhere you just leave open.
 */
describe("derived ports", () => {
  it("gives the same project the same port every time", () => {
    const project = "C:\\DEV\\AgentUI\\.agent-ui";
    expect(derivedPort(project)).toBe(derivedPort(project));
  });

  it("is case-insensitive, because Windows launchers disagree about case", () => {
    expect(derivedPort("C:\\DEV\\AgentUI")).toBe(derivedPort("c:\\dev\\agentui"));
  });

  it("gives different projects different ports (for a realistic sample)", () => {
    const projects = Array.from({ length: 40 }, (_, i) => `C:\\DEV\\project-${i}`);
    const ports = new Set(projects.map(derivedPort));
    // Hash collisions are possible and handled by scanning; what matters is that
    // it spreads rather than piling everything on one port.
    expect(ports.size).toBeGreaterThan(projects.length * 0.75);
  });

  it("only ever lands on an even slot, so shell/preview pairs cannot interleave", () => {
    for (let i = 0; i < 200; i++) {
      const port = derivedPort(`project-${i}`);
      expect(port % 2).toBe(0);
      expect(port).toBeGreaterThanOrEqual(PORT_BASE);
      expect(port).toBeLessThan(PORT_BASE + PORT_SLOTS * 2);
    }
  });

  it("advertises every project's origins, not just this one's", () => {
    const origins = candidateOriginsAllProjects();
    // Both halves of each pair, for every slot: an origin check must accept a
    // page served by any project's instance.
    expect(origins).toHaveLength(PORT_SLOTS * 2);
    expect(origins).toContain(`http://127.0.0.1:${PORT_BASE}`);
    expect(origins).toContain(`http://127.0.0.1:${PORT_BASE + 1}`);
    expect(new Set(origins).size).toBe(origins.length);
  });
});

/**
 * "Busy" and "broken" are different answers and must not be collapsed.
 *
 * Treating every bind error as busy means a real failure is reported only after
 * all 120 slots, and reported as "could not bind any port in 4520-4760" — which
 * sends you looking for a port conflict that does not exist. The errno is the
 * diagnosis; it has to survive the scan.
 */
describe("listenOnAvailablePair error handling", () => {
  it("surfaces a hard bind error immediately, with its errno", async () => {
    const shell = new FakeServer(() => errno("EPERM"));
    const preview = new FakeServer(() => "ok");

    await expect(listenOnAvailablePair(asServer(shell), asServer(preview))).rejects.toThrow(/EPERM/);
  });

  it("keeps scanning past a busy port and settles on the first free pair", async () => {
    const first = PORT_BASE;
    const shell = new FakeServer((port) => (port === first ? errno("EADDRINUSE") : "ok"));
    const preview = new FakeServer(() => "ok");

    const result = await listenOnAvailablePair(asServer(shell), asServer(preview));
    expect(result.shellPort).not.toBe(first);
    expect(result.previewPort).toBe(result.shellPort + 1);
  });

  it("releases the shell when the preview half is merely busy", async () => {
    // P binds, P+1 is taken: the pair is unusable, so P must not be left held.
    let firstPair = true;
    const shell = new FakeServer(() => "ok");
    const preview = new FakeServer(() => {
      if (firstPair) {
        firstPair = false;
        return errno("EADDRINUSE");
      }
      return "ok";
    });

    await listenOnAvailablePair(asServer(shell), asServer(preview));
    expect(shell.closed).toBe(1);
  });

  it("releases the shell when the preview half hard-fails, instead of stranding it", async () => {
    // The regression this guards: P is bound, P+1 rejects, and the throw
    // escapes past the cleanup leaving a listener nothing owns or will close.
    const shell = new FakeServer(() => "ok");
    const preview = new FakeServer(() => errno("EPERM"));

    await expect(listenOnAvailablePair(asServer(shell), asServer(preview))).rejects.toThrow(/EPERM/);
    expect(shell.closed).toBe(1);
  });
});
