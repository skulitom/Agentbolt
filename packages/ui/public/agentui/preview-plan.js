import { DELAY_HINT, SELF_SHOWN, isDurationish, isEasingSelect, isFontish, isSizeish } from "./vocab.js";

/**
 * What a generated preview contains, decided in ONE place.
 *
 * This file is plain JavaScript with no DOM for the same reason vocab.js is: the
 * preview frame is a second origin with no bundler, so anything both halves must
 * agree on has to be a file the frame can fetch and `import` verbatim.
 *
 * The agreement here is the point. `shouldAutoPreview` (shell) decides whether a
 * preview frame appears at all; the frame decides what goes inside it. Those
 * used to be separate implementations that a test compared — and the test caught
 * a real disagreement, where a control named "type_scale" landed in the frame's
 * font bucket while the gate said there was nothing to show, so the frame simply
 * never opened. Both sides now call `planPreview`, so disagreeing is not a thing
 * that can happen rather than a thing that is checked for.
 *
 * The plan is an ordered list of PANELS, not a bag of named buckets. That is
 * what makes the vocabulary extensible: a new view is one entry in
 * PREVIEW_VIEWS plus one implementation in preview.js, a declared
 * `preview.views` is simply a plan supplied instead of inferred, and "two motion
 * groups" stops being a special case because panels were never unique by view.
 */

/**
 * The view vocabulary: what each view is called, what it can draw, and whether
 * drawing it justifies opening a frame.
 *
 * `draws: false` is load-bearing. A state line is real content for a frame that
 * exists for another reason, and never a reason to open one — a half-screen pane
 * whose entire content is "Retries: 3" is the failure this whole tool is
 * against. Encoding that per-view means the rule is data rather than a condition
 * someone has to remember to update.
 */
export const PREVIEW_VIEWS = {
  motion: {
    label: "Motion",
    accepts: ["select", "slider", "number"],
    draws: true,
    /** Roles are assigned from the controls given, by name and unit. */
    roles: ["easing", "duration", "delay"],
    min: 1,
  },
  swatch: { label: "Colours", accepts: ["color"], draws: true, min: 1 },
  ruler: { label: "Sizes", accepts: ["slider", "number"], draws: true, min: 1 },
  type: { label: "Type", accepts: ["slider", "number"], draws: true, min: 1 },
  card: { label: "Sample card", accepts: ["color", "slider", "number"], draws: true, min: 0 },
  state: { label: "Settings", accepts: "*", draws: false, min: 1 },
};

/**
 * Every name here must have an implementation in preview.js, and a test asserts
 * it. A view in the vocabulary with nothing behind it is worse than a missing
 * one: `draws: true` opens a frame, and the frame then renders nothing.
 */

export const PREVIEW_VIEW_NAMES = Object.keys(PREVIEW_VIEWS);

/** Whether a view can draw the given control type. */
export function viewAccepts(view, controlType) {
  const spec = PREVIEW_VIEWS[view];
  if (!spec) return false;
  return spec.accepts === "*" || spec.accepts.includes(controlType);
}

/* ------------------------------------------------------------- inference */

const byId = (controls) => new Map(controls.map((c) => [c.id, c]));

/**
 * Groups motion controls into TRIOS.
 *
 * One easing select claims the nearest duration and delay. Claimed controls
 * leave every other bucket, because the replay IS the duration's visualization
 * and a bar for the same number says less, twice.
 *
 * Several easing selects now yield several motion panels. It used to be one
 * curve per widget with the rest falling through to the state line, which meant
 * a widget tuning both an enter and an exit transition could only show you one
 * of them — precisely the comparison the widget existed to support.
 */
function motionGroups(controls) {
  const groups = [];
  const claimed = new Set();

  for (const easing of controls.filter(isEasingSelect)) {
    const free = (c) => !claimed.has(c) && c !== easing;
    const durations = controls.filter((c) => free(c) && isDurationish(c));
    const duration = durations.find((c) => !DELAY_HINT.test(`${c.id} ${c.label}`));
    const delay = durations.find((c) => DELAY_HINT.test(`${c.id} ${c.label}`));

    claimed.add(easing);
    if (duration) claimed.add(duration);
    if (delay) claimed.add(delay);
    groups.push({ easing, duration, delay });
  }
  return { groups, claimed };
}

