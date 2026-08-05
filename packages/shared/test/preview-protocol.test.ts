import { describe, expect, it } from "vitest";
import { ParentToPreview, PREVIEW_IMPORTS, PREVIEW_MODULE_CONTRACT } from "../src/preview-protocol.js";
import { AUTO_PREVIEW_MODULE } from "../src/auto-preview.js";

/**
 * Pins the provenance field on parent→preview `values` messages. The rule it
 * protects: the preview animates agent-sourced changes and tracks user
 * gestures 1:1 — animating under a live drag is the one forbidden failure. If
 * `source` ever became required, an older shell would fail validation; if it
 * grew values beyond user/agent, the frame's user-wins coalescing would stop
 * being exhaustive.
 */
describe("ParentToPreview values.source", () => {
  it("accepts a values message without source — an older parent still validates", () => {
    const parsed = ParentToPreview.safeParse({ type: "values", values: { a: 1 } });
    expect(parsed.success).toBe(true);
  });

  it("accepts exactly user and agent", () => {
    for (const source of ["user", "agent"] as const) {
      expect(ParentToPreview.safeParse({ type: "values", values: {}, source }).success).toBe(true);
    }
  });

  it("rejects any other source", () => {
    expect(ParentToPreview.safeParse({ type: "values", values: {}, source: "server" }).success).toBe(false);
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
