/**
 * Types for the parts of `preview.js` that TypeScript actually consumes.
 *
 * That is `planPreview` and nothing else, on purpose. The rest of this module
 * builds DOM for the preview origin, where there is no TypeScript to consume
 * it — agent-authored modules import it as plain JS. `planPreview` is different:
 * it is the one piece with a correctness contract that the SHELL also has to
 * satisfy (`shouldAutoPreview` must agree with it), so it is the one piece a
 * typed test needs to reach. See preview-agreement.test.ts.
 */

/** The subset of a control the bucketing reads. Mirrors VocabControl. */
interface PlanControl {
  type: string;
  id: string;
  label?: string | undefined;
  unit?: string | undefined;
  options?: ReadonlyArray<{ value: string }> | undefined;
}

export interface PreviewPlan {
  /** The easing picker that leads the frame, if there is one. */
  easingSelect: PlanControl | undefined;
  /** Claimed by the easing select, so they leave the ruler and state buckets. */
  durationCtl: PlanControl | undefined;
  delayCtl: PlanControl | undefined;
  motion: PlanControl[];
  colours: PlanControl[];
  fonts: PlanControl[];
  sizes: PlanControl[];
  /** Rendered as a text state line. Content for a frame, never a reason for one. */
  others: PlanControl[];
  accent: PlanControl | undefined;
  radius: PlanControl | undefined;
  showCard: boolean;
  /** Whether the helper has anything to DRAW. Must equal shouldAutoPreview(spec). */
  drawsSomething: boolean;
  /** Whether the frame would show its "nothing to preview" notice. */
  empty: boolean;
}

export declare function planPreview(spec: unknown): PreviewPlan;
