import { z } from "zod";

/**
 * postMessage protocol between the session page (parent) and the sandboxed
 * preview iframe.
 *
 * The iframe runs with `sandbox="allow-scripts"` and no `allow-same-origin`, so
 * it has an opaque origin and MUST post with targetOrigin "*". That means the
 * parent cannot rely on an origin check and validates message *shape* instead —
 * hence these schemas. Treat everything coming out of the frame as hostile.
 */

export const ParentToPreview = z.discriminatedUnion("type", [
  z.object({ type: z.literal("init"), sessionId: z.string(), spec: z.unknown(), values: z.record(z.string(), z.unknown()) }),
  z.object({ type: z.literal("values"), values: z.record(z.string(), z.unknown()) }),
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
export const PREVIEW_IMPORTS = ["three", "three/addons/"] as const;

export const PREVIEW_MODULE_CONTRACT = `export default {
  setup({ container, spec, values, emit }) { /* build into container */ },
  update(values) { /* called on every change, ~60fps during drags */ },
  dispose() { /* optional cleanup */ },
};`;
