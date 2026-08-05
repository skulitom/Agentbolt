import { describe, expect, it } from "vitest";
import os from "node:os";
import path from "node:path";
import { isPlausibleProjectDir } from "../src/core/paths.js";

/**
 * Where session state goes when the client does not launch us from a workspace.
 *
 * This is not hypothetical: Claude Code launches the server from the project,
 * Claude Desktop launches it from C:\WINDOWS\system32, and the first real use
 * through Desktop failed with `EPERM: mkdir C:\WINDOWS\system32\.agent-ui`. The
 * cwd is a hint, not a fact.
 */
describe("project directory detection", () => {
  it("accepts an ordinary writable working directory", () => {
    expect(isPlausibleProjectDir(process.cwd())).toBe(true);
    expect(isPlausibleProjectDir(os.tmpdir())).toBe(true);
  });

  it("rejects the Windows system directory", () => {
    const systemRoot = process.env["SystemRoot"];
    if (!systemRoot) return; // not Windows
    expect(isPlausibleProjectDir(systemRoot)).toBe(false);
    expect(isPlausibleProjectDir(path.join(systemRoot, "system32"))).toBe(false);
  });

  it("rejects Program Files", () => {
    const programFiles = process.env["ProgramFiles"];
    if (!programFiles) return;
    expect(isPlausibleProjectDir(programFiles)).toBe(false);
  });

  it("rejects a filesystem or drive root", () => {
    expect(isPlausibleProjectDir(path.parse(process.cwd()).root)).toBe(false);
  });

  it("rejects a directory that does not exist or cannot be written", () => {
    expect(isPlausibleProjectDir(path.join(os.tmpdir(), "definitely-not-here-agentui"))).toBe(false);
  });

  it("rejects unix system directories by prefix, not by exact match", () => {
    for (const dir of ["/usr", "/usr/local/bin", "/etc", "/var/log"]) {
      expect(isPlausibleProjectDir(dir)).toBe(false);
    }
  });
});
