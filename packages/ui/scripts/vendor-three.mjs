// Copies a pinned three.js ESM build out of node_modules into public/vendor/three
// so agent-authored preview modules can `import * as THREE from "three"` offline,
// with no CDN and no network at runtime.
//
// Modern three splits the implementation into build/three.core.js, which
// three.module.js imports relatively — so the core chunk must be copied too or
// the import silently 404s inside the sandboxed iframe.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(here, "..", "public", "vendor", "three");

// three's "exports" map does not expose ./package.json, so resolving the
// manifest directly fails. Resolve a real entry point and walk up to the
// package root instead.
function findThreeRoot() {
  try {
    return path.dirname(require.resolve("three/package.json"));
  } catch {
    /* exports map hides it — fall through */
  }
  try {
    let dir = path.dirname(require.resolve("three"));
    for (let i = 0; i < 6; i++) {
      if (fs.existsSync(path.join(dir, "package.json"))) {
        const name = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8")).name;
        if (name === "three") return dir;
      }
      dir = path.dirname(dir);
    }
  } catch {
    /* not installed at all */
  }
  return null;
}

const threeRoot = findThreeRoot();
if (!threeRoot) {
  console.error("[vendor-three] three is not installed; skipping (preview 3D will be unavailable)");
  process.exit(0);
}

const pkg = JSON.parse(fs.readFileSync(path.join(threeRoot, "package.json"), "utf8"));

/** Everything in build/ that is an ESM chunk, so relative imports between them resolve. */
function buildFiles() {
  const buildDir = path.join(threeRoot, "build");
  return fs
    .readdirSync(buildDir)
    .filter((f) => f.endsWith(".js") || f.endsWith(".js.map"))
    .filter((f) => !f.includes(".cjs") && !f.includes(".min."))
    .map((f) => ({ from: path.join(buildDir, f), to: path.join(outDir, f) }));
}

/** Addons are pulled in wholesale — they cross-import each other freely. */
function addonFiles() {
  const src = path.join(threeRoot, "examples", "jsm");
  if (!fs.existsSync(src)) return [];
  const out = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".js")) {
        out.push({ from: full, to: path.join(outDir, "addons", path.relative(src, full)) });
      }
    }
  };
  walk(src);
  return out;
}

fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });

const files = [...buildFiles(), ...addonFiles()];
for (const { from, to } of files) {
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.copyFileSync(from, to);
}

fs.writeFileSync(
  path.join(outDir, "VERSION.json"),
  JSON.stringify({ package: "three", version: pkg.version, files: files.length }, null, 2) + "\n",
);

console.error(`[vendor-three] three@${pkg.version} -> public/vendor/three (${files.length} files)`);
