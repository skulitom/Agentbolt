import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { Control, Values } from "@agentui/shared";
import { useSession } from "../store.js";

interface Props {
  control: Control;
  disabled: boolean;
}

/**
 * One renderer per control type.
 *
 * The commit discipline is the important part: continuous inputs update local
 * state on every event (so the preview is live) but only *commit* on pointer
 * release or blur. Committing is what wakes the agent, so committing on every
 * frame would make the async model unusable.
 */
export function ControlRenderer({ control, disabled }: Props) {
  const value = useSession((s) => s.values[control.id]);
  const setLocalValue = useSession((s) => s.setLocalValue);
  const commit = useSession((s) => s.commit);
  const touched = useSession((s) => s.touched.has(control.id));
  const resetControl = useSession((s) => s.resetControl);
  const pushSeq = useSession((s) => s.pushSeq);
  const cardRef = useRef<HTMLDivElement>(null);

  // Flash the card when the agent pushes a new value into THIS control, so a
  // programmatic change is visible even when the eye is elsewhere. WAAPI rather
  // than a class toggle: restarts cleanly on rapid pushes and leaves no state
  // behind. Purely visual — nothing here re-enters the value pipeline.
  useEffect(() => {
    if (pushSeq === 0 || !useSession.getState().pushedIds.has(control.id)) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    cardRef.current?.animate(
      { backgroundColor: ["var(--accent-bg)", "transparent"] },
      { duration: 500, easing: "ease-out" },
    );
  }, [pushSeq, control.id]);

  const set = (next: Values[string]) => setLocalValue(control.id, next);

  // compare and multiselect are GROUPS of inputs, not one input: `for=` would
  // point at nothing. They get a labelled group instead, wired up below.
  const isGroup = control.type === "compare" || control.type === "multiselect";

  return (
    <div ref={cardRef} className="control" data-type={control.type} data-touched={touched}>
      <div className="control-head">
        <label
          className="control-label"
          id={isGroup ? `l-${control.id}` : undefined}
          htmlFor={isGroup ? undefined : `c-${control.id}`}
        >
          {control.label}
        </label>
        <span className="control-meta">
          {control.type === "slider" || control.type === "number" ? (
            <span className="control-value">
              {formatNumber(value)}
              {"unit" in control && control.unit ? <span className="unit">{control.unit}</span> : null}
            </span>
          ) : null}
          {/* Untouched means "no opinion expressed", and it is worth saying so
              plainly — otherwise an agent reads an authored default as a choice. */}
          {!touched ? <span className="tag-default">suggested</span> : null}
          {touched && !disabled ? (
            <button
              type="button"
              className="reset"
              title={`Back to the suggested ${formatDefault(control)}`}
              aria-label={`Reset ${control.label} to the suggested value`}
              onClick={() => resetControl(control.id)}
            >
              ↺
            </button>
          ) : null}
        </span>
      </div>

      {control.description ? <p className="control-description">{control.description}</p> : null}

      <div className="control-body">{renderInput()}</div>
    </div>
  );

  function renderInput() {
    switch (control.type) {
      case "slider":
        return (
          <input
            id={`c-${control.id}`}
            type="range"
            min={control.min}
            max={control.max}
            step={control.step}
            value={Number(value ?? control.default)}
            disabled={disabled}
            onChange={(e) => set(Number(e.target.value))}
            onPointerUp={commit}
            onKeyUp={commit}
            onBlur={commit}
          />
        );

      case "number":
        return (
          <input
            id={`c-${control.id}`}
            type="number"
            min={control.min}
            max={control.max}
            step={control.step ?? "any"}
            value={Number(value ?? control.default)}
            disabled={disabled}
            onChange={(e) => set(Number(e.target.value))}
            onBlur={commit}
          />
        );

      case "select":
        return (
          <select
            id={`c-${control.id}`}
            value={String(value ?? control.default)}
            disabled={disabled}
            onChange={(e) => {
              set(e.target.value);
              // Discrete choices have no drag phase, so the change *is* the commit.
              queueMicrotask(commit);
            }}
          >
            {control.options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label ?? option.value}
              </option>
            ))}
          </select>
        );

      case "toggle":
        return (
          <label className="switch">
            <input
              id={`c-${control.id}`}
              type="checkbox"
              checked={Boolean(value ?? control.default)}
              disabled={disabled}
              onChange={(e) => {
                set(e.target.checked);
                queueMicrotask(commit);
              }}
            />
            <span>{value ? "on" : "off"}</span>
          </label>
        );

      case "color":
        return (
          <div className="color-row">
            <input
              id={`c-${control.id}`}
              type="color"
              value={String(value ?? control.default)}
              disabled={disabled}
              onChange={(e) => set(e.target.value)}
              onBlur={commit}
            />
            <code>{String(value ?? control.default)}</code>
          </div>
        );

      case "text":
        // A multiline box is the biggest thing on a page and reads as homework.
        // Collapse it: open only when it already has content, or on request.
        return control.multiline ? (
          <details className="long-text" open={String(value ?? "").length > 0}>
            <summary>{String(value ?? "").trim() ? truncate(String(value)) : (control.placeholder ?? "Add text")}</summary>
            <textarea
              id={`c-${control.id}`}
              rows={4}
              placeholder={control.placeholder}
              value={String(value ?? "")}
              disabled={disabled}
              onChange={(e) => set(e.target.value)}
              onBlur={commit}
            />
          </details>
        ) : (
          <input
            id={`c-${control.id}`}
            type="text"
            placeholder={control.placeholder}
            value={String(value ?? "")}
            disabled={disabled}
            onChange={(e) => set(e.target.value)}
            onBlur={commit}
          />
        );

      case "vector": {
        const current = Array.isArray(value) ? (value as number[]) : control.default;
        const axes = control.labels ?? (control.size === 3 ? ["x", "y", "z"] : ["x", "y"]);
        return (
          <div className="vector-row">
            {Array.from({ length: control.size }, (_, i) => (
              <label key={i} className="vector-axis">
                <span>{axes[i] ?? String(i)}</span>
                <input
                  type="number"
                  min={control.min}
                  max={control.max}
                  step={control.step ?? "any"}
                  value={current[i] ?? 0}
                  disabled={disabled}
                  onChange={(e) => {
                    const next = [...current];
                    next[i] = Number(e.target.value);
                    set(next);
                  }}
                  onBlur={commit}
                />
              </label>
            ))}
          </div>
        );
      }

      case "compare":
        return (
          <Compare
            control={control}
            value={String(value ?? control.default)}
            disabled={disabled}
            onPick={(next) => {
              set(next);
              queueMicrotask(commit);
            }}
          />
        );

      case "multiselect": {
        const chosen = new Set(Array.isArray(value) ? (value as string[]) : control.default);
        const weighted = control.options.some((o) => o.weight !== undefined);
        const everything = control.options.reduce((sum, o) => sum + (o.weight ?? 0), 0);
        const picked = control.options.reduce((sum, o) => (chosen.has(o.value) ? sum + (o.weight ?? 0) : sum), 0);
        const heaviest = Math.max(...control.options.map((o) => o.weight ?? 0), 0);
        return (
          <div className="multiselect" role="group" aria-labelledby={`l-${control.id}`}>
            {control.options.map((option) => (
              <label key={option.value} className="check-row">
                <input
                  type="checkbox"
                  checked={chosen.has(option.value)}
                  disabled={disabled}
                  onChange={(e) => {
                    const next = new Set(chosen);
                    if (e.target.checked) next.add(option.value);
                    else next.delete(option.value);
                    // Kept in the agent's declared order, not click order, so the
                    // value an agent reads back is stable across two identical
                    // selections made in a different sequence.
                    set(control.options.filter((o) => next.has(o.value)).map((o) => o.value));
                    queueMicrotask(commit);
                  }}
                />
                <span className="check-main">
                  <span className="check-label">{option.label ?? option.value}</span>
                  {option.note ? <span className="check-note">{option.note}</span> : null}
                </span>
                {option.weight !== undefined ? (
                  <span className="check-weight">
                    {/* The bar is what makes "this one is most of the cost"
                        visible without adding six numbers up in your head. */}
                    <span className="check-bar" aria-hidden="true">
                      <span style={{ width: heaviest > 0 ? `${(option.weight / heaviest) * 100}%` : "0%" }} />
                    </span>
                    {formatNumber(option.weight)}
                    {control.weightUnit ? <span className="unit"> {control.weightUnit}</span> : null}
                  </span>
                ) : null}
              </label>
            ))}
            {/* With no weights this is a form, not a chart — and it says so
                rather than dressing a count up as a measurement. */}
            <div className="multiselect-foot">
              <span>
                <b>{chosen.size}</b> of {control.options.length} selected
              </span>
              {weighted ? (
                <span className="multiselect-total">
                  <span className="check-bar check-bar-total" aria-hidden="true">
                    <span style={{ width: everything > 0 ? `${(picked / everything) * 100}%` : "0%" }} />
                  </span>
                  <b>{formatNumber(picked)}</b>
                  {control.weightUnit ? ` ${control.weightUnit}` : ""}
                  <span className="muted"> of {formatNumber(everything)}</span>
                </span>
              ) : null}
            </div>
          </div>
        );
      }
    }
  }
}

