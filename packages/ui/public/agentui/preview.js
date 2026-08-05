/**
 * Built-in preview helpers, importable from any agent-authored preview module
 * as `import { ... } from "agentui"`.
 *
 * The premise of this whole tool is that a control you cannot see the effect of
 * is worse than a text question — you end up guessing, and the UI looks
 * authoritative while you do it. Hand-writing preview DOM for every widget is
 * too much friction for that to hold in practice, so the common shapes live
 * here and `autoPreview()` assembles them with no module at all.
 *
 * Plain ES module, no build step: it is served as a static asset to the preview
 * origin and must run in the browser exactly as written.
 *
 * Animation policy, in one line: animate ONLY agent-sourced changes
 * (meta.source === "agent") on rows that already exist — never under the
 * user's finger (drags track 1:1), never on first paint. Rows are therefore
 * persistent and keyed (syncRows below): the previous rebuild-everything
 * approach made CSS transitions dead on arrival, because a freshly-inserted
 * node has no prior computed style to animate from — the ".06s linear" width
 * transition this file carried for a while never once fired.
 */

import { spring, tween, resolveEasing, prefersReducedMotion, durations } from "./motion.js";
// The single source for "is this control spatial / a duration / an easing
// picker", shared byte-for-byte with the shell via packages/shared/src/vocab.js.
// Same-origin relative import: this file and vocab.js are both served from
// /agentui on the preview origin, so it resolves without a bundler.
import { isFontish, roundForDisplay } from "./vocab.js";
// The plan — which views, in what order, over which controls — is computed in
// shared and imported here, so the shell's "does a frame open" and this file's
// "what goes in it" are literally the same answer. See preview-plan.js.
import { PREVIEW_VIEWS, motionRoles, planPreview } from "./preview-plan.js";

/** Motion utilities ride along, so `import { spring } from "agentui"` works too. */
export * from "./motion.js";

/* --------------------------------------------------------------- utilities */

const el = (tag, props = {}, ...children) => {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "style" && typeof value === "object") Object.assign(node.style, value);
    else if (key === "class") node.className = value;
    else if (key.startsWith("on") && typeof value === "function") node.addEventListener(key.slice(2), value);
    else if (value !== undefined && value !== null) node.setAttribute(key, String(value));
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
};

/**
 * Round for display. Shared with the shell so the same value never prints two
 * ways in the two halves of one screen — it used to show 3 decimals here and 4
 * there.
 */
const round = roundForDisplay;

const BASE_CSS = `
  .aui { font: 13px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif;
         color: light-dark(#1c1c1a, #ececee); height: 100%; overflow: auto; padding: 12px; box-sizing: border-box; }
  .aui-section + .aui-section { margin-top: 16px; padding-top: 14px;
         border-top: 1px solid light-dark(#e4e4df, #34343a); }
  .aui-label { font-size: 10.5px; text-transform: uppercase; letter-spacing: .07em;
         color: light-dark(#6d6d67, #9b9ba3); margin-bottom: 8px; }
  .aui-mono { font-family: ui-monospace, Menlo, Consolas, monospace; font-size: 11.5px;
         color: light-dark(#6d6d67, #9b9ba3); }
  .aui-muted { color: light-dark(#6d6d67, #9b9ba3); }
`;

function ensureStyles(container) {
  if (container.querySelector("style[data-aui]")) return;
  container.append(el("style", { "data-aui": "1" }, BASE_CSS));
}

/** Relative luminance, for choosing readable text on a swatch. */
function readableOn(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(hex));
  if (!m) return "#000";
  const n = parseInt(m[1], 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.45 ? "#111" : "#fff";
}

