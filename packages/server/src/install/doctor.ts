import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { allTargets, type Target } from "./targets.js";
import { serverConfig, serverEntryPath } from "./install.js";

/**
 * Verifies the install by launching the server the way each agent actually
 * launches it — same command, same args, and crucially the same working
 * directory.
 *
 * This exists because of a real failure: Claude Desktop starts MCP servers from
 * C:\WINDOWS\system32, the data directory was derived from the cwd, and the very
 * first real call died with EPERM. Every test and demo up to that point had set
 * cwd explicitly, so nothing caught it. A check that does not reproduce the
 * client's launch conditions is not a check.
 */

export interface Diagnosis {
  target: Target | undefined;
  label: string;
  ok: boolean;
  detail: string;
  cwd: string;
}

/** cwds worth testing: what each detected client will actually use. */
function launchCwdsToTest(): Array<{ label: string; cwd: string; target?: Target }> {
  const cases: Array<{ label: string; cwd: string; target?: Target }> = [
    { label: "a project directory", cwd: process.cwd() },
  ];

  // Desktop-style launchers inherit the OS default rather than a workspace.
  const systemRoot = process.env["SystemRoot"];
  if (systemRoot) {
    cases.push({ label: "a desktop app's default cwd", cwd: path.join(systemRoot, "system32") });
  } else {
    cases.push({ label: "a desktop app's default cwd", cwd: "/" });
  }

  return cases;
}

interface Probe {
  ok: boolean;
  detail: string;
}

/**
 * Speaks just enough MCP over stdio to prove the server initializes and
 * advertises its tools. Deliberately hand-rolled rather than using the SDK
 * client: this must exercise the shipped binary exactly as a client would,
 * without our own code in the loop.
 */
function probeServer(cwd: string, timeoutMs = 15_000): Promise<Probe> {
  return new Promise((resolve) => {
    const { command, args } = serverConfig();
    const child = spawn(command, args, {
      cwd,
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, AGENT_UI_NO_OPEN: "1", AGENT_UI_LOG_LEVEL: "error" },
    });

    let stdout = "";
    let stderr = "";
    let settled = false;
    let toolCount = 0;

    const finish = (result: Probe) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.kill();
      resolve(result);
    };

    const timer = setTimeout(
      () => finish({ ok: false, detail: `no response within ${timeoutMs / 1000}s${stderr ? ` — ${firstError(stderr)}` : ""}` }),
      timeoutMs,
    );

    child.on("error", (error) => finish({ ok: false, detail: `could not start: ${error.message}` }));
    // A child that exits while we are mid-handshake makes the next write EPIPE,
    // which is an unhandled 'error' event on the socket and takes the CLI down
    // with it. Swallow it: the probe result is decided by the timeout instead.
    child.stdin.on("error", () => {});

    const send = (message: unknown): void => {
      if (settled || child.stdin.destroyed || !child.stdin.writable) return;
      child.stdin.write(`${JSON.stringify(message)}\n`);
    };

    child.on("exit", (code) =>
      finish({ ok: false, detail: `exited early (code ${code ?? "?"})${stderr ? ` — ${firstError(stderr)}` : ""}` }),
    );
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
    });

    child.stdout.on("data", (chunk) => {
      stdout += String(chunk);
      for (const line of stdout.split("\n")) {
        if (!line.trim().startsWith("{")) continue;
        let message: { id?: number; result?: { tools?: unknown[]; serverInfo?: { name?: string } }; error?: { message?: string } };
        try {
          message = JSON.parse(line);
        } catch {
          continue;
        }
        if (message.error) return finish({ ok: false, detail: message.error.message ?? "protocol error" });

        if (message.id === 1 && message.result?.serverInfo) {
          send({ jsonrpc: "2.0", method: "notifications/initialized" });
          send({ jsonrpc: "2.0", id: 2, method: "tools/list" });
        }
        if (message.id === 2 && message.result?.tools) {
          toolCount = message.result.tools.length;
          // Listing tools is NOT enough. The cwd bug this check exists for
          // passed initialize and tools/list happily and only died when a
          // session was created — a doctor that stops here would have given a
          // clean bill of health to a completely broken install.
          send({
            jsonrpc: "2.0",
            id: 3,
            method: "tools/call",
            params: {
              name: "ui_create",
              arguments: {
                spec: { title: "agentui doctor", controls: [{ id: "ok", label: "OK", type: "toggle", default: true }] },
                open: false,
              },
            },
          });
          continue;
        }

        if (message.id === 3) {
          const call = message.result as { isError?: boolean; content?: Array<{ text?: string }> } | undefined;
          if (!call || call.isError) {
            return finish({
              ok: false,
              detail: `could not create a session: ${call?.content?.[0]?.text?.split("\n")[0] ?? "unknown error"}`,
            });
          }
          // Clean up after ourselves — a health check should leave no widgets.
          const created = (message.result as { structuredContent?: { sessionId?: string } }).structuredContent;
          if (created?.sessionId) {
            send({
              jsonrpc: "2.0",
              id: 4,
              method: "tools/call",
              params: { name: "ui_close", arguments: { sessionId: created.sessionId, deleteFiles: true } },
            });
          }
          // The server warns when it cannot use the cwd; surface it, because
          // "works" and "works where you expected" are different claims.
          const fallback = /not a usable project directory/.test(stderr);
          return finish({
            ok: true,
            detail: `${toolCount} tools, session created${fallback ? "; data kept in your home folder (cwd unusable)" : ""}`,
          });
        }
      }
    });

    send({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2025-06-18",
        capabilities: {},
        clientInfo: { name: "agentui-doctor", version: "0.1.0" },
      },
    });
  });
}

