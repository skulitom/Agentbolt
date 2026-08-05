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
 */

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

/** Round for display. JS float maths leaks 0.30000000000000004 into the UI. */
const round = (value, places = 3) => {
  if (typeof value !== "number" || !Number.isFinite(value)) return String(value ?? "");
  const factor = 10 ** places;
  const rounded = Math.round(value * factor) / factor;
  return Number.isInteger(rounded) ? String(rounded) : String(rounded);
};

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

/* ----------------------------------------------------------------- helpers */

/**
 * Colour swatches with contrast ratios. Seeing a colour at size, next to text,
 * is the only way to judge it — a hex field tells you nothing.
 */
export function swatchGrid({ title = "Colours" } = {}) {
  let root;
  return {
    mount(container) {
      ensureStyles(container);
      root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), el("div"));
      container.append(root);
    },
    update(entries) {
      const grid = root.lastChild;
      grid.replaceChildren(
        ...entries.map(({ label, value }) => {
          const fg = readableOn(value);
          const ratio = contrastRatio(value, fg);
          return el(
            "div",
            { style: { display: "flex", alignItems: "center", gap: "10px", padding: "5px 0" } },
            el("div", {
              style: {
                width: "44px", height: "30px", borderRadius: "6px", background: String(value),
                border: "1px solid rgba(128,128,128,.35)", flex: "none",
              },
            }),
            el(
              "div",
              { style: { minWidth: "0", flex: "1" } },
              el("div", {}, label),
              el("div", { class: "aui-mono" }, String(value)),
            ),
            el(
              "div",
              {
                style: {
                  background: String(value), color: fg, padding: "4px 10px", borderRadius: "5px",
                  fontSize: "12px", flex: "none",
                },
              },
              `Aa ${ratio.toFixed(1)}:1`,
            ),
          );
        }),
      );
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
  return {
    mount(container) {
      ensureStyles(container);
      root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), el("div"));
      container.append(root);
    },
    update(entries) {
      // Scale within each unit, never across them. Sharing one scale between a
      // 4000ms duration and a 12px radius crushes every pixel value to a stub
      // and makes the bars actively misleading.
      const maxByUnit = new Map();
      for (const entry of entries) {
        const unit = entry.unit ?? "";
        maxByUnit.set(unit, Math.max(maxByUnit.get(unit) ?? 1, Number(entry.value) || 0));
      }

      root.lastChild.replaceChildren(
        ...entries.map(({ label, value, unit }) =>
          el(
            "div",
            { style: { padding: "4px 0" } },
            el(
              "div",
              { style: { display: "flex", justifyContent: "space-between", fontSize: "12px" } },
              el("span", {}, label),
              el("span", { class: "aui-mono" }, `${round(value)}${unit ?? ""}`),
            ),
            el("div", {
              style: {
                height: "8px", marginTop: "3px", borderRadius: "4px",
                background: "light-dark(#e4e4df, #34343a)",
                width: `${Math.max(2, ((Number(value) || 0) / (maxByUnit.get(unit ?? "") || 1)) * 100)}%`,
                transition: "width .06s linear",
              },
            }),
          ),
        ),
      );
    },
  };
}

/** Type specimen — the only honest way to judge a font size. */
export function typeScale({ title = "Type", sample = "The quick brown fox jumps over the lazy dog" } = {}) {
  let root;
  return {
    mount(container) {
      ensureStyles(container);
      root = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, title), el("div"));
      container.append(root);
    },
    update(entries) {
      root.lastChild.replaceChildren(
        ...entries.map(({ label, value, unit }) =>
          el(
            "div",
            { style: { padding: "5px 0" } },
            el("div", { class: "aui-mono" }, `${label} · ${round(value)}${unit ?? "px"}`),
            el("div", { style: { fontSize: `${Number(value) || 14}px`, lineHeight: "1.45" } }, sample),
          ),
        ),
      );
    },
  };
}