/**
 * The comparison matrix: axes down, options across, one radio per column.
 *
 * Below ~720px the columns have nowhere to go, so the same data renders as one
 * card per option. Two DOM shapes rather than CSS gymnastics, because a matrix
 * that reflows into overlapping columns is worse than either layout.
 */
function Compare({
  control,
  value,
  disabled,
  onPick,
}: {
  control: Extract<Control, { type: "compare" }>;
  value: string;
  disabled: boolean;
  onPick: (next: string) => void;
}) {
  const narrow = useNarrow();
  const name = `c-${control.id}`;

  const radio = (option: Extract<Control, { type: "compare" }>["options"][number]) => (
    <input
      type="radio"
      name={name}
      value={option.value}
      checked={value === option.value}
      disabled={disabled}
      onChange={() => onPick(option.value)}
    />
  );

  if (narrow) {
    return (
      <div className="compare-cards" role="radiogroup" aria-labelledby={`l-${control.id}`}>
        {control.options.map((option) => (
          <label key={option.value} className="compare-card" data-selected={value === option.value}>
            <span className="compare-card-head">
              {radio(option)}
              <span className="compare-option-label">{option.label ?? option.value}</span>
            </span>
            {option.summary ? <span className="compare-summary">{option.summary}</span> : null}
            <dl className="compare-card-axes">
              {control.axes.map((axis) => (
                <div key={axis.id}>
                  <dt>{axis.label}</dt>
                  <dd>
                    <Cell axis={axis} option={option} />
                  </dd>
                </div>
              ))}
            </dl>
          </label>
        ))}
      </div>
    );
  }

  const style = { ["--compare-cols" as string]: String(control.options.length) } as CSSProperties;
  return (
    <div className="compare" style={style} role="radiogroup" aria-labelledby={`l-${control.id}`}>
      <div className="compare-row compare-options">
        <span className="compare-axis-label" />
        {control.options.map((option) => (
          <label key={option.value} className="compare-option" data-selected={value === option.value}>
            {radio(option)}
            <span className="compare-option-main">
              <span className="compare-option-label">{option.label ?? option.value}</span>
              {option.summary ? <span className="compare-summary">{option.summary}</span> : null}
            </span>
          </label>
        ))}
      </div>
      {control.axes.map((axis) => (
        <div key={axis.id} className="compare-row">
          <span className="compare-axis-label">{axis.label}</span>
          {control.options.map((option) => (
            <span key={option.value} className="compare-cell" data-selected={value === option.value}>
              <Cell axis={axis} option={option} />
            </span>
          ))}
        </div>
      ))}
    </div>
  );
}

