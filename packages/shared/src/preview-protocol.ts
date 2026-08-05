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

export const ParentToPreview = z.discriminatedUnion("type", [
  z.object({ type: z.literal("init"), sessionId: z.string(), spec: z.unknown(), values: z.record(z.string(), z.unknown()) }),
  z.object({
    type: z.literal("values"),
    values: z.record(z.string(), z.unknown()),
    /**
     * "user" is a continuous gesture — the preview must track it 1:1, never
     * animate under the finger. "agent" is a discrete programmatic jump
     * (ui_update push, reconnect resync, reset) — animating it is what makes
     * the change legible. Optional so an older parent that omits it degrades
     * to "user", the safe non-laggy default.
     */
    source: z.enum(["user", "agent"]).optional(),
  }),
  z.object({ type: z.literal("dispose") }),
]);
export type ParentToPreview = z.infer<typeof ParentToPreview>;

export const PreviewToParent = z.discriminatedUnion("type", [
  z.object({ type: z.literal("ready") }),
  z.object({ type: z.literal("preview_event"), name: z.string(), data: z.unknown().optional() }),
  z.object({ type: z.literal("preview_error"), message: z.string(), stack: z.string().optional() }),
  z.object({ type: z.literal("resize"), height: z.number().positive() }),
]);
export type PreviewToParent = z.infer<typeof PreviewToParent>;

/** Bare specifiers an agent-authored preview module is allowed to import. */
export const PREVIEW_IMPORTS = ["three", "three/addons/", "agentui", "agentui/motion"] as const;

/**
 * The second update() argument is an object, not a bare string, so future
 * fields (tick time, viewport…) never break existing two-arg modules.
 */
export const PREVIEW_MODULE_CONTRACT = `export default {
  setup({ container, spec, values, emit }) { /* build into container */ },
  update(values, meta) { /* every change, ~60fps during drags; meta.source is "user" (track 1:1) or "agent" (animate the change) */ },
  dispose() { /* optional cleanup */ },
};`;
