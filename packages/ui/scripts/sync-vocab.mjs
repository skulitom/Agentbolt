/**
 * Copies the shared vocabulary to the preview origin's asset directory.
 *
 * The preview frame runs on a second origin with no bundler, so it can only
 * import files that physically sit next to it under `public/agentui/`. Shared
 * cannot be imported from there and `public/` cannot reach into `../shared`,
 * so a copy is the only mechanism available.
 *
 * The copy is CHECKED IN rather than generated-and-ignored: `pnpm dev:ui`
 * serves `public/` straight off disk with no build step, so an ignored copy
 * would mean a fresh clone has a broken preview until someone runs a build.
 * `vocab-sync.test.ts` asserts the two files are byte-identical, and this
 * script runs as `prebuild`/`predev` so the copy cannot go stale unnoticed.
 */
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";

const source = fileURLToPath(new URL("../../shared/src/vocab.js", import.meta.url));
const destination = fileURLToPath(new URL("../public/agentui/vocab.js", import.meta.url));

const contents = readFileSync(source);

// Byte comparison, not a timestamp check: the point is that the copy matches,
// and rewriting an identical file would churn mtimes for every watcher.
if (existsSync(destination) && readFileSync(destination).equals(contents)) {
  console.log("[sync-vocab] up to date");
} else {
  writeFileSync(destination, contents);
  console.log("[sync-vocab] shared/src/vocab.js -> ui/public/agentui/vocab.js");
}
