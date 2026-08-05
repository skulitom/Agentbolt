import { collectPlaceholders } from "./bind.js";
import { parseSampleRule } from "./sample.js";
import type { InterfaceSpec } from "./spec.js";

/**
 * Whether a spec has anything worth showing without the agent writing a preview.
 *
 * The rule, arrived at from real use: anything with a MAGNITUDE deserves a
 * visual — a colour, a length, a duration, a type size. You cannot judge "760px"
 * or "#b0367a" from a number. Toggles, selects and free text are
 * self-describing, and forcing them into a picture would add chrome that says
 * nothing.
 *
 * Getting this wrong in the generous direction is the safer failure: a slightly
 * redundant preview costs a little space, whereas a missing one means the human
 * is guessing while the UI looks authoritative.
 */
/**
 * Names that imply a spatial magnitude.
 *
 * `duration`/`delay` were here and are deliberately gone: time is not a length,
 * and a name alone is too weak a signal to open a whole preview pane on. A
 * duration with an explicit `unit` still qualifies via the unit check, which is
 * a real declaration rather than a guess at intent.
 *
 * MUST stay identical to SIZE_HINTS in packages/ui/public/agentui/preview.js —
 * this decides whether a frame exists, that one decides what goes inside it, and
 * drift between them yields an empty preview pane. A test asserts they match.
 */
const SIZE_HINTS = /width|height|size|radius|spacing|gap|padding|margin|length|distance|offset/i;

export const SIZE_HINTS_SOURCE = SIZE_HINTS.source;

/**
 * Easing-select detection, deliberately conservative: the control must be
 * NAMED like an easing choice AND every option value must be easing
 * vocabulary. Either test alone misfires — {id:"mode", options:[ease,linear]}
 * is not an easing picker, and neither is {id:"easing", options:[fast,slow]}.
 * A false positive draws a meaningless curve over a real select; a miss just
 * leaves today's state line, which is the acceptable failure.
 *
 * Mirrored in packages/ui/public/agentui/preview.js like SIZE_HINTS above,
 * and pinned by the same sync test.
 */
const EASING_NAME_HINT = /(^|[_\s-])(easing|ease|curve|timing)([_\s-]|$)/i;
const EASING_OPTION =
  /^(none|instant|linear|ease|spring|bounce|elastic|back|overshoot|anticipate|cubic-bezier\(.+\)|ease[-_]?[a-z]+([-_]?(in|out|back|bounce|elastic))*|[a-z]+[-_]?(in|out)([-_]?(back|bounce|elastic|quad|cubic|expo|sine|circ))?)$/i;

export const EASING_NAME_HINT_SOURCE = EASING_NAME_HINT.source;
export const EASING_OPTION_SOURCE = EASING_OPTION.source;

/** Whether this select is an easing/timing-function picker. */
export function isEasingSelect(control: InterfaceSpec["controls"][number]): boolean {
  return (
    control.type === "select" &&
    EASING_NAME_HINT.test(`${control.id} ${control.label}`) &&
    control.options.length >= 2 &&
    control.options.every((o) => EASING_OPTION.test(o.value.trim()))
  );
}

/**
 * Would the built-in auto-preview actually have something to DRAW for this
 * control? Gates whether a preview frame appears at all.
 *
 * Note what is absent: `vector`. It reads as spatial, but the helper has no
 * vector view, so claiming it is previewable opens a pane whose entire content
 * is the text "Position: 1,2,3". An empty frame is worse than no frame.
 */
export function isPreviewSubject(control: InterfaceSpec["controls"][number]): boolean {
  if (control.type === "color") return true;
  if (control.type === "slider" || control.type === "number") {
    return Boolean(control.unit) || SIZE_HINTS.test(`${control.id} ${control.label}`);
  }
  // An easing curve is a shape the helper CAN draw (easingCurve): the name
  // "easeOutBack" tells you nothing you can feel, the plotted curve does.
  if (isEasingSelect(control)) return true;
  return false;
}

/**
 * Does anything on screen change when this control moves? Used only for
 * ORDERING controls, which is a weaker question than "can we draw it" — a
 * control can be illustrated by an agent's own preview module, by a `sample`
 * block, by a bound snippet, or by its own renderer, without the built-in
 * helper knowing how to draw a thing.
 *
 * Keeping this separate from isPreviewSubject is what stops "referenced by an
 * effect ⇒ visual" from also opening a preview FRAME: a widget whose only
 * illustrated control is a compare would otherwise ship half a screen of pane
 * containing the string "Retries: 3".
 */
export function controlIsIllustrated(spec: InterfaceSpec, control: InterfaceSpec["controls"][number]): boolean {
  if (isPreviewSubject(control)) return true;
  // A vector has no built-in view, but an authored module usually draws it.
  if (control.type === "vector") return true;
  // These two ARE their own illustration: the matrix and the running total are
  // on the control itself.
  if (control.type === "compare") return true;
  if (control.type === "multiselect") return control.options.some((option) => option.weight !== undefined);

  return spec.content.some((block) => {
    if (block.kind === "sample") return parseSampleRule(block.rule)?.control === control.id;
    if ((block.kind === "markdown" || block.kind === "code") && block.bind) {
      return collectPlaceholders(block.value).some((placeholder) => placeholder.id === control.id);
    }
    return false;
  });
}

export function shouldAutoPreview(spec: InterfaceSpec): boolean {
  // An agent can decline a generated preview outright. Without this the only
  // way to suppress one was to delete `unit`, which the skill rightly forbids.
  if (spec.preview?.auto === false) return false;
  return spec.controls.some(isPreviewSubject);
}

/**
 * The module served for a spec with no authored preview. Deliberately tiny: all
 * the work lives in the shared helper, so improving previews improves every
 * widget that never asked for one.
 */
export const AUTO_PREVIEW_MODULE = `import { autoPreview } from "agentui";

// Generated by AgentUI because this widget has values with a visible magnitude
// and no previewModule of its own. Supply one to ui_create to replace it.
let impl = null;
export default {
  setup(ctx) {
    impl = autoPreview(ctx.spec);
    return impl.setup(ctx);
  },
  update(values, meta) {
    impl?.update(values, meta);
  },
};
`;