/** Positional roles for a motion panel, from whatever controls it was given. */
export function motionRoles(controls) {
  const easing = controls.find(isEasingSelect);
  const times = controls.filter((c) => c !== easing && isDurationish(c));
  return {
    easing,
    duration: times.find((c) => !DELAY_HINT.test(`${c.id} ${c.label}`)) ?? times[0],
    delay: times.find((c) => DELAY_HINT.test(`${c.id} ${c.label}`)),
  };
}

/**
 * Turns declared `preview.views` into panels, dropping anything that names a
 * control this spec does not have.
 *
 * The spec parser already rejects unknown ids, so a drop here means the spec was
 * built by hand or edited by ui_update — rendering the rest beats rendering
 * nothing.
 */
function declaredPanels(spec, controls) {
  const lookup = byId(controls);
  const panels = [];
  for (const declared of spec.preview.views) {
    const picked = (declared.controls ?? []).map((id) => lookup.get(id)).filter(Boolean);
    const view = PREVIEW_VIEWS[declared.view];
    if (!view) continue;
    if (picked.length < (view.min ?? 1)) continue;
    panels.push({
      view: declared.view,
      controls: picked,
      title: declared.title ?? view.label,
      declared: true,
    });
  }
  return panels;
}

/**
 * Infers panels for a spec that did not declare any.
 *
 * The rule, from how these widgets are actually used: anything with a MAGNITUDE
 * earns a visual — a colour, a length, a duration, a type size. Toggles, selects
 * and free text are self-describing, and forcing them into a picture would add
 * chrome that says nothing.
 */
function inferredPanels(controls) {
  const panels = [];
  const { groups, claimed } = motionGroups(controls);

  for (const group of groups) {
    const members = [group.easing, group.duration, group.delay].filter(Boolean);
    panels.push({ view: "motion", controls: members, title: PREVIEW_VIEWS.motion.label, declared: false });
  }

  const free = controls.filter((c) => !claimed.has(c));
  const colours = free.filter((c) => c.type === "color");
  const fonts = free.filter(isFontish);
  const sizes = free.filter((c) => !fonts.includes(c) && isSizeish(c));

  if (colours.length) panels.push({ view: "swatch", controls: colours, title: PREVIEW_VIEWS.swatch.label, declared: false });
  if (sizes.length) panels.push({ view: "ruler", controls: sizes, title: PREVIEW_VIEWS.ruler.label, declared: false });
  if (fonts.length) panels.push({ view: "type", controls: fonts, title: PREVIEW_VIEWS.type.label, declared: false });

  const accent = colours[0];
  const radius = sizes.find((c) => /radius/i.test(`${c.id} ${c.label}`));
  if (accent || radius || fonts.length) {
    panels.push({
      view: "card",
      controls: [accent, radius, ...fonts].filter(Boolean),
      title: PREVIEW_VIEWS.card.label,
      declared: false,
    });
  }

  // `text`, `compare` and `multiselect` are left out for one reason: they are
  // already illustrated in the SHELL, by the input itself, the matrix, or the
  // running total. Echoing them here says the same thing twice and says it
  // worse — an array flattened to "a.ts,b.ts" is not a summary.
  const selfShown = new Set(SELF_SHOWN);
  const drawn = new Set(panels.filter((p) => p.view !== "card").flatMap((p) => p.controls));
  const rest = free.filter((c) => !drawn.has(c) && !selfShown.has(c.type));
  if (rest.length) panels.push({ view: "state", controls: rest, title: PREVIEW_VIEWS.state.label, declared: false });

  return panels;
}

/**
 * The plan for a spec's generated preview: which views, in what order, over
 * which controls.
 *
 * Called by the SHELL to decide whether a frame opens, and by the FRAME to
 * decide what to build. One function, so those cannot disagree.
 */
export function planPreview(spec) {
  const controls = spec?.controls ?? [];
  const declared = Array.isArray(spec?.preview?.views) && spec.preview.views.length > 0;
  const panels = declared ? declaredPanels(spec, controls) : inferredPanels(controls);

  return {
    panels,
    /**
     * Is there anything here worth opening a frame for? The value
     * `shouldAutoPreview` returns.
     */
    drawsSomething: panels.some((panel) => PREVIEW_VIEWS[panel.view]?.draws),
    /** True when the frame would show its "nothing to preview" notice. */
    empty: panels.length === 0,
  };
}
