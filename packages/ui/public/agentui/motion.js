/**
 * Motion utilities for preview modules, importable as
 * `import { spring, tween } from "agentui/motion"` — or from "agentui", which
 * re-exports everything here.
 *
 * Why this exists: an animated preview used to mean hand-rolling a
 * requestAnimationFrame loop per module (examples/three-preview.mjs), and the
 * built-in helpers could not animate at all. This is the smallest shared core
 * that fixes both. Numbers in, callbacks out — deliberately no DOM binding, no
 * orchestration, and no colour interpolation (toggle a CSS transition instead;
 * the compositor interpolates colours better than JS ever will).
 *
 * Decisions, with reasons:
 *
 * - ONE loop drives every live animation, scheduled as a rAF racing a 100ms
 *   timer — the same pattern (and constant) as preview.html's value coalescer,
 *   for the same reason: browsers pause rAF in hidden tabs, and an agent push
 *   must still settle while the tab is backgrounded. dt is clamped and
 *   substepped (see springStep) so timer-rate ticks cannot blow up spring
 *   integration.
 * - The loop STOPS when the last animation settles. A preview idles for
 *   minutes between adjustments; burning a rAF the whole time is not on.
 * - spring() is the default recommendation over tween(): set() retargets
 *   mid-flight carrying velocity, so a second push landing while the first is
 *   still animating stays smooth instead of restarting.
 * - Reduced motion is honoured HERE, not left to callers: tween() completes
 *   synchronously, spring.set() behaves as jump(). A module written without
 *   ever thinking about it is still polite.
 * - No top-level DOM/matchMedia access: the file must import cleanly in Node,
 *   where its pure cores (cubicBezier, springStep) are unit-tested
 *   (packages/ui/test/motion.test.ts).
 * - `durations` hand-mirrors the shell's --t-* tokens in styles.css. The frame
 *   is a different origin and inherits no CSS variables, so a copy is the only
 *   option — the same precedent as SIZE_HINTS in shared/auto-preview.ts.
 */

/* ---------------------------------------------------------------- tokens */

/** Mirrors --t-fast / --t-quick / --t-gentle in packages/ui/src/styles.css. */
export const durations = { fast: 90, quick: 140, gentle: 220 };

/* ---------------------------------------------------------------- easings */

/**
 * CSS-parity cubic bezier: P0=(0,0), P3=(1,1), returns t → progress.
 * Newton first (fast for the smooth middle), bisection fallback (steep ends).
 */
export function cubicBezier(x1, y1, x2, y2) {
  const coord = (a, b) => (u) => ((1 - 3 * b + 3 * a) * u * u * u) + ((3 * b - 6 * a) * u * u) + (3 * a * u);
  const px = coord(x1, x2);
  const py = coord(y1, y2);
  const dpx = (u) => 3 * (1 - 3 * x2 + 3 * x1) * u * u + 2 * (3 * x2 - 6 * x1) * u + 3 * x1;
  return (t) => {
    if (t <= 0) return 0;
    if (t >= 1) return 1;
    let u = t;
    for (let i = 0; i < 8; i++) {
      const slope = dpx(u);
      if (Math.abs(slope) < 1e-6) break;
      u -= (px(u) - t) / slope;
    }
    if (u < 0 || u > 1 || Math.abs(px(u) - t) > 1e-4) {
      let lo = 0;
      let hi = 1;
      for (let i = 0; i < 32; i++) {
        u = (lo + hi) / 2;
        if (px(u) < t) lo = u;
        else hi = u;
      }
    }
    return py(u);
  };
}

export const easings = {
  linear: (t) => t,
  easeIn: cubicBezier(0.42, 0, 1, 1),
  easeOut: cubicBezier(0.2, 0, 0, 1),
  easeInOut: cubicBezier(0.45, 0, 0.2, 1),
  easeOutBack: (t) => {
    const c1 = 1.70158;
    return 1 + (c1 + 1) * (t - 1) ** 3 + c1 * (t - 1) ** 2;
  },
  easeOutBounce: (t) => {
    const n1 = 7.5625;
    const d1 = 2.75;
    if (t < 1 / d1) return n1 * t * t;
    if (t < 2 / d1) return n1 * (t -= 1.5 / d1) * t + 0.75;
    if (t < 2.5 / d1) return n1 * (t -= 2.25 / d1) * t + 0.9375;
    return n1 * (t -= 2.625 / d1) * t + 0.984375;
  },
  easeOutElastic: (t) => {
    if (t === 0 || t === 1) return t;
    return 2 ** (-10 * t) * Math.sin((t * 10 - 0.75) * ((2 * Math.PI) / 3)) + 1;
  },
};

const NAMED = {
  linear: easings.linear,
  ease: cubicBezier(0.25, 0.1, 0.25, 1),
  "ease-in": easings.easeIn,
  "ease-out": easings.easeOut,
  "ease-in-out": easings.easeInOut,
  "ease-out-back": easings.easeOutBack,
  "ease-out-bounce": easings.easeOutBounce,
  "ease-out-elastic": easings.easeOutElastic,
  back: easings.easeOutBack,
  overshoot: easings.easeOutBack,
  anticipate: easings.easeOutBack,
  bounce: easings.easeOutBounce,
  elastic: easings.easeOutElastic,
  spring: easings.easeOutElastic,
  none: (t) => (t >= 1 ? 1 : 0),
  instant: (t) => (t >= 1 ? 1 : 0),
};