function contrastRatio(a, b) {
  const lum = (hex) => {
    const m = /^#?([0-9a-f]{6})$/i.exec(String(hex));
    if (!m) return 0;
    const n = parseInt(m[1], 16);
    const [r, g, bl] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

/**
 * Keyed row reconciliation — the precondition for animating anything here.
 * Rows are keyed by label+unit (+occurrence, so duplicate labels cannot eat
 * each other) and mutated in place across updates. A row's first appearance
 * applies its values instantly; a removed entry removes its row and stops its
 * animations; surviving rows are reordered to match entry order.
 */
function syncRows(rows, container, entries, createRow) {
  const seen = new Set();
  const counts = new Map();
  const out = [];
  entries.forEach((entry, index) => {
    const base = `${entry.label}\u0000${entry.unit ?? ""}`;
    const n = counts.get(base) ?? 0;
    counts.set(base, n + 1);
    const key = n ? `${base}\u0000${n}` : base;
    seen.add(key);
    let row = rows.get(key);
    const isNew = !row;
    if (isNew) {
      row = createRow(entry);
      rows.set(key, row);
    }
    const at = container.children[index] ?? null;
    if (at !== row.el) container.insertBefore(row.el, at);
    out.push({ row, entry, isNew });
  });
  for (const [key, row] of rows) {
    if (!seen.has(key)) {
      row.dispose?.();
      row.el.remove();
      rows.delete(key);
    }
  }
  return out;
}

/**
 * A CSS transition toggled per-frame: on for agent-sourced jumps, "none" for
 * live drags and first paint. Used for colours and font sizes, where the
 * compositor interpolates better than any JS loop would.
 */
function agentTransition(animate, ...props) {
  if (!animate || prefersReducedMotion()) return "none";
  return props.map((p) => `${p} ${durations.gentle}ms cubic-bezier(0.2, 0, 0, 1)`).join(", ");
}

/* ----------------------------------------------------------------- helpers */

/**
 * Colour swatches with contrast ratios. Seeing a colour at size, next to text,
 * is the only way to judge it — a hex field tells you nothing.
 */
export function swatchGrid({ title = "Colours" } = {}) {
  let root;
  const rows = new Map();

  const createRow = ({ label }) => {
    const swatch = el("div", {
      style: {
        width: "44px", height: "30px", borderRadius: "6px",
        border: "1px solid rgba(128,128,128,.35)", flex: "none",
      },
    });
    const hex = el("div", { class: "aui-mono" });
    const badge = el("div", {
      style: { padding: "4px 10px", borderRadius: "5px", fontSize: "12px", flex: "none" },
    });
    const rowEl = el(
      "div",
      { style: { display: "flex", alignItems: "center", gap: "10px", padding: "5px 0" } },
      swatch,
      el("div", { style: { minWidth: "0", flex: "1" } }, el("div", {}, label), hex),
      badge,
    );
    return { el: rowEl, swatch, hex, badge };
  };

  return {
    mount(container) {
      ensureStyles(container);
      root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), el("div"));
      container.append(root);
    },
    update(entries, meta) {
      const animate = meta?.source === "agent";
      for (const { row, entry, isNew } of syncRows(rows, root.lastChild, entries, createRow)) {
        const value = String(entry.value);
        const fg = readableOn(value);
        // Agent pushes crossfade; drags and first paint write directly.
        const t = agentTransition(animate && !isNew, "background");
        row.swatch.style.transition = t;
        row.badge.style.transition = t;
        row.swatch.style.background = value;
        row.badge.style.background = value;
        row.badge.style.color = fg;
        row.badge.textContent = `Aa ${contrastRatio(value, fg).toFixed(1)}:1`;
        row.hex.textContent = value;
      }
    },
  };
}

/**
 * Draws lengths at true relative proportion with the real number attached.
 * A number like "760px" is meaningless in a narrow preview pane unless you can
 * see it against the others.
 */
