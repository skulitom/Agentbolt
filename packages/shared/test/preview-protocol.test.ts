import { describe, expect, it } from "vitest";
import { PreviewToParent, PREVIEW_IMPORTS, PREVIEW_MODULE_CONTRACT, type ParentToPreview } from "../src/preview-protocol.js";
import { AUTO_PREVIEW_MODULE } from "../src/auto-preview.js";

/**
 * Pins the provenance field on parent→preview `values` messages. The rule it
 * protects: the preview animates agent-sourced changes and tracks user gestures
 * 1:1 — animating under a live drag is the one forbidden failure. If `source`
 * grew a third value, the frame's user-wins coalescing would stop being
 * exhaustive.
 *
 * These are COMPILE-TIME assertions, checked by `pnpm typecheck` (the package
 * tsconfig includes test/), because `ParentToPreview` is a type rather than a
 * schema — nothing on the receiving end can validate it, so the type is the
 * only enforcement there is. Each `@ts-expect-error` fails the build if the
 * expression below it stops being an error.
 */
describe("ParentToPreview values.source", () => {
  it("accepts a values message with no source, and with either provenance", () => {
    const accepted: ParentToPreview[] = [
      // No source: degrades to "user", the safe non-laggy default.
      { type: "values", values: { a: 1 } },
      { type: "values", values: {}, source: "user" },
      { type: "values", values: {}, source: "agent" },
    ];
    expect(accepted).toHaveLength(3);
  });

  it("rejects anything else at compile time", () => {
    const rejected: ParentToPreview[] = [
      // @ts-expect-error — a third provenance would break the user-wins coalescing
      { type: "values", values: {}, source: "server" },
      // @ts-expect-error — no `dispose` message: the parent tears the frame down
      // by remounting the iframe, and the module's dispose() runs on pagehide
      { type: "dispose" },
    ];
    expect(rejected).toHaveLength(2);
  });
});

/**
 * The other direction IS a schema, and stays one: an agent wrote the code that
 * produces these messages.
 */
describe("PreviewToParent", () => {
  it("validates every message the frame actually sends", () => {
    for (const message of [
      { type: "ready-for-init" },
      { type: "ready" },
      { type: "preview_event", name: "picked", data: { i: 2 } },
      { type: "preview_error", message: "boom", stack: "at x" },
    ]) {
      expect(PreviewToParent.safeParse(message).success).toBe(true);
    }
  });

  it("covers ready-for-init, which the parent must not have to special-case", () => {
    // This one used to be handled ad hoc *before* the safeParse, so it was the
    // one frame message the parent trusted without validating.
    expect(PreviewToParent.safeParse({ type: "ready-for-init" }).success).toBe(true);
  });

  it("rejects a message shape the frame never sends", () => {
    expect(PreviewToParent.safeParse({ type: "resize", height: 400 }).success).toBe(false);
    expect(PreviewToParent.safeParse({ type: "preview_error" }).success).toBe(false);
  });
});

describe("the documented module contract", () => {
  it("teaches the two-arg update", () => {
    expect(PREVIEW_MODULE_CONTRACT).toContain("update(values, meta)");
    expect(PREVIEW_MODULE_CONTRACT).toContain('"user"');
    expect(PREVIEW_MODULE_CONTRACT).toContain('"agent"');
  });

  it("has the generated auto-preview module forward meta", () => {
    // Without this, every widget with a generated preview would silently lose
    // provenance and nothing built-in could ever animate.
    expect(AUTO_PREVIEW_MODULE).toContain("update(values, meta)");
    expect(AUTO_PREVIEW_MODULE).toContain("impl?.update(values, meta)");
  });

  it("lists the real import surface", () => {
    expect(PREVIEW_IMPORTS).toEqual(["three", "three/addons/", "agentui", "agentui/motion"]);
  });
});
