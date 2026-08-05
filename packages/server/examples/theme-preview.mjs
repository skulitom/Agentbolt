// Preview module for the "shell visual tuning" widget.
//
// It renders a miniature of the real session UI — header, pill, diff, a slider,
// buttons — using the values as live CSS variables. Dragging a slider restyles
// this instantly, so "760px" and "compact" stop being abstract numbers and
// become something you can look at and judge.
//
// No imports: plain DOM is the right tool here. `three` is available via the
// import map when a preview genuinely needs 3D.
const DENSITY = {
  compact: { pad: 10, gap: 7, rowPad: 6, scale: 0.92 },
  comfortable: { pad: 15, gap: 12, rowPad: 9, scale: 1 },
  roomy: { pad: 22, gap: 18, rowPad: 13, scale: 1.06 },
};

export default {
  setup({ container }) {
    container.innerHTML = `
      <div class="stage">
        <div class="ruler"><span class="ruler-label"></span></div>
        <div class="card">
          <div class="head">
            <div>
              <div class="title">Approve the auth refactor?</div>
              <div class="sub">a widget rendered with your current settings</div>
            </div>
            <span class="pill">the agent is waiting</span>
          </div>
          <div class="diff">
            <div class="diff-row del">- await redis.set(key, token);</div>
            <div class="diff-row add">+ await redis.set(key, token, { EX: TTL });</div>
          </div>
          <div class="field">
            <div class="field-head"><span>Session TTL</span><span class="mono">3600s</span></div>
            <input type="range" min="0" max="100" value="62" />
          </div>
          <div class="field">
            <div class="field-head"><span>Rotate on refresh</span></div>
            <label class="toggle"><input type="checkbox" checked /> on</label>
          </div>
          <div class="foot">
            <span class="status"><i class="dot"></i>connected</span>
            <span class="btns">
              <button class="ghost">Hold</button>
              <button class="primary">Ship it</button>
            </span>
          </div>
        </div>
      </div>`;

    const style = document.createElement("style");
    style.textContent = `
      .stage { height: 100%; display: flex; flex-direction: column; align-items: center;
               justify-content: center; gap: 6px; padding: 10px; overflow: hidden; }
      .ruler { position: relative; height: 12px; display: flex; align-items: center;
               justify-content: center; width: var(--w); max-width: 100%; }
      .ruler::before, .ruler::after { content: ""; position: absolute; top: 50%; width: 100%;
               border-top: 1px dashed light-dark(#c9c9c2, #4a4a52); }
      .ruler-label { position: relative; padding: 0 6px; font-size: 10.5px;
               background: light-dark(#fbfbfa, #171719); color: light-dark(#6d6d67, #9b9ba3); }
      .card { width: var(--w); max-width: 100%; box-sizing: border-box;
              background: light-dark(#fff, #1f1f22);
              border: 1px solid light-dark(#e4e4df, #34343a);
              border-radius: calc(var(--r) * 1.5px); padding: calc(var(--pad) * 1px);
              display: flex; flex-direction: column; gap: calc(var(--gap) * 1px);
              font-size: calc(var(--fs) * 1px); transform: scale(var(--scale)); transform-origin: top center; }
      .head { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; }
      .title { font-weight: 600; }
      .sub { font-size: .8em; color: light-dark(#6d6d67, #9b9ba3); margin-top: 2px; }
      .pill { flex: none; font-size: .72em; padding: 3px 9px; border-radius: 999px;
              background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--accent); }
      .diff { border: 1px solid light-dark(#e4e4df, #34343a); border-radius: calc(var(--r) * 1px);
              overflow: hidden; font-family: ui-monospace, Menlo, Consolas, monospace; font-size: .76em; }
      .diff-row { padding: calc(var(--rowPad) * .45px) 8px; white-space: pre; }
      .diff-row.del { background: light-dark(#fceaea, #331b1b); color: light-dark(#a32d2d, #f09595); }
      .diff-row.add { background: light-dark(#eaf3de, #1b2c14); color: light-dark(#3b6d11, #97c459); }
      .field-head { display: flex; justify-content: space-between; font-size: .85em;
                    margin-bottom: calc(var(--rowPad) * .35px); }
      .mono { font-family: ui-monospace, Menlo, Consolas, monospace;
              color: light-dark(#6d6d67, #9b9ba3); }
      input[type=range] { width: 100%; accent-color: var(--accent); margin: 0; }
      .toggle { display: inline-flex; align-items: center; gap: 6px; font-size: .85em;
                color: light-dark(#6d6d67, #9b9ba3); }
      .toggle input { accent-color: var(--accent); }
      .foot { display: flex; align-items: center; justify-content: space-between;
              padding-top: calc(var(--gap) * .6px); border-top: 1px solid light-dark(#e4e4df, #34343a); }
      .status { display: inline-flex; align-items: center; gap: 6px; font-size: .72em;
                color: light-dark(#6d6d67, #9b9ba3); }
      .dot { width: 6px; height: 6px; border-radius: 50%; background: #3b6d11; }
      .btns { display: inline-flex; gap: 6px; }
      button { font: inherit; font-size: .82em; cursor: pointer;
               padding: calc(var(--rowPad) * .5px) calc(var(--pad) * .62px);
               border-radius: calc(var(--r) * 1px);
               border: 1px solid light-dark(#d0d0c9, #45454d);
               background: light-dark(#fff, #1f1f22); color: inherit; }
      button.primary { background: var(--accent); border-color: transparent; color: #fff; }
      .seq { font-size: .68em; color: light-dark(#6d6d67, #9b9ba3);
             font-family: ui-monospace, Menlo, Consolas, monospace; margin-left: 6px; }`;
    container.appendChild(style);

    this.root = container.querySelector(".stage");
    this.card = container.querySelector(".card");
    this.rulerLabel = container.querySelector(".ruler-label");
    this.status = container.querySelector(".status");
  },

  update(values) {
    const density = DENSITY[values.density] ?? DENSITY.comfortable;
    const width = Number(values.content_width ?? 760);

    // The preview pane is far narrower than a real window, so show the chosen
    // width proportionally and label the true number — otherwise "1100px" and
    // "760px" would look identical in a 700px frame.
    const shown = Math.round((width / 1100) * 100);

    const s = this.root.style;
    s.setProperty("--accent", String(values.accent ?? "#185fa5"));
    s.setProperty("--w", `${shown}%`);
    s.setProperty("--r", String(values.corner_radius ?? 7));
    s.setProperty("--fs", String(values.base_font_size ?? 15));
    s.setProperty("--pad", String(density.pad));
    s.setProperty("--gap", String(density.gap));
    s.setProperty("--rowPad", String(density.rowPad));
    s.setProperty("--scale", String(density.scale));

    this.rulerLabel.textContent = `${width}px content column`;

    const existing = this.status.querySelector(".seq");
    if (values.show_seq) {
      if (!existing) {
        const seq = document.createElement("span");
        seq.className = "seq";
        seq.textContent = "seq 42";
        this.status.appendChild(seq);
      }
    } else if (existing) {
      existing.remove();
    }
  },
};
