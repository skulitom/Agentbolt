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

  const set = (next: Values[string]) => setLocalValue(control.id, next);

  return (
    <div className="control" data-type={control.type} data-touched={touched}>
      <div className="control-head">
        <label className="control-label" htmlFor={`c-${control.id}`}>
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
    }
  }
}

function truncate(text: string, max = 60): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
}

function formatDefault(control: Control): string {
  const value = control.default;
  if (Array.isArray(value)) return `[${value.join(", ")}]`;
  if (control.type === "select") {
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
