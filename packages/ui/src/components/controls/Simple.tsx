import type { ReactElement } from "react";
import type { Control } from "@agentui/shared";
import type { InputProps } from "./index.js";
import { truncate } from "./format.js";

/**
 * The six single-input control types.
 *
 * The commit discipline is the important part and it splits them in two.
 * CONTINUOUS inputs (slider, number, colour, text) write local state on every
 * event so the preview is live, but only *commit* on pointer release or blur —
 * committing is what wakes the agent, so committing per frame would make the
 * async model unusable. DISCRETE inputs (select, toggle) have no drag phase, so
 * the change IS the commit; `queueMicrotask` lets the local write land first.
 */
const commitAfterChange = (commit: () => void) => queueMicrotask(commit);

export function SliderInput({ control, value, disabled, set, commit }: InputProps<Extract<Control, { type: "slider" }>>): ReactElement {
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
}

export function NumberInput({ control, value, disabled, set, commit }: InputProps<Extract<Control, { type: "number" }>>): ReactElement {
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
}

export function SelectInput({ control, value, disabled, set, commit }: InputProps<Extract<Control, { type: "select" }>>): ReactElement {
  return (
    <select
      id={`c-${control.id}`}
      value={String(value ?? control.default)}
      disabled={disabled}
      onChange={(e) => {
        set(e.target.value);
        commitAfterChange(commit);
      }}
    >
      {control.options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label ?? option.value}
        </option>
      ))}
    </select>
  );
}

export function ToggleInput({ control, value, disabled, set, commit }: InputProps<Extract<Control, { type: "toggle" }>>): ReactElement {
  return (
    <label className="switch">
      <input
        id={`c-${control.id}`}
        type="checkbox"
        checked={Boolean(value ?? control.default)}
        disabled={disabled}
        onChange={(e) => {
          set(e.target.checked);
          commitAfterChange(commit);
        }}
      />
      <span>{value ? "on" : "off"}</span>
    </label>
  );
}

export function ColorInput({ control, value, disabled, set, commit }: InputProps<Extract<Control, { type: "color" }>>): ReactElement {
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
}

export function TextInput({ control, value, disabled, set, commit }: InputProps<Extract<Control, { type: "text" }>>): ReactElement {
  // A multiline box is the biggest thing on a page and reads as homework.
  // Collapse it: open only when it already has content, or on request.
  if (control.multiline) {
    return (
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
    );
  }
  return (
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
}
