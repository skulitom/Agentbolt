import { describe, expect, it } from "vitest";
// The .js import is the point: motion.js must import cleanly in Node (no
// top-level DOM/matchMedia access), because that is what makes its pure cores
// testable at all. If this import ever throws, someone touched module scope.
import {
  cubicBezier,
  durations,
  easings,
  isSettled,
  resolveEasing,
  springStep,
} from "../public/agentui/motion.js";

describe("easings", () => {
  it("all start at 0 and end at 1", () => {
    for (const [name, fn] of Object.entries(easings)) {
      expect(fn(0), `${name}(0)`).toBeCloseTo(0, 6);
      expect(fn(1), `${name}(1)`).toBeCloseTo(1, 6);
    }
  });

  it("non-overshooting curves are monotonic", () => {
    for (const name of ["linear", "easeIn", "easeOut", "easeInOut"] as const) {
      const fn = easings[name];
      let prev = fn(0);
      for (let i = 1; i <= 100; i++) {
        const next = fn(i / 100);
        expect(next, `${name} at t=${i / 100}`).toBeGreaterThanOrEqual(prev - 1e-9);
        prev = next;
      }
    }
  });

  it("matches CSS 'ease' reference values", () => {
    // Reference points for cubic-bezier(0.25, 0.1, 0.25, 1), the CSS `ease`
    // keyword, sampled from the standard solver. Pin the parity claim.
    const ease = cubicBezier(0.25, 0.1, 0.25, 1);
    expect(ease(0.25)).toBeCloseTo(0.4085, 2);
    expect(ease(0.5)).toBeCloseTo(0.8024, 2);
    expect(ease(0.75)).toBeCloseTo(0.9606, 2);
  });
});

describe("resolveEasing", () => {
  it("resolves css names, camelCase and cubic-bezier literals", () => {
    expect(resolveEasing("linear")(0.5)).toBeCloseTo(0.5, 6);
    expect(resolveEasing("ease-out")(0.5)).toBe(easings.easeOut(0.5));
    expect(resolveEasing("easeOutBack")(0.5)).toBe(easings.easeOutBack(0.5));
    expect(resolveEasing("cubic-bezier(0.42, 0, 1, 1)")(0.3)).toBeCloseTo(easings.easeIn(0.3), 4);
  });

  it("falls back to easeOut for unknown vocabulary instead of throwing", () => {
    // Fed from agent-authored select options; a preview that dies over a word
    // is worse than one with a slightly wrong curve.
    expect(resolveEasing("whatever")(0.5)).toBe(easings.easeOut(0.5));
    expect(resolveEasing(undefined)(0.5)).toBe(easings.easeOut(0.5));
  });

  it("passes functions through untouched", () => {
    const fn = (t: number) => t * t;
    expect(resolveEasing(fn)).toBe(fn);
  });
});

describe("springStep", () => {
  const step = (state: { x: number; v: number }, target: number, dt: number) =>
    springStep(state, target, dt) as { x: number; v: number };

  it("converges to the target and settles", () => {
    let state = { x: 0, v: 0 };
    for (let i = 0; i < 300; i++) state = step(state, 100, 16);
    expect(state.x).toBeCloseTo(100, 1);
    expect(isSettled(state, 100, 0.1)).toBe(true);
  });

  it("preserves velocity across a retarget — the reason spring is the default", () => {
    let state = { x: 0, v: 0 };
    for (let i = 0; i < 10; i++) state = step(state, 100, 16);
    expect(state.v).toBeGreaterThan(0);
    // Retargeting is just changing the target; the state carries its velocity,
    // so immediately after a retarget BELOW the current position the value
    // still travels upward for a step instead of snapping around. That
    // carried momentum is what makes a second push mid-animation look smooth.
    const after = step(state, 50, 16);
    expect(after.x).toBeGreaterThan(state.x);
  });

  it("clamps dt so a background tab cannot blow up the integration", () => {
    // A hidden tab ticks at ~100ms; without the clamp the explicit Euler term
    // overshoots harder the larger dt gets. The clamp makes a huge dt behave
    // exactly like 64ms.
    const big = step({ x: 0, v: 0 }, 100, 5000);
    const clamped = step({ x: 0, v: 0 }, 100, 64);
    expect(big.x).toBe(clamped.x);
    expect(big.v).toBe(clamped.v);
    // And the result stays finite/stable from any single step.
    expect(Number.isFinite(big.x)).toBe(true);
  });

  it("stays stable over a long background-tab run", () => {
    let state = { x: 0, v: 0 };
    for (let i = 0; i < 200; i++) state = step(state, 1, 100);
    expect(state.x).toBeCloseTo(1, 2);
  });
});

describe("tokens", () => {
  it("mirror the shell's --t-* motion tokens", async () => {
    // durations hand-mirrors the CSS tokens because the preview frame is a
    // different origin and inherits no CSS variables. Same contract as the
    // SIZE_HINTS mirror in shared/auto-preview.ts.
    const fs = await import("node:fs");
    const url = await import("node:url");
    const path = await import("node:path");
    const here = path.dirname(url.fileURLToPath(import.meta.url));
    const css = fs.readFileSync(path.resolve(here, "..", "src", "styles", "tokens-and-base.css"), "utf8");
    expect(css).toContain(`--t-fast: ${durations.fast}ms`);
    expect(css).toContain(`--t-quick: ${durations.quick}ms`);
    expect(css).toContain(`--t-gentle: ${durations.gentle}ms`);
  });
});
