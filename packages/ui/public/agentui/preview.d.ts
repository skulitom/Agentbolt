/**
 * Types for the parts of `preview.js` that TypeScript actually consumes.
 *
 * The rest of this module builds DOM for the preview origin, where there is no
 * TypeScript to consume it — agent-authored modules import it as plain JS.
 * `VIEW_IMPLS` is different: it has to stay in step with `PREVIEW_VIEWS` over in
 * shared, and a typed test is what checks that.
 */
import type { PreviewPanel } from "../../../shared/src/preview-plan.js";

export interface PreviewView {
  mount(container: HTMLElement): void;
  update(payload: unknown, meta?: { source?: "user" | "agent" }): void;
}

export interface ViewImpl {
  create(panel: PreviewPanel, spec: unknown): PreviewView;
  payload(panel: PreviewPanel, values: Record<string, unknown>): unknown;
}

/**
 * One entry per name in PREVIEW_VIEWS. A view in the vocabulary with nothing
 * here opens a frame (`draws: true`) and then renders nothing, so the key sets
 * are asserted equal.
 */
export declare const VIEW_IMPLS: Record<string, ViewImpl>;

export declare function autoPreview(spec: unknown): {
  setup(ctx: { container: HTMLElement }): void;
  update(values: Record<string, unknown>, meta?: { source?: "user" | "agent" }): void;
};