export function ruler({ title = "Sizes" } = {}) {
  let root;
  const rows = new Map();

  // Bar width AND numeric readout are driven by one spring per row, so an
  // agent-pushed value glides while the number counts up alongside it. On
  // settle the spring delivers the exact target, so the readout ends on the
  // real number rather than a float one epsilon away.
  const paint = (row, value) => {
    row.readout.textContent = `${round(value)}${row.unit}`;
    if (row.span) {
      const pct = Math.max(2, Math.min(100, ((value - row.span.min) / (row.span.max - row.span.min)) * 100));
      row.fill.style.width = `${pct}%`;
    }
  };

  const createRow = (entry) => {
    const readout = el("span", { class: "aui-mono" });
    const fill = el("div", {
      style: { height: "100%", borderRadius: "4px", width: "0%", background: "light-dark(#888780, #6f6f78)" },
    });
    const track = el(
      "div",
      { style: { height: "8px", marginTop: "3px", borderRadius: "4px", background: "light-dark(#eeeeea, #2a2a30)" } },
      fill,
    );
    const ceiling = el("div", {
      class: "aui-mono",
      style: { fontSize: "10px", marginTop: "2px", textAlign: "right" },
    });
    const rowEl = el(
      "div",
      { style: { padding: "4px 0" } },
      el(
        "div",
        { style: { display: "flex", justifyContent: "space-between", fontSize: "12px" } },
        el("span", {}, entry.label),
        readout,
      ),
      track,
      ceiling,
    );
    const row = { el: rowEl, readout, track, fill, ceiling, span: null, unit: entry.unit ?? "" };
    row.spring = spring(0, { onUpdate: (x) => paint(row, x) });
    row.dispose = () => row.spring.stop();
    return row;
  };

  return {
    mount(container) {
      ensureStyles(container);
      root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), el("div"));
      container.append(root);
    },
    update(entries, meta) {
      // Scale against the control's DECLARED RANGE, not against the other
      // values on screen.
      //
      // Scaling by the observed maximum looks reasonable and is a lie: with a
      // single entry the max IS the value, so the bar sits at 100% no matter
      // what you drag it to. That is the precise failure this tool exists to
      // prevent — a picture that looks informative and encodes nothing — and it
      // shipped here for a while. A control with no declared bounds gets no bar
      // at all, because there is nothing honest to draw.
      const spanFor = (entry) => {
        if (typeof entry.min === "number" && typeof entry.max === "number" && entry.max > entry.min) {
          return { min: entry.min, max: entry.max };
        }
        const peers = entries.filter((e) => (e.unit ?? "") === (entry.unit ?? ""));
        if (peers.length < 2) return null; // nothing to compare against
        return { min: 0, max: Math.max(...peers.map((e) => Number(e.value) || 0)) };
      };

      const animate = meta?.source === "agent";
      for (const { row, entry, isNew } of syncRows(rows, root.lastChild, entries, createRow)) {
        row.unit = entry.unit ?? "";
        row.span = spanFor(entry);
        const hasBar = row.span !== null;
        row.track.style.display = hasBar ? "" : "none";
        row.ceiling.style.display = hasBar ? "" : "none";
        // The ceiling, so the bar is readable as a proportion of something.
        // Written from the target, not the spring: the scale is context, and
        // context sliding around mid-animation would be noise.
        if (hasBar) row.ceiling.textContent = `of ${round(row.span.max)}${row.unit}`;
        const value = Number(entry.value) || 0;
        if (!isNew && animate) row.spring.set(value);
        else row.spring.jump(value);
      }
    },
  };
}

/** Type specimen — the only honest way to judge a font size. */
export function typeScale({ title = "Type", sample = "The quick brown fox jumps over the lazy dog" } = {}) {
  let root;
  const rows = new Map();

  const createRow = () => {
    const meta = el("div", { class: "aui-mono" });
    const sampleEl = el("div", { style: { lineHeight: "1.45" } }, sample);
    return { el: el("div", { style: { padding: "5px 0" } }, meta, sampleEl), meta, sample: sampleEl };
  };

  return {
    mount(container) {
      ensureStyles(container);
      root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), el("div"));
      container.append(root);
    },
    update(entries, meta) {
      const animate = meta?.source === "agent";
      for (const { row, entry, isNew } of syncRows(rows, root.lastChild, entries, createRow)) {
        row.meta.textContent = `${entry.label} · ${round(entry.value)}${entry.unit ?? "px"}`;
        row.sample.style.transition = agentTransition(animate && !isNew, "font-size");
        row.sample.style.fontSize = `${Number(entry.value) || 14}px`;
      }
    },
  };
}

