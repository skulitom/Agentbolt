import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = path.resolve(here, "..", "src", "vocab.js");
const copy = path.resolve(here, "..", "..", "ui", "public", "agentui", "vocab.js");

/**
 * The preview frame is a second origin with no bundler, so it cannot import
 * from `shared/`. The vocabulary both halves share therefore exists as a file
 * that is COPIED next to the frame's other assets, and the copy is checked in
 * so `pnpm dev:ui` works from a fresh clone with no build step.
 *
 * A byte comparison, not a regex over the source. The test this replaces
 * scraped `preview.js` for `/^const SIZE_HINTS = \/(.+)\/i;$/` — it would have
 * broken on a reformat that changed no behaviour, and it could not see the
 * drift that actually mattered: `isEasingSelect` was implemented twice with
 * nothing comparing the two copies at all.
 */
describe("the shared vocabulary reaches the preview origin intact", () => {
  it("is byte-identical on both origins", () => {
    const a = readFileSync(source);
    const b = readFileSync(copy);
    expect(
      b.equals(a),
      "ui/public/agentui/vocab.js is stale — run `node packages/ui/scripts/sync-vocab.mjs`",
    ).toBe(true);
  });

  it("stays importable by a browser with no bundler: no imports, no TypeScript", () => {
    const text = readFileSync(source, "utf8");
    // A bare specifier or a `../` would resolve in vitest and 404 on the
    // preview origin — green here, blank preview pane in the browser, and the
    // DevTools error blames CSP rather than the import.
    expect(text).not.toMatch(/^\s*import\s/m);
    expect(text).not.toMatch(/\bexport\s+(type|interface)\b/);
  });
});
