import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import { log } from "../log.js";

// NEVER resolve bundled assets from process.cwd(): agents launch this server
// with cwd set to *their* project directory, which is the whole point of the
// data-dir default below — but it means cwd tells us nothing about where our
// own files live. Everything shipped resolves from import.meta.url.
const moduleDir = path.dirname(fileURLToPath(import.meta.url));

/**
 * The built React shell. Published packages carry it at <pkg>/web; in the
 * monorepo it is still sitting in packages/ui/dist.
 */
export function webRoot(): string {
  const candidates = [
    path.resolve(moduleDir, "..", "web"), // dist/../web  (published + local build)
    path.resolve(moduleDir, "..", "..", "web"), // src/core/../../web (tsx dev)
    path.resolve(moduleDir, "..", "..", "..", "ui", "dist"), // monorepo dev
    path.resolve(moduleDir, "..", "..", "ui", "dist"),
  ];
  for (const dir of candidates) {
    if (fs.existsSync(path.join(dir, "index.html"))) return dir;
  }
  throw new Error(
    `AgentUI web assets not found. Looked in:\n  ${candidates.join("\n  ")}\nRun \`pnpm build\` first.`,
  );
}

/**
 * Directories we must never treat as "the project".
 *
 * Not every client launches us from the workspace. Claude Code does; Claude
 * Desktop launches from C:\WINDOWS\system32, and a launcher that inherits its
 * parent's cwd can land anywhere. Writing `.agent-ui` into system32 fails with
 * EPERM if you are lucky and litters a system directory if you are not, so the
 * cwd is treated as a hint to be validated rather than a fact.
 */
export function isPlausibleProjectDir(dir: string): boolean {
  const resolved = path.resolve(dir);
  const lower = resolved.toLowerCase();

  // Filesystem root, or a drive root like C:\ — nobody's project.
  if (path.dirname(resolved) === resolved) return false;

  const forbidden = [
    process.env["SystemRoot"]?.toLowerCase(),
    process.env["ProgramFiles"]?.toLowerCase(),
    process.env["ProgramFiles(x86)"]?.toLowerCase(),
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/var",
    "/system",
    "/library",
  ].filter((entry): entry is string => Boolean(entry));

  if (forbidden.some((bad) => lower === bad || lower.startsWith(`${bad}${path.sep}`) || lower.startsWith(`${bad}/`))) {
    return false;
  }

  // Must actually be writable — a read-only checkout is not a place for state.
  try {
    fs.accessSync(resolved, fs.constants.W_OK);
  } catch {
    return false;
  }
  return true;
}

/** Where session state lives, resolved once and remembered. */
let resolvedDataDir: string | undefined;

/**
 * Per-project session storage.
 *
 * Prefers `<cwd>/.agent-ui`, because when the client does launch us from the
 * workspace that gives every project its own sessions with zero configuration.
 * When the cwd is clearly not a workspace, falls back to a per-instance folder
 * under the user's home rather than failing or writing somewhere it should not.
 */
export function dataDir(): string {
  const override = process.env["AGENT_UI_DATA_DIR"];
  if (override) return path.resolve(override);
  if (resolvedDataDir) return resolvedDataDir;

  const cwd = process.cwd();
  if (isPlausibleProjectDir(cwd)) {
    resolvedDataDir = path.resolve(cwd, ".agent-ui");
  } else {
    log.warn("cwd is not a usable project directory; keeping session data in your home folder instead", {
      cwd,
      using: path.join(os.homedir(), ".agentui", "sessions"),
    });
    resolvedDataDir = path.join(os.homedir(), ".agentui", "sessions");
  }
  return resolvedDataDir;
}

/**
 * Lets the MCP layer supply the workspace once the client tells us what it is
 * (via `roots`), which is better information than the cwd ever was.
 */
export function adoptProjectRoot(root: string): boolean {
  if (process.env["AGENT_UI_DATA_DIR"]) return false;
  if (!isPlausibleProjectDir(root)) return false;
  const next = path.resolve(root, ".agent-ui");
  if (resolvedDataDir === next) return false;
  resolvedDataDir = next;
  projectNameOverride = path.basename(path.resolve(root));
  log.info("adopted workspace root reported by the client", { root, dataDir: next });
  return true;
}

export function sessionsDir(): string {
  return path.join(dataDir(), "sessions");
}

/** Widget library: global by default, with a per-project overlay. */
export function userLibraryDir(): string {
  const override = process.env["AGENT_UI_LIBRARY_DIR"];
  return override ? path.resolve(override) : path.join(os.homedir(), ".agentui", "library");
}

export function projectLibraryDir(): string {
  return path.join(dataDir(), "library");
}

let projectNameOverride: string | undefined;

/** A human-readable name for this project, used in the dashboard and library metadata. */
export function projectName(): string {
  if (projectNameOverride) return projectNameOverride;
  const dir = dataDir();
  // When the data dir fell back to the home folder there is no project to name.
  if (dir === path.join(os.homedir(), ".agentui", "sessions")) return "no workspace";
  return path.basename(path.dirname(path.resolve(dir)));
}
