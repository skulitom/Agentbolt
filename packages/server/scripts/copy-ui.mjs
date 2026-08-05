// Copies the built shell into packages/server/web so the published npm package
// is self-contained. At runtime the server prefers this copy and falls back to
// ../../ui/dist for local development — see src/core/paths.ts.
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

const here = path.dirname(fileURLToPath(import.meta.url));
const src = path.resolve(here, "..", "..", "ui", "dist");
const dest = path.resolve(here, "..", "web");

if (!fs.existsSync(src)) {
  console.error(`[copy-ui] ${src} does not exist — run the ui build first`);
  process.exit(1);
}

fs.rmSync(dest, { recursive: true, force: true });
fs.cpSync(src, dest, { recursive: true });

// The vendored three.js lives in ui/public and is emitted into ui/dist by Vite,
// so it comes along with the copy above. Fail loudly if that ever stops being true.
const vendor = path.join(dest, "vendor", "three", "three.module.js");
if (!fs.existsSync(vendor)) {
  console.error("[copy-ui] WARNING: vendored three.js missing from the build — 3D previews will fail");
}

const count = fs.readdirSync(dest, { recursive: true }).length;
console.error(`[copy-ui] ui/dist -> server/web (${count} entries)`);

// The skill ships with the package so `agentui install` can place it without
// needing the repo checked out.
const skillSrc = path.resolve(here, "..", "..", "..", "skills", "agentui", "SKILL.md");
const skillDest = path.resolve(here, "..", "skill", "SKILL.md");
if (fs.existsSync(skillSrc)) {
  fs.mkdirSync(path.dirname(skillDest), { recursive: true });
  fs.copyFileSync(skillSrc, skillDest);
  console.error("[copy-ui] SKILL.md -> server/skill");
} else {
  console.error("[copy-ui] WARNING: SKILL.md not found — agentui install will skip it");
}
