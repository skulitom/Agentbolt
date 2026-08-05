import type { ReactElement } from "react";
import type { Control } from "@agentui/shared";
import type { InputProps } from "./index.js";

/** Two or three numbers that belong together, labelled x/y/z unless told otherwise. */
export function VectorInput({
  control,
  value,
  disabled,
  set,
  commit,
}: InputProps<Extract<Control, { type: "vector" }>>): ReactElement {
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