function firstError(stderr: string): string {
  const line = stderr.split("\n").find((l) => /error|EPERM|EACCES|ENOENT/i.test(l));
  return (line ?? stderr.split("\n")[0] ?? "").trim().slice(0, 160);
}

export async function runDoctor(): Promise<Diagnosis[]> {
  const results: Diagnosis[] = [];

  // 1. Is the thing we would register actually there?
  try {
    const entry = serverEntryPath();
    results.push({
      target: undefined,
      label: "built server",
      ok: true,
      detail: entry,
      cwd: "",
    });
  } catch (error) {
    results.push({
      target: undefined,
      label: "built server",
      ok: false,
      detail: `${error instanceof Error ? error.message : String(error)} — run \`pnpm build\``,
      cwd: "",
    });
    return results; // Nothing else can pass.
  }

  // 2. Does the shell it serves exist? A server that starts but cannot serve a
  //    page fails only once someone opens the URL, which is far too late.
  const web = path.resolve(path.dirname(serverEntryPath()), "..", "web", "index.html");
  results.push({
    target: undefined,
    label: "web assets",
    ok: fs.existsSync(web),
    detail: fs.existsSync(web) ? web : `missing ${web} — run \`pnpm build\``,
    cwd: "",
  });

  // 3. Does it actually start, from each cwd a real client would use?
  for (const testCase of launchCwdsToTest()) {
    const probe = await probeServer(testCase.cwd);
    results.push({
      target: undefined,
      label: `starts from ${testCase.label}`,
      ok: probe.ok,
      detail: probe.detail,
      cwd: testCase.cwd,
    });
  }

  // 4. Is it registered where the user thinks it is?
  for (const target of allTargets()) {
    if (!fs.existsSync(target.file)) continue;
    const text = fs.readFileSync(target.file, "utf8");
    const registered = text.includes('"agentui"') || text.includes("[mcp_servers.agentui]");
    const pointsAtBuild = text.includes(serverEntryPath().replace(/\\/g, "\\\\")) || text.includes(serverEntryPath());
    results.push({
      target,
      label: target.label,
      ok: registered && pointsAtBuild,
      detail: registered
        ? pointsAtBuild
          ? "registered"
          : "registered, but pointing at a different build — re-run `agentui install`"
        : "not registered — run `agentui install`",
      cwd: "",
    });
  }

  return results;
}

export function renderDiagnosis(results: Diagnosis[]): string {
  const width = Math.max(...results.map((r) => r.label.length));
  return results
    .map((r) => {
      const mark = r.ok ? "ok  " : "FAIL";
      const where = r.cwd ? `\n     ${r.cwd}` : "";
      return ` ${mark} ${r.label.padEnd(width)}  ${r.detail}${where}`;
    })
    .join("\n");
}

export function homeFallbackDir(): string {
  return path.join(os.homedir(), ".agentui", "sessions");
}
