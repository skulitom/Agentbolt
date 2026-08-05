import type { ReactElement } from "react";
import type { Control, Values } from "@agentui/shared";
import { ColorInput, NumberInput, SelectInput, SliderInput, TextInput, ToggleInput } from "./Simple.js";
import { VectorInput } from "./Vector.js";
import { CompareInput } from "./Compare.js";
import { MultiSelectInput } from "./MultiSelect.js";

/**
 * One input component per control type, keyed by the same union the shared
 * registry uses.
 *
 * The shell cannot put JSX in `@agentui/shared`, so the exhaustiveness check has
 * to be made twice — once there for behaviour, once here for rendering. A
 * mapped Record does it: `InputRegistry` cannot be built without an entry for
 * every member of `Control`, and the compiler names the missing type.
 *
 * The return type is `ReactElement`, NOT `ReactElement | null`, and that is the
 * whole point. This used to be a 218-line `switch` inside the card component
 * with no declared return type at all, so a control type nobody wrote an arm
 * for fell through, returned undefined, and rendered NOTHING — a labelled card
 * with an empty body, and no error anywhere.
 */
export interface InputProps<C extends Control> {
  control: C;
  value: Values[string] | undefined;
  disabled: boolean;
  /** Local-only write. Fires every frame during a drag; never wakes the agent. */
  set(next: Values[string]): void;
  /** Commits. This is what wakes the agent, so it fires on release, not on move. */
  commit(): void;
}

type InputRegistry = {
  [K in Control["type"]]: (props: InputProps<Extract<Control, { type: K }>>) => ReactElement;
};

export const CONTROL_INPUTS: InputRegistry = {
  slider: SliderInput,
  number: NumberInput,
  select: SelectInput,
  toggle: ToggleInput,
  color: ColorInput,
  text: TextInput,
  vector: VectorInput,
  compare: CompareInput,
  multiselect: MultiSelectInput,
};

/**
 * The one cast in the dispatch. TypeScript cannot correlate a union key with a
 * union of functions that each take a different narrowed argument, so the
 * lookup has to be widened at the call site — but the REGISTRY above is fully
 * checked, which is where a mistake would actually be made.
 */
export function inputFor(control: Control): (props: InputProps<Control>) => ReactElement {
  return CONTROL_INPUTS[control.type] as (props: InputProps<Control>) => ReactElement;
}
