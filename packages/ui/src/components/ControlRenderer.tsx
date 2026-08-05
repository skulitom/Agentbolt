import { useEffect, useRef } from "react";
import { behaviourFor, type Control, type Values } from "@agentui/shared";
import { useSession } from "../store.js";
import { inputFor } from "./controls/index.js";
import { formatNumber } from "./controls/format.js";

interface Props {
  control: Control;
  disabled: boolean;
}

/**
 * The chrome around one control: label, value readout, "suggested" tag, reset.
 *
 * The input itself comes from CONTROL_INPUTS, which is a mapped Record over the
 * `Control` union — so a control type with no renderer is a compile error. This
 * file used to hold a 218-line `switch` with no declared return type, where a
 * missing arm produced a labelled card with an empty body and no error at all.
 *
 * The commit discipline lives in the input components: continuous inputs write
 * local state every frame (so the preview is live) but only commit on release,
 * because committing is what wakes the agent.
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
  const behaviour = behaviourFor(control);
  const Input = inputFor(control);

  // compare and multiselect are GROUPS of inputs, not one input: `for=` would
  // point at nothing. Asked of the registry rather than listed here, which is
  // one fewer place a new group-shaped control has to be remembered in.
  const isGroup = behaviour.groupLabelled === true;
  const isNumeric = control.type === "slider" || control.type === "number";

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
          {isNumeric ? (
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
              title={`Back to the suggested ${behaviour.formatDefault(control)}`}
              aria-label={`Reset ${control.label} to the suggested value`}
              onClick={() => resetControl(control.id)}
            >
              ↺
            </button>
          ) : null}
        </span>
      </div>

      {control.description ? <p className="control-description">{control.description}</p> : null}

      <div className="control-body">
        <Input control={control} value={value} disabled={disabled} set={set} commit={commit} />
      </div>
    </div>
  );
}