/** A representative widget card, restyled by the values. */
export function card({ title = "Sample card", heading = "A widget", subtitle = "styled with your settings" } = {}) {
  let root;
  let box;
  return {
    mount(container) {
      ensureStyles(container);
      box = el(
        "div",
        {
          style: {
            border: "1px solid light-dark(#e4e4df, #34343a)",
            background: "light-dark(#fff, #1f1f22)",
            padding: "14px", display: "flex", flexDirection: "column", gap: "10px",
          },
        },
        // Titled from the widget itself. A card headed "In context" showing a
        // question the user never asked is not their context — it is a stock
        // photo, and it teaches them to distrust the whole pane.
        el("div", { style: { fontWeight: "500" } }, heading),
        el("div", { class: "aui-muted", style: { fontSize: "12px" } }, subtitle),
        el("div", { "data-slot": "bar", style: { height: "6px", borderRadius: "3px" } }),
        el(
          "div",
          { style: { display: "flex", gap: "8px", justifyContent: "flex-end" } },
          el("button", { "data-slot": "ghost", style: { font: "inherit", fontSize: "12px", padding: "5px 12px" } }, "Hold"),
          el("button", { "data-slot": "primary", style: { font: "inherit", fontSize: "12px", padding: "5px 12px", border: "0", color: "#fff" } }, "Ship it"),
        ),
      );
      root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), box);
      container.append(root);
    },
    update({ accent, radius, fontSize, padding }, meta) {
      const animate = meta?.source === "agent";
      box.style.transition = agentTransition(animate, "border-radius", "font-size", "padding");
      const primary = box.querySelector('[data-slot="primary"]');
      const bar = box.querySelector('[data-slot="bar"]');
      primary.style.transition = agentTransition(animate, "background", "border-radius");
      bar.style.transition = agentTransition(animate, "background");
      if (accent) {
        primary.style.background = accent;
        bar.style.background = accent;
      }
      if (radius !== undefined) {
        box.style.borderRadius = `${radius}px`;
        for (const b of box.querySelectorAll("button")) {
          if (b !== primary) b.style.transition = agentTransition(animate, "border-radius");
          b.style.borderRadius = `${Math.min(radius, 10)}px`;
        }
      }
      if (fontSize !== undefined) box.style.fontSize = `${fontSize}px`;
      if (padding !== undefined) box.style.padding = `${padding}px`;
    },
  };
}

/**
 * An easing curve plotted, plus a dot that replays it at the REAL duration.
 * The plot shows the shape; the replay shows the feel — a curve alone cannot
 * convey 150ms vs 800ms, and "easeOutBack" as a word conveys nothing at all.
 *
 * update({ easing, duration, delay }, meta) — durations in ms. The replay
 * restarts only when one of the three actually changed, so dragging an
 * unrelated control does not interrupt the loop. Under prefers-reduced-motion
 * the dot parks at the end and only the static curve communicates.
 */
