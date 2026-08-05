import { describe, expect, it } from "vitest";
import { PORT_BASE, PORT_SLOTS, candidateOriginsAllProjects, derivedPort } from "../src/core/ports.js";

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
