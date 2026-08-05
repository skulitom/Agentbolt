/**
 * The vocabulary shared by the SHELL and the PREVIEW FRAME.
 *
 * Plain JavaScript with no imports, and that is the entire point. The frame is
 * served to a second origin (the shell's port + 1) with no bundler and no
 * module resolution beyond same-origin relative paths, so it cannot import
 * TypeScript, cannot import zod, and cannot reach into `src/`. Anything both
 * sides must agree on has to be expressible as a file the frame can fetch and
 * `import` verbatim.
 *
 * Before this file, the regexes below lived twice — once here in TypeScript and
 * once hand-copied into `packages/ui/public/agentui/preview.js` — pinned by a
 * test that scraped the .js source with a regex. That test could not see a
 * semantic drift (`isEasingSelect` was duplicated with nothing comparing the
 * two copies at all) and would have broken on a reformat that changed nothing.
 *
 * `packages/ui/scripts/sync-vocab.mjs` copies this file to
 * `packages/ui/public/agentui/vocab.js`; the copy is checked in so a fresh
 * clone works with no build step, and `vocab-sync.test.ts` asserts the two are
 * byte-identical.
 *
 * Types live in `vocab.d.ts` next door.
 */

/**
 * Names that imply a spatial magnitude.
 *
 * `duration`/`delay` were here and are deliberately gone: time is not a length,
 * and a name alone is too weak a signal to open a whole preview pane on. A
 * duration with an explicit `unit` still qualifies via the unit check, which is
 * a real declaration rather than a guess at intent.
 */
export const SIZE_HINTS = /width|height|size|radius|spacing|gap|padding|margin|length|distance|offset/i;

/**
 * No COLOUR_HINTS. There was one, unused on both sides: a colour control is
 * found by `type === "color"`, which is a declaration rather than a guess at
 * what "accent" means.
 */
export const FONT_HINTS = /font|type|text.*size|size.*text/i;

export const DURATION_HINT = /(^|[_\s-])(duration|delay)([_\s-]|$)/i;

export const DELAY_HINT = /(^|[_\s-])delay([_\s-]|$)/i;

/**
 * Easing-select detection, deliberately conservative: the control must be NAMED
 * like an easing choice AND every option value must be easing vocabulary.
 * Either test alone misfires — {id:"mode", options:[ease,linear]} is not an
 * easing picker, and neither is {id:"easing", options:[fast,slow]}. A false
 * positive draws a meaningless curve over a real select; a miss just leaves
 * today's state line, which is the acceptable failure.
 */
export const EASING_NAME_HINT = /(^|[_\s-])(easing|ease|curve|timing)([_\s-]|$)/i;
export const EASING_OPTION =
  /^(none|instant|linear|ease|spring|bounce|elastic|back|overshoot|anticipate|cubic-bezier\(.+\)|ease[-_]?[a-z]+([-_]?(in|out|back|bounce|elastic))*|[a-z]+[-_]?(in|out)([-_]?(back|bounce|elastic|quad|cubic|expo|sine|circ))?)$/i;

/** Whether this select is an easing/timing-function picker. */
export function isEasingSelect(control) {
  return (
    control.type === "select" &&
    EASING_NAME_HINT.test(`${control.id} ${control.label}`) &&
    (control.options?.length ?? 0) >= 2 &&
    control.options.every((option) => EASING_OPTION.test(String(option.value).trim()))
  );
}

/** Whether this control reads as a time, either by unit or by name. */
export function isDurationish(control) {
  return (
    (control.type === "slider" || control.type === "number") &&
    (control.unit === "ms" || control.unit === "s" || DURATION_HINT.test(`${control.id} ${control.label}`))
  );
}

/** Whether this control reads as a spatial magnitude. */
export function isSizeish(control) {
  return (
    (control.type === "slider" || control.type === "number") &&
    Boolean(control.unit || SIZE_HINTS.test(`${control.id} ${control.label}`))
  );
}

/** Whether this control reads as a type-size. */
export function isFontish(control) {
  return (
    (control.type === "slider" || control.type === "number") && FONT_HINTS.test(`${control.id} ${control.label}`)
  );
}

/**
 * Control types the SHELL already illustrates, so the preview frame's state
 * line must not echo them: the text input shows its own text, the compare shows
 * its matrix, the multiselect shows its running total. Repeating them in the
 * frame says the same thing twice and says it worse — an array flattened to
 * "a.ts,b.ts" is not a summary.
 */
export const SELF_SHOWN = ["text", "compare", "multiselect"];

/**
 * Display rounding, used by every surface that prints a number: the control
 * readout, the sample table, bound placeholders, the event log and the preview.
 *
 * The problem being solved is float artefacts — 0.1 + 0.2 must print "0.3", not
 * "0.30000000000000004". The problem being AVOIDED is the fix that was in place
 * for it: rounding to a fixed number of decimal places, which silently turned
 * 0.0001234 into "0". For a tool whose premise is that a visual must encode
 * something real, printing 0 for a small non-zero value is the worse failure of
 * the two, and it fired on any legitimately small-magnitude control (opacity
 * deltas, tolerances, learning rates).
 *
 * Significant digits cut the artefact without ever touching the magnitude.
 * Twelve is comfortably inside a double's ~15-17 digits, so the noise goes and
 * nothing an author actually typed does.
 *
 * (The surfaces disagreed anyway: the shell rounded to 4 places and the preview
 * to 3, for the same value.)
 */
export function roundForDisplay(value) {
  if (typeof value !== "number" || !Number.isFinite(value)) return String(value ?? "");
  return String(Number.parseFloat(value.toPrecision(12)));
}