type CompareAxis = Extract<Control, { type: "compare" }>["axes"][number];
type CompareOption = Extract<Control, { type: "compare" }>["options"][number];

/**
 * One cell. `better` is what decides whether anything gets coloured at all —
 * the default is "none", and a matrix that tints every column implies a winner
 * the agent has not earned.
 */
function Cell({ axis, option }: { axis: CompareAxis; option: CompareOption }) {
  if (axis.template !== undefined) {
    return <code className="compare-code">{axis.template.replaceAll("{}", option.value)}</code>;
  }
  const cell = option.cells[axis.id];
  if (cell === undefined) return <span className="compare-missing">—</span>;

  if (axis.kind === "bool") {
    const yes = cell === true;
    const tone = axis.better === "none" ? "none" : (yes === (axis.better === "high") ? "good" : "bad");
    return (
      <span className="compare-bool" data-tone={tone} title={yes ? "yes" : "no"}>
        {yes ? "✓" : "✗"}
      </span>
    );
  }

  const text = String(cell);
  if (axis.kind === "code") return <code className="compare-code">{text}</code>;

  if (axis.kind === "scale" && axis.scale) {
    const rank = axis.scale.indexOf(text);
    const best = axis.better === "low" ? 0 : axis.scale.length - 1;
    const worst = axis.better === "low" ? axis.scale.length - 1 : 0;
    const tone = axis.better === "none" || rank < 0 ? "none" : rank === best ? "good" : rank === worst ? "bad" : "none";
    return (
      <span className="compare-scale" data-tone={tone}>
        {/* Pips make the row comparable at a glance: three words of different
            lengths do not, and "S / M / L" reads as text you have to decode. */}
        {rank >= 0 ? (
          <span className="pips" aria-hidden="true">
            {axis.scale.map((step, i) => (
              <span key={step} className="pip" data-on={i <= rank} />
            ))}
          </span>
        ) : null}
        <span className="compare-scale-text">{text}</span>
      </span>
    );
  }

  return <span>{text}</span>;
}

/**
 * Deliberately a JS media query rather than CSS: the narrow layout is different
 * MARKUP, and rendering both and hiding one would put two radios with the same
 * name in the document.
 */
function useNarrow(query = "(max-width: 719px)"): boolean {
  const [narrow, setNarrow] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const media = window.matchMedia(query);
    const onChange = () => setNarrow(media.matches);
    onChange();
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [query]);
  return narrow;
}

function truncate(text: string, max = 60): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
}

function formatDefault(control: Control): string {
  const value = control.default;
  if (control.type === "multiselect") {
    if (!control.default.length) return "nothing selected";
    const labels = control.default.map((v) => control.options.find((o) => o.value === v)?.label ?? v);
    return labels.join(", ");
  }
  if (Array.isArray(value)) return `[${value.join(", ")}]`;
  if (control.type === "select" || control.type === "compare") {
    const match = control.options.find((o) => o.value === value);
    return match?.label ?? String(value);
  }
  return String(value);
}

function formatNumber(value: unknown): string {
  if (typeof value !== "number") return String(value ?? "");
  // Float maths leaks artefacts like 0.30000000000000004 into the readout.
  return Number.isInteger(value) ? String(value) : String(Math.round(value * 10000) / 10000);
}
