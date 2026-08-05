import { useEffect, useState, type CSSProperties, type ReactElement } from "react";
import type { Control } from "@agentui/shared";
import type { InputProps } from "./index.js";

type CompareControl = Extract<Control, { type: "compare" }>;
type CompareAxis = CompareControl["axes"][number];
type CompareOption = CompareControl["options"][number];

/**
 * The comparison matrix: axes down, options across, one radio per column.
 *
 * Below ~720px the columns have nowhere to go, so the same data renders as one
 * card per option. Two DOM shapes rather than CSS gymnastics, because a matrix
 * that reflows into overlapping columns is worse than either layout.
 */
export function CompareInput({ control, value, disabled, set, commit }: InputProps<CompareControl>): ReactElement {
  const narrow = useNarrow();
  const name = `c-${control.id}`;
  const selected = String(value ?? control.default);

  const onPick = (next: string) => {
    set(next);
    // A discrete choice has no drag phase, so the change is the commit.
    queueMicrotask(commit);
  };

  const radio = (option: CompareOption) => (
    <input
      type="radio"
      name={name}
      value={option.value}
      checked={selected === option.value}
      disabled={disabled}
      onChange={() => onPick(option.value)}
    />
  );

  if (narrow) {
    return (
      <div className="compare-cards" role="radiogroup" aria-labelledby={`l-${control.id}`}>
        {control.options.map((option) => (
          <label key={option.value} className="compare-card" data-selected={selected === option.value}>
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
          <label key={option.value} className="compare-option" data-selected={selected === option.value}>
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
            <span key={option.value} className="compare-cell" data-selected={selected === option.value}>
              <Cell axis={axis} option={option} />
            </span>
          ))}
        </div>
      ))}
    </div>
  );
}

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
    const tone = axis.better === "none" ? "none" : yes === (axis.better === "high") ? "good" : "bad";
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
