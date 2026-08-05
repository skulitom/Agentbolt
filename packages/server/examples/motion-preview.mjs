// Preview module for motion-demo.mjs — the dogfood consumer of the "agentui"
// motion utilities (spring/tween/easingCurve) that motion-demo tunes the shell
// with. Plain DOM, no three.js: motion needs nothing heavier than a div.
//
// It follows the meta.source rule it exists to demonstrate: the readout spring
// GLIDES when the agent pushes a value (meta.source === "agent") and JUMPS
// while the human drags, so nothing ever animates under the finger.
import { spring, tween, resolveEasing, easingCurve, stopAll } from "agentui";

const el = (tag, style = {}, text) => {
  const node = document.createElement(tag);
  Object.assign(node.style, style);
  if (text) node.textContent = text;
  return node;
};

const section = (title) => {
  const wrap = el("div", { marginBottom: "16px" });
  wrap.append(
    el(
      "div",
      {
        fontSize: "10.5px", textTransform: "uppercase", letterSpacing: ".07em",
        color: "light-dark(#6d6d67, #9b9ba3)", marginBottom: "8px",
      },
      title,
    ),
  );
  return wrap;
};

export default {
  curve: null,
  state: { press: 90, hover: 140, reveal: 220, easing: "ease-out" },
  timers: [],
  revealSpring: null,

  setup({ container }) {
    const root = el("div", {
      font: '13px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif',
      color: "light-dark(#1c1c1a, #ececee)",
      padding: "12px", height: "100%", overflow: "auto", boxSizing: "border-box",
    });
    container.append(root);

    // The chosen easing, plotted and replayed at the reveal duration.
    const curveWrap = section("The curve, at reveal speed");
    this.curve = easingCurve({ title: "" });
    this.curve.mount(curveWrap);
    root.append(curveWrap);

    // A fake primary button that presses itself, so press_duration is felt
    // rather than read.
    const pressWrap = section("Press — replays automatically");
    this.pressBtn = el("button", {
      font: "inherit", fontSize: "12px", padding: "6px 14px", border: "0",
      borderRadius: "7px", color: "#fff", background: "light-dark(#185fa5, #85b7eb)",
      transformOrigin: "center",
    }, "Ship it");
    pressWrap.append(this.pressBtn);
    root.append(pressWrap);

    // A row that tints in hover_duration, then clears.
    const hoverWrap = section("Hover");
    this.hoverRow = el("div", {
      padding: "8px 10px", borderRadius: "7px",
      border: "1px solid light-dark(#e4e4df, #34343a)",
    }, "A row being hovered");
    hoverWrap.append(this.hoverRow);
    root.append(hoverWrap);

    // Reveal: a card that rises in at reveal_duration with the chosen easing.
    const revealWrap = section("Reveal");
    this.revealCard = el("div", {
      padding: "10px 12px", borderRadius: "7px",
      border: "1px solid light-dark(#e4e4df, #34343a)",
      background: "light-dark(#fff, #1f1f22)",
    }, "Yes, finish");
    revealWrap.append(this.revealCard);
    root.append(revealWrap);

    // Numeric readout driven by a spring — glides on agent pushes only.
    this.readout = el("div", {
      fontFamily: "ui-monospace, Menlo, Consolas, monospace", fontSize: "11.5px",
      color: "light-dark(#6d6d67, #9b9ba3)", marginTop: "4px",
    });
    root.append(this.readout);
    this.revealSpring = spring(this.state.reveal, {
      onUpdate: (x) => {
        this.readout.textContent =
          `press ${this.state.press}ms · hover ${this.state.hover}ms · reveal ${Math.round(x)}ms · ${this.state.easing}`;
      },
    });

    // Replay loops. Each runs its tween with the CURRENT settings, so a drag
    // is picked up on the very next cycle without restarting anything.
    const loop = (ms, fn) => this.timers.push(setInterval(fn, ms));
    loop(1600, () => {
      const { press, easing } = this.state;
      tween({ from: 1, to: 0.94, duration: press, easing,
        onUpdate: (s) => { this.pressBtn.style.transform = `scale(${s})`; },
        onComplete: () => tween({ from: 0.94, to: 1, duration: press, easing,
          onUpdate: (s) => { this.pressBtn.style.transform = `scale(${s})`; } }),
      });
    });
    loop(2400, () => {
      const { hover, easing } = this.state;
      tween({ from: 0, to: 1, duration: hover, easing,
        onUpdate: (a) => { this.hoverRow.style.background = `color-mix(in srgb, light-dark(#f4f4f1, #26262a) ${a * 100}%, transparent)`; },
        onComplete: () => setTimeout(() => tween({ from: 1, to: 0, duration: hover, easing,
          onUpdate: (a) => { this.hoverRow.style.background = `color-mix(in srgb, light-dark(#f4f4f1, #26262a) ${a * 100}%, transparent)`; } }), 700),
      });
    });
    loop(3000, () => {
      const { reveal, easing } = this.state;
      tween({ from: 0, to: 1, duration: reveal, easing,
        onUpdate: (p) => {
          this.revealCard.style.opacity = String(p);
          this.revealCard.style.transform = `translateY(${(1 - p) * 6}px)`;
        },
      });
    });
  },

  update(values, meta) {
    this.state = {
      press: Number(values.press_duration ?? 90),
      hover: Number(values.hover_duration ?? 140),
      reveal: Number(values.reveal_duration ?? 220),
      easing: String(values.easing ?? "ease-out"),
    };
    this.curve.update({ easing: this.state.easing, duration: this.state.reveal });
    // The rule in action: agent pushes glide, user input tracks 1:1.
    if (meta?.source === "agent") this.revealSpring.set(this.state.reveal);
    else this.revealSpring.jump(this.state.reveal);
  },

  dispose() {
    for (const t of this.timers) clearInterval(t);
    stopAll();
  },
};
