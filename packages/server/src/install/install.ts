import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as TOML from "smol-toml";
import { SERVER_KEY, allTargets, type Target } from "./targets.js";

/**
 * Registers the MCP server with every coding agent found on this machine.
 *
 * Three rules, all learned from how these files actually behave:
 *
 *  1. **Merge, never replace.** These files hold the user's other servers and
 *     unrelated settings. Unknown keys are preserved untouched.
 *  2. **Absolute paths to `node` and to the entry script.** Not `npx`, not a
 *     bare command. The Windows `npx`/`cmd` wrapper problem is still open in
 *     shipped clients, and an absolute path sidesteps every variant of it.
 *  3. **Back up before writing**, because being wrong here breaks the user's
 *     other tools, not just ours.
 */

export interface PlanEntry {
  target: Target;
  exists: boolean;
  action: "install" | "update" | "uninstall" | "skip";
  reason?: string;
}

export interface ApplyResult extends PlanEntry {
  written: boolean;
  backup?: string;
  error?: string;
}

/** Absolute path to the built MCP entry point, resolved from this module. */
export function serverEntryPath(): string {
  const here = path.dirname(fileURLToPath(import.meta.url));
  for (const candidate of [
    path.resolve(here, "mcp.js"), // dist/cli.js sits beside dist/mcp.js
    path.resolve(here, "..", "dist", "mcp.js"),
    path.resolve(here, "..", "..", "dist", "mcp.js"),
  ]) {
    if (fs.existsSync(candidate)) return candidate;
  }
  throw new Error("Could not locate the built mcp.js — run `pnpm build` first.");
}

export function serverConfig(): { command: string; args: string[] } {
  return { command: process.execPath, args: [serverEntryPath()] };
}

function readTextIfPresent(file: string): string | undefined {
  try {
    return fs.readFileSync(file, "utf8");
  } catch {
    return undefined;
  }
}

/** Detects which agents are present, without writing anything. */
export function plan(mode: "install" | "uninstall", only?: string[]): PlanEntry[] {
  return allTargets().map((target) => {
    if (only?.length && !only.includes(target.id)) {
      return { target, exists: fs.existsSync(target.file), action: "skip", reason: "not selected" };
    }
    const exists = fs.existsSync(target.file);
    if (!exists) {
      // Absence of the config file is how we detect absence of the agent. We do
      // not create configs for tools that are not installed — that would litter
      // the user's home directory with settings for software they do not have.
      return { target, exists, action: "skip", reason: "not installed" };
    }
    const text = readTextIfPresent(target.file) ?? "";
    const registered = text.includes(`"${SERVER_KEY}"`) || text.includes(`[mcp_servers.${SERVER_KEY}]`);
    if (mode === "uninstall") {
      return { target, exists, action: registered ? "uninstall" : "skip", ...(registered ? {} : { reason: "not registered" }) };
    }
    return { target, exists, action: registered ? "update" : "install" };
  });
}

async function backup(file: string): Promise<string> {
  // Fixed suffix rather than a timestamp: repeated runs should not accumulate
  // one backup per invocation in the user's home directory.
  const dest = `${file}.agentui-backup`;

  // NEVER overwrite an existing backup. Re-serialising JSON can differ from the
  // user's original formatting, so a second run rewrites the file — and if that
  // refreshed the backup it would replace the pristine copy with an
  // already-modified one, destroying the only thing that makes this reversible.
  if (fs.existsSync(dest)) return dest;

  await fsp.copyFile(file, dest);
  return dest;
}

export function editJson(text: string, format: "mcpServers" | "vscode", entry: object | undefined): string {
  // An empty file is legitimate (VS Code ships mcp.json empty) and JSON.parse
  // would throw on it.
  const root = text.trim() ? (JSON.parse(text) as Record<string, unknown>) : {};
  const key = format === "vscode" ? "servers" : "mcpServers";
  const bucket = (root[key] as Record<string, unknown> | undefined) ?? {};

  if (entry) bucket[SERVER_KEY] = entry;
  else delete bucket[SERVER_KEY];

  if (Object.keys(bucket).length || entry) root[key] = bucket;
  return JSON.stringify(root, null, 2) + "\n";
}

export function editToml(text: string, entry: { command: string; args: string[] } | undefined): string {
  const root = (TOML.parse(text) as Record<string, unknown>) ?? {};
  const servers = (root["mcp_servers"] as Record<string, unknown> | undefined) ?? {};

  if (entry) servers[SERVER_KEY] = { command: entry.command, args: entry.args };
  else delete servers[SERVER_KEY];

  if (Object.keys(servers).length) root["mcp_servers"] = servers;
  else delete root["mcp_servers"];

  return TOML.stringify(root) + "\n";
}

export async function apply(entries: PlanEntry[], dryRun: boolean): Promise<ApplyResult[]> {
  const config = serverConfig();
  const results: ApplyResult[] = [];

  for (const entry of entries) {
    if (entry.action === "skip") {
      results.push({ ...entry, written: false });
      continue;
    }

    try {
      const original = readTextIfPresent(entry.target.file) ?? "";
      const removing = entry.action === "uninstall";
      const next =
        entry.target.format === "toml"
          ? editToml(original, removing ? undefined : config)
          : editJson(original, entry.target.format, removing ? undefined : config);

      if (dryRun || next === original) {
        results.push({ ...entry, written: false });
        continue;
      }

      const backupPath = await backup(entry.target.file);
      await fsp.writeFile(entry.target.file, next, "utf8");
      results.push({ ...entry, written: true, backup: backupPath });
    } catch (error) {
      results.push({ ...entry, written: false, error: String(error instanceof Error ? error.message : error) });
    }
  }

  return results;
}

/**
 * Installs the skill for Claude Code, which loads personal skills from
 * ~/.claude/skills in every project.
 *
 * Other agents get the same contract through the tool descriptions instead —
 * they are always in context, whereas a skill may not be, which is why the
 * descriptions were written to stand alone.
 */
export async function installSkill(dryRun: boolean): Promise<{ path: string; written: boolean; reason?: string }> {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const source = [
    path.resolve(here, "..", "skill", "SKILL.md"),
    path.resolve(here, "..", "..", "skill", "SKILL.md"),
    path.resolve(here, "..", "..", "..", "skills", "agentui", "SKILL.md"),
  ].find((candidate) => fs.existsSync(candidate));

  const dest = path.join(os.homedir(), ".claude", "skills", "agentui", "SKILL.md");
  if (!source) return { path: dest, written: false, reason: "SKILL.md not found in the package" };
  if (dryRun) return { path: dest, written: false };

  await fsp.mkdir(path.dirname(dest), { recursive: true });
  await fsp.copyFile(source, dest);
  return { path: dest, written: true };
}

export function renderPlan(results: ApplyResult[], dryRun: boolean): string {
  const width = Math.max(...results.map((r) => r.target.label.length));
  const lines = results.map((result) => {
    const mark =
      result.error ? "!" : result.action === "skip" ? " " : result.written ? "+" : dryRun ? "~" : "=";
    const status = result.error
      ? `FAILED — ${result.error}`
      : result.action === "skip"
        ? (result.reason ?? "skipped")
        : dryRun
          ? `would ${result.action}`
          : result.written
            ? `${result.action}ed`
            : "already up to date";
    const note = result.target.note && result.action !== "skip" ? `  (${result.target.note})` : "";
    return ` ${mark} ${result.target.label.padEnd(width)}  ${status}${note}\n     ${result.target.file}`;
  });
  return lines.join("\n");
}