export function easingCurve({ title = "Motion" } = {}) {
  let root;
  let path;
  let dot;
  let readout;
  let replay = null;
  let delayTimer = 0;
  let restTimer = 0;
  let current = null;

  // PAD absorbs overshoot: back/elastic curves exceed 1 and the plot must
  // show that rather than clip it — the overshoot IS the information.
  const W = 100;
  const H = 64;
  const PAD = 12;

  function stopReplay() {
    replay?.stop();
    replay = null;
    clearTimeout(delayTimer);
    clearTimeout(restTimer);
    delayTimer = 0;
    restTimer = 0;
  }

  function drawCurve() {
    const fn = resolveEasing(current.easing);
    const pts = [];
    for (let i = 0; i <= 60; i++) {
      const t = i / 60;
      pts.push(`${i ? "L" : "M"}${((t * W)).toFixed(1)} ${(H - PAD - fn(t) * (H - 2 * PAD)).toFixed(1)}`);
    }
    path.setAttribute("d", pts.join(""));
  }

  function play() {
    stopReplay();
    if (prefersReducedMotion()) {
      dot.style.left = "100%";
      return;
    }
    dot.style.left = "0%";
    delayTimer = setTimeout(() => {
      replay = tween({
        from: 0,
        to: 1,
        duration: Math.max(1, current.duration),
        easing: current.easing,
        // No clamp: an overshooting curve carries the dot past the end and
        // back, which is exactly what choosing that curve means.
        onUpdate: (p) => {
          dot.style.left = `${p * 100}%`;
        },
        onComplete: () => {
          restTimer = setTimeout(play, 700);
        },
      });
    }, Math.min(Math.max(0, current.delay || 0), 2000));
  }

  return {
    mount(container) {
      ensureStyles(container);
      const plot = el("div");
      plot.innerHTML =
        `<svg viewBox="0 0 ${W} ${H}" style="display:block;width:100%;height:80px" preserveAspectRatio="none">` +
        `<line x1="0" y1="${H - PAD}" x2="${W}" y2="${H - PAD}" stroke="light-dark(#e4e4df, #34343a)" stroke-width="1"/>` +
        `<line x1="0" y1="${PAD}" x2="${W}" y2="${PAD}" stroke="light-dark(#e4e4df, #34343a)" stroke-width="1" stroke-dasharray="2 3"/>` +
        `<path fill="none" stroke="light-dark(#185fa5, #85b7eb)" stroke-width="1.5" vector-effect="non-scaling-stroke"/>` +
        `</svg>`;
      path = plot.querySelector("path");
      dot = el("div", {
        style: {
          position: "absolute", left: "0%", top: "50%", width: "10px", height: "10px",
          borderRadius: "50%", transform: "translate(-50%, -50%)",
          background: "light-dark(#185fa5, #85b7eb)",
        },
      });
      const track = el(
        "div",
        {
          style: {
            position: "relative", height: "4px", borderRadius: "2px", margin: "10px 5px 8px",
            background: "light-dark(#eeeeea, #2a2a30)",
          },
        },
        dot,
      );
      readout = el("div", { class: "aui-mono" });
      root = el(
        "div",
        { class: "aui-section" },
        el("div", { class: "aui-label" }, title),
        plot,
        track,
        readout,
      );
      container.append(root);
    },
    update({ easing = "ease-out", duration = 300, delay = 0 } = {}) {
      const changed =
        !current || current.easing !== easing || current.duration !== duration || current.delay !== delay;
      current = { easing, duration, delay };
      if (!changed) return;
      drawCurve();
      readout.textContent =
        `${easing} · ${round(duration)}ms` + (delay ? ` (+${round(delay)}ms delay)` : "");
      play();
    },
  };
}

/* ------------------------------------------------------------ autoPreview */

/**
 * A compact "everything else" line, for controls no view can draw.
 *
 * Deliberately a view like any other, so it goes through the same panel
 * machinery — and so `PREVIEW_VIEWS.state.draws === false` is the only place
 * that knows a state line is not a reason to open a frame.
 */
export function stateLine({ title = "Settings" } = {}) {
  let out;
  return {
    mount(container) {
      ensureStyles(container);
      const root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), el("div", { class: "aui-mono" }));
      out = root.lastChild;
      container.append(root);
    },
    update(entries) {
      out.textContent = entries.map((e) => `${e.label}: ${round(e.value)}`).join("   ");
    },
  };
}