/** A representative widget card, restyled by the values. */
export function card({ title = "In context" } = {}) {
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
        el("div", { style: { fontWeight: "500" } }, "Approve the auth refactor?"),
        el("div", { class: "aui-muted", style: { fontSize: "12px" } }, "rendered with your current settings"),
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
    update({ accent, radius, fontSize, padding }) {
      if (accent) {
        box.querySelector('[data-slot="primary"]').style.background = accent;
        box.querySelector('[data-slot="bar"]').style.background = accent;
      }
      if (radius !== undefined) {
        box.style.borderRadius = `${radius}px`;
        for (const b of box.querySelectorAll("button")) b.style.borderRadius = `${Math.min(radius, 10)}px`;
      }
      if (fontSize !== undefined) box.style.fontSize = `${fontSize}px`;
      if (padding !== undefined) box.style.padding = `${padding}px`;
    },
  };
}

/* ------------------------------------------------------------ autoPreview */

const COLOUR_HINTS = /colou?r|tint|accent|hue|shade|background|foreground/i;
const SIZE_HINTS = /width|height|size|radius|spacing|gap|padding|margin|length|distance|offset|duration|delay/i;
const FONT_HINTS = /font|type|text.*size|size.*text/i;

/**
 * Picks a preview for a spec that did not bring its own.
 *
 * The rule, from how these widgets are actually used: anything with a MAGNITUDE
 * earns a visual — a colour, a length, a duration. Toggles and selects are
 * self-describing and get a compact state line instead of being forced into a
 * picture that would say nothing.
 */
export function autoPreview(spec) {
  const controls = spec?.controls ?? [];

  const colours = controls.filter((c) => c.type === "color");
  const fonts = controls.filter(
    (c) => (c.type === "slider" || c.type === "number") && FONT_HINTS.test(`${c.id} ${c.label}`),
  );
  const sizes = controls.filter(
    (c) =>
      (c.type === "slider" || c.type === "number") &&
      !fonts.includes(c) &&
      (c.unit || SIZE_HINTS.test(`${c.id} ${c.label}`)),
  );
  const others = controls.filter(
    (c) => !colours.includes(c) && !fonts.includes(c) && !sizes.includes(c) && c.type !== "text",
  );

  const parts = [];
  if (colours.length) parts.push({ kind: "colours", controls: colours, view: swatchGrid({ title: "Colours" }) });
  if (sizes.length) parts.push({ kind: "sizes", controls: sizes, view: ruler({ title: "Sizes" }) });
  if (fonts.length) parts.push({ kind: "fonts", controls: fonts, view: typeScale({ title: "Type" }) });

  const accent = colours[0];
  const radius = sizes.find((c) => /radius/i.test(`${c.id} ${c.label}`));
  const showCard = Boolean(accent || radius || fonts.length);

  let cardView;
  let stateLine;

  return {
    setup({ container }) {
      ensureStyles(container);
      const wrap = el("div", { class: "aui" });
      container.append(wrap);

      for (const part of parts) part.view.mount(wrap);

      if (showCard) {
        cardView = card({ title: "In context" });
        cardView.mount(wrap);
      }

      if (others.length) {
        stateLine = el("div", { class: "aui-section" }, el("div", { class: "aui-label" }, "Settings"), el("div", { class: "aui-mono" }));
        wrap.append(stateLine);
      }

      if (!parts.length && !showCard && !others.length) {
        wrap.append(el("div", { class: "aui-muted" }, "Nothing here has a visual effect to preview."));
      }
    },

    update(values) {
      for (const part of parts) {
        part.view.update(
          part.controls.map((c) => ({ label: c.label, value: values[c.id] ?? c.default, unit: c.unit })),
        );
      }
      if (cardView) {
        cardView.update({
          accent: accent ? values[accent.id] ?? accent.default : undefined,
          radius: radius ? Number(values[radius.id] ?? radius.default) : undefined,
          fontSize: fonts[0] ? Number(values[fonts[0].id] ?? fonts[0].default) : undefined,
        });
      }
      if (stateLine) {
        stateLine.lastChild.textContent = others
          .map((c) => `${c.label}: ${round(values[c.id] ?? c.default)}`)
          .join("   ");
      }
    },
  };
}

export default { swatchGrid, ruler, typeScale, card, autoPreview };