/**
 * "easeOutBack" | "ease-out" | "cubic-bezier(…)" | function → easing function.
 * Unknown names fall back to easeOut rather than throwing: this is fed from
 * select options an agent authored, and a preview that dies over vocabulary
 * is worse than one with a slightly wrong curve.
 */
export function resolveEasing(nameOrCss) {
  if (typeof nameOrCss === "function") return nameOrCss;
  const raw = String(nameOrCss ?? "").trim();
  // Bezier first: the name normalisation below would mangle the whitespace
  // inside "cubic-bezier(0.2, 0, 0, 1)" before the arguments could parse.
  const bezier = /^cubic-bezier\(\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*\)$/i.exec(raw);
  if (bezier) return cubicBezier(+bezier[1], +bezier[2], +bezier[3], +bezier[4]);
  const key = raw
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .toLowerCase()
    .replace(/[_\s]+/g, "-");
  return NAMED[key] ?? easings.easeOut;
}

export function prefersReducedMotion() {
  return typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/* ------------------------------------------------------------- pure cores */

/**
 * Damped-spring integration over one frame. Pure and deterministic so it is
 * testable in Node. dt is clamped to 64ms and then integrated in ≤16ms
 * semi-implicit Euler substeps: the scheme destabilises once damping·dt
 * approaches 1 (at the default damping, one raw 64ms step diverges), and a
 * background tab ticks at ~100ms — substepping keeps hidden-tab frames the
 * same shape as foreground ones. The clamp/substep both live HERE so no
 * caller can feed an integration-exploding step.
 */
export function springStep(state, target, dtMs, { stiffness = 170, damping = 26 } = {}) {
  let x = state.x;
  let v = state.v;
  let remaining = Math.min(dtMs, 64);
  while (remaining > 0) {
    const dt = Math.min(remaining, 16) / 1000;
    v += (stiffness * (target - x) - damping * v) * dt;
    x += v * dt;
    remaining -= 16;
  }
  return { x, v };
}

export function isSettled(state, target, epsilon = 0.001) {
  return Math.abs(state.x - target) < epsilon && Math.abs(state.v) < epsilon * 20;
}

/* ------------------------------------------------- the shared driving loop */

const live = new Set();
let rafId = 0;
let timerId = 0;
let lastTick = 0;

const now = () => (typeof performance !== "undefined" ? performance.now() : Date.now());
const hasRaf = () => typeof requestAnimationFrame === "function";

function tick() {
  rafId = 0;
  timerId = 0;
  const t = now();
  const dt = Math.min(t - lastTick, 64);
  lastTick = t;
  for (const anim of [...live]) anim.step(dt);
  if (live.size) schedule();
}

function schedule() {
  if (rafId || timerId) return;
  if (hasRaf()) {
    rafId = requestAnimationFrame(() => {
      if (timerId) clearTimeout(timerId);
      timerId = 0;
      tick();
    });
  }
  timerId = setTimeout(() => {
    if (rafId) cancelAnimationFrame(rafId);
    rafId = 0;
    tick();
  }, 100);
}

function start(anim) {
  if (!live.size) lastTick = now();
  live.add(anim);
  schedule();
}

/** Stop every animation this module is driving. The polite call in dispose(). */
export function stopAll() {
  live.clear();
  if (rafId) cancelAnimationFrame(rafId);
  if (timerId) clearTimeout(timerId);
  rafId = 0;
  timerId = 0;
}

/* ------------------------------------------------------ managed animations */

/**
 * Fixed-duration interpolation from → to. Use for replays and one-shots;
 * for values that can be retargeted mid-flight, prefer spring().
 */
export function tween({ from = 0, to = 1, duration = durations.gentle, easing = easings.easeOut, onUpdate, onComplete } = {}) {
  const fn = resolveEasing(easing);
  if (duration <= 0 || prefersReducedMotion()) {
    onUpdate?.(to);
    onComplete?.();
    return { stop() {} };
  }
  let elapsed = 0;
  const anim = {
    step(dt) {
      elapsed += dt;
      const t = Math.min(1, elapsed / duration);
      onUpdate?.(from + (to - from) * fn(t));
      if (t >= 1) {
        live.delete(anim);
        onComplete?.();
      }
    },
  };
  start(anim);
  return {
    stop() {
      live.delete(anim);
    },
  };
}

/**
 * A retargetable damped spring around one number.
 *   set(target) — animate toward target, PRESERVING current velocity;
 *   jump(value) — write instantly (user-sourced frames, first paint);
 *   get() / stop().
 * On settle the exact target is delivered, so readouts end on the real number
 * rather than a float one epsilon away.
 */
export function spring(initial = 0, { stiffness = 170, damping = 26, restDelta, onUpdate, onSettle } = {}) {
  let state = { x: initial, v: 0 };
  let target = initial;
  const epsilon = () => restDelta ?? Math.max(1e-3, Math.abs(target) * 1e-3);
  const anim = {
    step(dt) {
      state = springStep(state, target, dt, { stiffness, damping });
      if (isSettled(state, target, epsilon())) {
        state = { x: target, v: 0 };
        live.delete(anim);
        onUpdate?.(target);
        onSettle?.(target);
      } else {
        onUpdate?.(state.x);
      }
    },
  };
  const jump = (value) => {
    target = value;
    state = { x: value, v: 0 };
    live.delete(anim);
    onUpdate?.(value);
  };
  return {
    set(next) {
      target = next;
      if (prefersReducedMotion()) {
        jump(next);
        return;
      }
      if (state.x === next && state.v === 0) return;
      start(anim);
    },
    jump,
    get: () => state.x,
    stop() {
      live.delete(anim);
    },
  };
}
