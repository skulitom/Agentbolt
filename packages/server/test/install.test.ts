import { describe, expect, it } from "vitest";
import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { apply, editJson, editToml } from "../src/install/install.js";
import { allTargets } from "../src/install/targets.js";

const ENTRY = { command: "C:\\Program Files\\nodejs\\node.exe", args: ["C:\\DEV\\AgentUI\\packages\\server\\dist\\mcp.js"] };

/**
 * Being wrong here breaks the user's OTHER tools, not just ours — so the writers
 * are tested against copies of the real config files on this machine wherever
 * they exist, not only against invented fixtures.
 */
describe("config writers", () => {
  it("adds our server without disturbing existing ones", () => {
    const before = JSON.stringify({
      mcpServers: {
        Blender: { command: "uvx", args: ["blender-mcp"] },
        filesystem: { command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "C:\\DEV"] },
      },
    });
    const after = JSON.parse(editJson(before, "mcpServers", ENTRY));
    expect(Object.keys(after.mcpServers).sort()).toEqual(["Blender", "agentui", "filesystem"]);
    expect(after.mcpServers.Blender).toEqual({ command: "uvx", args: ["blender-mcp"] });
  });

  it("preserves unrelated top-level settings", () => {
    const before = JSON.stringify({
      selectedAuthType: "oauth-personal",
      security: { auth: { selectedType: "oauth-personal" } },
    });
    const after = JSON.parse(editJson(before, "mcpServers", ENTRY));
    expect(after.selectedAuthType).toBe("oauth-personal");
    expect(after.security.auth.selectedType).toBe("oauth-personal");
    expect(after.mcpServers.agentui).toEqual(ENTRY);
  });

  it("handles an empty file, which VS Code genuinely ships", () => {
    const after = JSON.parse(editJson("", "vscode", ENTRY));
    expect(after.servers.agentui).toEqual(ENTRY);
  });

  it('uses "servers" for VS Code and "mcpServers" elsewhere', () => {
    expect(JSON.parse(editJson("", "vscode", ENTRY))).toHaveProperty("servers.agentui");
    expect(JSON.parse(editJson("", "mcpServers", ENTRY))).toHaveProperty("mcpServers.agentui");
  });

  it("is idempotent — installing twice changes nothing the second time", () => {
    const once = editJson("{}", "mcpServers", ENTRY);
    expect(editJson(once, "mcpServers", ENTRY)).toBe(once);
  });

  it("removes only our entry on uninstall", () => {
    const withOurs = editJson(JSON.stringify({ mcpServers: { other: { command: "x" } } }), "mcpServers", ENTRY);
    const after = JSON.parse(editJson(withOurs, "mcpServers", undefined));
    expect(after.mcpServers.agentui).toBeUndefined();
    expect(after.mcpServers.other).toEqual({ command: "x" });
  });

  it("round-trips Codex TOML, preserving other servers and their env blocks", () => {
    const before = [
      "[mcp_servers.node_repl]",
      'command = "C:\\\\tools\\\\node_repl.exe"',
      "args = []",
      "startup_timeout_sec = 120",
      "",
      "[mcp_servers.node_repl.env]",
      'NODE_REPL_NATIVE_PIPE_CONNECT_TIMEOUT_MS = "1000"',
      "",
    ].join("\n");

    const after = editToml(before, ENTRY);
    expect(after).toContain("[mcp_servers.agentui]");
    expect(after).toContain("node_repl");
    expect(after).toContain("startup_timeout_sec");
    expect(after).toContain("NODE_REPL_NATIVE_PIPE_CONNECT_TIMEOUT_MS");

    const removed = editToml(after, undefined);
    expect(removed).not.toContain("agentui");
    expect(removed).toContain("node_repl");
  });

  it("does not leave an empty mcp_servers table behind", () => {
    const only = editToml("", ENTRY);
    expect(editToml(only, undefined).trim()).toBe("");
  });

  it("never overwrites an existing backup", async () => {
    const dir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-backup-"));
    const file = path.join(dir, "mcp.json");
    const pristine = JSON.stringify({ mcpServers: { other: { command: "x" } } }, null, 2) + "\n";
    await fsp.writeFile(file, pristine, "utf8");

    const target = { id: "t", label: "T", file, format: "mcpServers" as const };
    // Two installs. Re-serialisation can change formatting, so the second run
    // may rewrite the file — the backup must still hold the ORIGINAL, or the
    // change stops being reversible.
    await apply([{ target, exists: true, action: "install" }], false);
    await apply([{ target, exists: true, action: "update" }], false);

    const backupText = await fsp.readFile(`${file}.agentui-backup`, "utf8");
    expect(backupText).toBe(pristine);
    expect(backupText).not.toContain("agentui");

    await fsp.rm(dir, { recursive: true, force: true });
  });

  it("is a no-op on a second identical run", async () => {
    const dir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-idem-"));
    const file = path.join(dir, "mcp.json");
    await fsp.writeFile(file, "{}", "utf8");
    const target = { id: "t", label: "T", file, format: "mcpServers" as const };

    const first = await apply([{ target, exists: true, action: "install" }], false);
    const second = await apply([{ target, exists: true, action: "update" }], false);

    expect(first[0]?.written).toBe(true);
    expect(second[0]?.written).toBe(false);

    await fsp.rm(dir, { recursive: true, force: true });
  });

  /**
   * The real files on this machine. Skipped rather than failed where an agent
   * is not installed, so the suite stays meaningful on any machine.
   */
  for (const target of allTargets()) {
    const exists = fs.existsSync(target.file);
    it.skipIf(!exists)(`merges into the real ${target.label} config without losing anything`, () => {
      const original = fs.readFileSync(target.file, "utf8");

      if (target.format === "toml") {
        const after = editToml(original, ENTRY);
        expect(after).toContain("[mcp_servers.agentui]");
        // Every previously-declared server survives.
        for (const name of original.matchAll(/\[mcp_servers\.([a-z0-9_]+)\]/gi)) {
          expect(after).toContain(`[mcp_servers.${name[1]}]`);
        }
        return;
      }

      const before = original.trim() ? JSON.parse(original) : {};
      const after = JSON.parse(editJson(original, target.format === "vscode" ? "vscode" : "mcpServers", ENTRY));
      const key = target.format === "vscode" ? "servers" : "mcpServers";

      expect(after[key].agentui).toEqual(ENTRY);
      for (const existing of Object.keys(before[key] ?? {})) {
        expect(after[key][existing]).toEqual(before[key][existing]);
      }
      // Nothing unrelated is dropped.
      for (const topLevel of Object.keys(before)) {
        expect(after).toHaveProperty(topLevel);
      }
    });
  }
});
