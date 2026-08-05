import os from "node:os";
import path from "node:path";

/**
 * Where each coding agent keeps its MCP server registry.
 *
 * Every path here was confirmed to exist on a real machine and its actual file
 * read before the writer was built, rather than being taken from documentation —
 * the shapes differ in ways docs gloss over (VS Code uses `servers`, not
 * `mcpServers`; Codex uses TOML).
 */
export type ConfigFormat = "mcpServers" | "vscode" | "toml";

export interface Target {
  id: string;
  label: string;
  /** Config file, resolved for this platform. */
  file: string;
  format: ConfigFormat;
  /** Extra note surfaced to the user (e.g. restart required). */
  note?: string;
}

const home = os.homedir();
const appData = process.env["APPDATA"] ?? path.join(home, "AppData", "Roaming");

function vsCodeUserDir(): string {
  if (process.platform === "win32") return path.join(appData, "Code", "User");
  if (process.platform === "darwin") return path.join(home, "Library", "Application Support", "Code", "User");
  return path.join(process.env["XDG_CONFIG_HOME"] ?? path.join(home, ".config"), "Code", "User");
}

function claudeDesktopDir(): string {
  if (process.platform === "win32") return path.join(appData, "Claude");
  if (process.platform === "darwin") return path.join(home, "Library", "Application Support", "Claude");
  return path.join(process.env["XDG_CONFIG_HOME"] ?? path.join(home, ".config"), "Claude");
}

export function allTargets(): Target[] {
  return [
    {
      id: "claude-code",
      label: "Claude Code",
      file: path.join(home, ".claude.json"),
      format: "mcpServers",
      note: "user scope — loads in every project",
    },
    { id: "cursor", label: "Cursor", file: path.join(home, ".cursor", "mcp.json"), format: "mcpServers" },
    {
      id: "claude-desktop",
      label: "Claude Desktop",
      file: path.join(claudeDesktopDir(), "claude_desktop_config.json"),
      format: "mcpServers",
      note: "restart the app to pick it up",
    },
    {
      id: "windsurf",
      label: "Windsurf",
      file: path.join(home, ".codeium", "windsurf", "mcp_config.json"),
      format: "mcpServers",
    },
    {
      id: "gemini-cli",
      label: "Gemini CLI",
      file: path.join(home, ".gemini", "settings.json"),
      format: "mcpServers",
      note: "merged into existing settings",
    },
    {
      id: "vscode",
      label: "VS Code (Copilot)",
      file: path.join(vsCodeUserDir(), "mcp.json"),
      format: "vscode",
      note: 'uses "servers", not "mcpServers"',
    },
    { id: "codex", label: "Codex CLI", file: path.join(home, ".codex", "config.toml"), format: "toml" },
  ];
}

export const SERVER_KEY = "agentui";
