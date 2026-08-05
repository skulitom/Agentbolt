import { z } from "zod";

/**
 * postMessage protocol between the session page (parent) and the sandboxed
 * preview iframe.
 *
 * The iframe runs cross-origin (the shell's port + 1) with
 * `sandbox="allow-scripts allow-same-origin"` — safe only *because* it is a
 * genuinely different origin; see PreviewFrame.tsx. It posts with targetOrigin
 * "*", so the parent cannot rely on an origin check and validates message
 * *shape* instead — hence these schemas. Treat everything coming out of the
 * frame as hostile.
 */

/**
 * Parent -> frame. A plain TYPE, not a zod schema, and the asymmetry is the
 * point: nothing on the receiving end can validate it. The frame is served to a
 * second origin with no bundler, so it cannot import zod — `preview.html` reads
 * these messages with hand-written property checks. A schema here would only
 * have looked like enforcement.
 *
 * What makes the type load-bearing instead of decorative is `postToPreview` in
 * `PreviewFrame.tsx`: it is the only way the parent posts, so a malformed
 * message is a compile error at the one place that could produce one.
 */
export type ParentToPreview =
  | { type: "init"; sessionId: string; spec: unknown; values: Record<string, unknown> }
  | {
      type: "values";
      values: Record<string, unknown>;
      /**
       * "user" is a continuous gesture — the preview must track it 1:1, never
       * animate under the finger. "agent" is a discrete programmatic jump
       * (ui_update push, reconnect resync, reset) — animating it is what makes
       * the change legible. Optional so a parent that omits it degrades to
       * "user", the safe non-laggy default.
       */
      source?: "user" | "agent";
    };

/**
 * Frame -> parent. This one IS a schema, and stays one: an agent wrote the code
 * that produces these messages, so the parent treats every field as hostile and
 * validates shape before touching it.
 */
export const PreviewToParent = z.discriminatedUnion("type", [
  /**
   * The frame announcing it has loaded and wants `init`. Sent before the module
   * is fetched, because the module needs `spec` to build anything at all.
   */
  z.object({ type: z.literal("ready-for-init") }),
  z.object({ type: z.literal("ready") }),
  z.object({ type: z.literal("preview_event"), name: z.string(), data: z.unknown().optional() }),
  z.object({ type: z.literal("preview_error"), message: z.string(), stack: z.string().optional() }),
  /**
   * The preview asking to CHANGE a value — a dragged point, a moved stop.
   *
   * This is the frame's only write, and it is deliberately not a separate
   * pipeline: the parent replays it through the same `setLocalValue`/`commit`
   * a real input goes through. That is what keeps the invariants true for free
   * — the change counts as `touched`, it is sourced "user" so nothing animates
   * under the finger, drag frames stay interim and only the release commits,
   * and a finished session refuses it like any other input.
   *
   * `commit: true` marks the end of a gesture (pointerup). Without it the
   * change is treated as drag noise, which is exactly what it is.
   */
  z.object({
    type: z.literal("set_values"),
    values: z.record(z.string(), z.unknown()),
    commit: z.boolean().optional(),
  }),
]);
export type PreviewToParent = z.infer<typeof PreviewToParent>;

/** Bare specifiers an agent-authored preview module is allowed to import. */
export const PREVIEW_IMPORTS = ["three", "three/addons/", "agentui", "agentui/motion"] as const;

/**
 * The second update() argument is an object, not a bare string, so future
 * fields (tick time, viewport…) never break existing two-arg modules.
 */
export const PREVIEW_MODULE_CONTRACT = `export default {
  setup({ container, spec, values, data, emit, setValues }) { /* build into container */ },
  update(values, meta) { /* every change, ~60fps during drags; meta.source is "user" (track 1:1) or "agent" (animate the change) */ },
  dispose() { /* optional cleanup */ },
};

// setValues makes a preview INTERACTIVE — drag a point, move a stop:
//   setValues({ origin: [x, y] });                 // during a gesture
//   setValues({ origin: [x, y] }, { commit: true }); // on release
// It goes through the same path as a real input, so the change is marked as
// deliberately set by the user and only the committed one wakes the agent.`;