/* ------------------------------------------------------- view registry */

/** The standard payload: one entry per control, with its declared range. */
const entriesFor = (panel, values) =>
  panel.controls.map((c) => ({
    label: c.label,
    // min/max travel with the value: without the declared range the ruler has
    // nothing honest to scale against.
    value: values[c.id] ?? c.default,
    unit: c.unit,
    min: c.min,
    max: c.max,
  }));

/**
 * How each view in PREVIEW_VIEWS is built and fed.
 *
 * The vocabulary lives in shared (preview-plan.js) because the SHELL has to
 * reason about it too; the DOM lives here because only the frame has a DOM.
 * Same split as CONTROLS vs CONTROL_INPUTS, and for the same reason.
 *
 * A name in PREVIEW_VIEWS with no entry here is a bug that opens an empty
 * frame, so preview-agreement.test.ts asserts the two key sets match.
 */
export const VIEW_IMPLS = {
  motion: {
    create: (panel) => easingCurve({ title: panel.title }),
    payload: (panel, values) => {
      const { easing, duration, delay } = motionRoles(panel.controls);
      const num = (c) => Number(values[c.id] ?? c.default);
      // Seconds are a legitimate unit for a duration; the curve replays in ms.
      const toMs = (c) => (c.unit === "s" ? num(c) * 1000 : num(c));
      return {
        easing: easing ? String(values[easing.id] ?? easing.default) : "ease-out",
        duration: duration ? toMs(duration) : 300,
        delay: delay ? toMs(delay) : 0,
      };
    },
  },
  swatch: { create: (panel) => swatchGrid({ title: panel.title }), payload: entriesFor },
  ruler: { create: (panel) => ruler({ title: panel.title }), payload: entriesFor },
  type: { create: (panel) => typeScale({ title: panel.title }), payload: entriesFor },
  card: {
    create: (panel, spec) => card({ title: panel.title, heading: spec?.title ?? "A widget" }),
    payload: (panel, values) => {
      const at = (pred) => panel.controls.find(pred);
      const accent = at((c) => c.type === "color");
      const radius = at((c) => /radius/i.test(`${c.id} ${c.label}`));
      const font = at((c) => isFontish(c));
      const num = (c) => Number(values[c.id] ?? c.default);
      return {
        accent: accent ? values[accent.id] ?? accent.default : undefined,
        radius: radius ? num(radius) : undefined,
        fontSize: font ? num(font) : undefined,
      };
    },
  },
  state: { create: (panel) => stateLine({ title: panel.title }), payload: entriesFor },
};

/**
 * Builds a preview from a plan.
 *
 * Every panel is built the same way, so "two motion groups" and "a declared
 * arrangement" are not special cases — they are just different plans. This used
 * to be a fixed sequence of named buckets, which is why there could only ever
 * be one motion curve per widget.
 */
export function autoPreview(spec) {
  const { panels } = planPreview(spec);
  const built = panels
    .map((panel) => ({ panel, impl: VIEW_IMPLS[panel.view] }))
    // A panel whose view has no implementation is dropped rather than thrown:
    // a spec authored against a newer build should degrade, not blank the pane.
    .filter((entry) => entry.impl)
    .map((entry) => ({ ...entry, view: entry.impl.create(entry.panel, spec) }));

  return {
    setup({ container }) {
      ensureStyles(container);
      const wrap = el("div", { class: "aui" });
      container.append(wrap);
      for (const { view } of built) view.mount(wrap);
      if (!built.length) {
        wrap.append(el("div", { class: "aui-muted" }, "Nothing here has a visual effect to preview."));
      }
    },

    update(values, meta) {
      for (const { panel, impl, view } of built) view.update(impl.payload(panel, values), meta);
    },
  };
}

export default { swatchGrid, ruler, typeScale, card, easingCurve, stateLine, autoPreview };
