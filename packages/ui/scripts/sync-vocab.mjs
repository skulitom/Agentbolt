/**
 * Copies the shared browser modules to the preview origin's asset directory.
 *
 * The preview frame runs on a second origin with no bundler, so it can only
 * import files that physically sit next to it under `public/agentui/`. Shared
 * cannot be imported from there and `public/` cannot reach into `../shared`,
 * so a copy is the only mechanism available.
 *
 * The copies are CHECKED IN rather than generated-and-ignored: `pnpm dev:ui`
 * serves `public/` straight off disk with no build step, so an ignored copy
 * would mean a fresh clone has a broken preview until someone runs a build.
 * `vocab-sync.test.ts` asserts each pair is byte-identical, and this script
 * runs as `prebuild`/`predev` so a copy cannot go stale unnoticed.
 */
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";

/** Every plain-JS module both origins share. Add here, not in two places. */
export const SHARED_BROWSER_MODULES = ["vocab.js", "preview-plan.js"];

let changed = 0;
for (const name of SHARED_BROWSER_MODULES) {
  const source = fileURLToPath(new URL(`../../shared/src/${name}`, import.meta.url));
  const destination = fileURLToPath(new URL(`../public/agentui/${name}`, import.meta.url));
  const contents = readFileSync(source);

  // Byte comparison, not a timestamp check: the point is that the copy matches,
  // and rewriting an identical file would churn mtimes for every watcher.
  if (existsSync(destination) && readFileSync(destination).equals(contents)) continue;
  writeFileSync(destination, contents);
  console.log(`[sync-vocab] shared/src/${name} -> ui/public/agentui/${name}`);
  changed++;
}

if (!changed) console.log("[sync-vocab] up to date");
