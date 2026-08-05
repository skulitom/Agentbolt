import type { VocabControl } from "./vocab.js";

/**
 * Types for `preview-plan.js`, which is hand-written JavaScript so the preview
 * origin can import it verbatim. See the header there.
 */

export interface PreviewViewSpec {
  label: string;
  /** Control types this view can draw, or "*" for anything. */
  accepts: readonly string[] | "*";
  /** Whether drawing this justifies opening a preview frame at all. */
  draws: boolean;
  /** Fewest controls the view needs to be worth rendering. */
  min?: number;
  roles?: readonly string[];
}

export declare const PREVIEW_VIEWS: Record<string, PreviewViewSpec>;
export declare const PREVIEW_VIEW_NAMES: string[];

export declare function viewAccepts(view: string, controlType: string): boolean;

export interface PreviewPanel {
  view: string;
  controls: VocabControl[];
  title: string;
  /** True when the author asked for this panel rather than it being inferred. */
  declared: boolean;
}

export declare function motionRoles(controls: VocabControl[]): {
  easing: VocabControl | undefined;
  duration: VocabControl | undefined;
  delay: VocabControl | undefined;
};

export interface PreviewPlan {
  panels: PreviewPanel[];
  /** Whether a frame should open. This is what shouldAutoPreview returns. */
  drawsSomething: boolean;
  empty: boolean;
}

export declare function planPreview(spec: unknown): PreviewPlan;
