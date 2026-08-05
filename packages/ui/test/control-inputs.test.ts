import { describe, expect, it } from "vitest";
import { CONTROL_TYPES } from "@agentui/shared";
import { CONTROL_INPUTS } from "../src/components/controls/index.js";

/**
 * The renderer half of the exhaustiveness contract.
 *
 * `CONTROL_INPUTS` is a mapped Record over the `Control` union whose values
 * return `ReactElement` rather than `ReactElement | null`, so the real
 * enforcement is in `pnpm typecheck`: a control type with no renderer, or a
 * renderer that can return nothing, will not compile.
 *
 * What that cannot check is agreement with the SHARED registry — the two live
 * in different packages and are keyed independently. If they diverge, a control
 * validates fine and then draws nothing, which is exactly the failure the
 * original 218-line switch (no declared return type) produced silently.
 */
describe("every control type can be rendered", () => {
  it("has an input component for every type the shared registry declares", () => {
    expect(Object.keys(CONTROL_INPUTS).sort()).toEqual([...CONTROL_TYPES].sort());
  });

  it("maps every type to something callable", () => {
    for (const type of CONTROL_TYPES) {
      expect(CONTROL_INPUTS[type], type).toBeTypeOf("function");
    }
  });
});
