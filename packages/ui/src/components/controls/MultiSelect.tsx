import type { ReactElement } from "react";
import type { Control } from "@agentui/shared";
import type { InputProps } from "./index.js";
import { formatNumber } from "./format.js";

/**
 * A subset of a known set, with a live total.
 *
 * The bar is what makes "this one is most of the cost" visible without adding
 * six numbers up in your head. With no weights this is a form, not a chart —
 * and it says so, rather than dressing a count up as a measurement.
 */
export function MultiSelectInput({
  control,
  value,
  disabled,
  set,
  commit,
}: InputProps<Extract<Control, { type: "multiselect" }>>): ReactElement {
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
              <span className="check-bar" aria-hidden="true">
                <span style={{ width: heaviest > 0 ? `${(option.weight / heaviest) * 100}%` : "0%" }} />
              </span>
              {formatNumber(option.weight)}
              {control.weightUnit ? <span className="unit"> {control.weightUnit}</span> : null}
            </span>
          ) : null}
        </label>
      ))}
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
