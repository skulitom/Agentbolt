import { afterAll, beforeAll, describe, expect, it } from "vitest";
import path from "node:path";
import os from "node:os";
import net from "node:net";
import fsp from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

/**
 * End-to-end against the REAL built server over a real stdio transport.
 *
 * This is the test that matters most for this project: it exercises the exact
 * path an agent uses, so it catches protocol-level breakage (schema conversion,
 * structuredContent guards, stdout contamination) that unit tests cannot see.
 */

const here = path.dirname(fileURLToPath(import.meta.url));
const serverEntry = path.resolve(here, "..", "dist", "mcp.js");

/** Raw HTTP GET so we can set headers `fetch` forbids, such as Host. */
function rawGet(port: number, requestPath: string, host: string): Promise<number> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, "127.0.0.1", () => {
      socket.write(`GET ${requestPath} HTTP/1.1\r\nHost: ${host}\r\nConnection: close\r\n\r\n`);
    });
    let buffer = "";
    socket.on("data", (chunk) => {
      buffer += chunk.toString();
    });
    socket.on("end", () => {
      const match = /^HTTP\/1\.\d (\d{3})/.exec(buffer);
      if (match) resolve(Number(match[1]));
      else reject(new Error(`no status line in response: ${buffer.slice(0, 120)}`));
    });
    socket.on("error", reject);
  });
}

let client: Client;
let dataDir: string;

beforeAll(async () => {
  dataDir = await fsp.mkdtemp(path.join(os.tmpdir(), "agentui-e2e-"));

  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [serverEntry],
    env: {
      ...(process.env as Record<string, string>),
      AGENT_UI_DATA_DIR: dataDir,
      // Never launch a browser from a test run.
      AGENT_UI_NO_OPEN: "1",
      AGENT_UI_LOG_LEVEL: "error",
    },
    stderr: "pipe",
  });

  client = new Client({ name: "agentui-e2e", version: "0.0.0" });
  await client.connect(transport);
}, 60_000);

afterAll(async () => {
  await client?.close().catch(() => {});
  await fsp.rm(dataDir, { recursive: true, force: true }).catch(() => {});
});

describe("mcp surface", () => {
  it("advertises the ui_* tools with usable schemas", async () => {
    const { tools } = await client.listTools();
    const names = tools.map((t) => t.name).sort();
    expect(names).toContain("ui_create");
    expect(names).toContain("ui_get_state");
    expect(names).toContain("ui_close");

    const create = tools.find((t) => t.name === "ui_create");
    expect(create?.description).toMatch(/ui_ask|ui_wait/);
    expect(create?.inputSchema).toBeTruthy();

    // The zod-4 trap: a field with .default() is emitted as REQUIRED under
    // io:"output" conversion, which would force agents to pass arguments they
    // should be able to omit. Nothing here is mandatory — `spec` and `template`
    // are alternatives, checked at runtime with a message that says so.
    const required = (create?.inputSchema as { required?: string[] }).required ?? [];
    expect(required).toEqual([]);

    // The library shortlist is folded into this description at startup.
    expect(create?.description).toMatch(/ui_library/);

    // The preview import surface lives in the previewModule field description,
    // because bare MCP clients never load the skill file. "agentui" appearing
    // here is what tells such a client the motion helpers exist at all.
    expect(JSON.stringify(create?.inputSchema)).toMatch(/agentui/);
  });

  it("creates a session, returns structured content, and writes files to disk", async () => {
    const result = await client.callTool({
      name: "ui_create",
      arguments: {
        spec: {
          title: "Muzzle flash",
          controls: [
            { id: "intensity", label: "Intensity", type: "slider", min: 0, max: 1, default: 0.7 },
            { id: "tint", label: "Tint", type: "color", default: "#ef9f27" },
          ],
        },
        open: false,
      },
    });

    expect(result.isError).toBeFalsy();
    const structured = result.structuredContent as {
      sessionId: string;
      url: string;
      values: Record<string, unknown>;
    };
    expect(structured.sessionId).toMatch(/^[a-z0-9]{10}$/);
    expect(structured.url).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/s\//);
    expect(structured.values).toEqual({ intensity: 0.7, tint: "#ef9f27" });

    const dir = path.join(dataDir, "sessions", structured.sessionId);
    const spec = JSON.parse(await fsp.readFile(path.join(dir, "spec.json"), "utf8"));
    const state = JSON.parse(await fsp.readFile(path.join(dir, "state.json"), "utf8"));
    const events = (await fsp.readFile(path.join(dir, "events.ndjson"), "utf8")).trim().split("\n");

    expect(spec.title).toBe("Muzzle flash");
    expect(state.status).toBe("active");
    expect(state.values.intensity).toBe(0.7);
    expect(JSON.parse(events[0]!).type).toBe("session_created");

    // The data dir must ignore itself so it never dirties someone else's repo.
    expect(await fsp.readFile(path.join(dataDir, ".gitignore"), "utf8")).toContain("*");
  });

  it("rejects an invalid spec with a path-precise, actionable message", async () => {
    const result = await client.callTool({
      name: "ui_create",
      arguments: {
        spec: {
          title: "Bad",
          controls: [{ id: "Gain", label: "Gain", type: "slider", minimum: 0, max: 1, default: 5 }],
        },
        open: false,
      },
    });

    expect(result.isError).toBe(true);
    const message = (result.content as Array<{ text: string }>)[0]!.text;
    expect(message).toContain("controls[0]");
    expect(message).toMatch(/snake_case/);
    expect(message).toMatch(/minimum/);
  });

  it("lists sessions when ui_get_state is called with no id", async () => {
    const result = await client.callTool({ name: "ui_get_state", arguments: {} });
    const { sessions } = result.structuredContent as { sessions: Array<{ title: string }> };
    expect(sessions.length).toBeGreaterThan(0);
    expect(sessions.some((s) => s.title === "Muzzle flash")).toBe(true);
  });

  /**
   * The dashboard and the agent must never disagree about what is still waiting.
   * They read the same summaries server-side; this checks that neither transport
   * quietly reshapes them on the way out — which is how the two would drift.
   */
  it("gives the dashboard and ui_get_state the same view of what is waiting", async () => {
    const listed = await client.callTool({ name: "ui_get_state", arguments: {} });
    const fromTool = (listed.structuredContent as { sessions: Array<Record<string, unknown>> }).sessions;

    const origin = new URL(String(fromTool[0]!["url"])).origin;
    const response = await fetch(`${origin}/api/sessions`);
    const fromDashboard = ((await response.json()) as { sessions: Array<Record<string, unknown>> }).sessions;

    const compare = (rows: Array<Record<string, unknown>>) =>
      rows.map((row) => [row["sessionId"], row["status"], row["stale"]]);
    expect(compare(fromDashboard)).toEqual(compare(fromTool));

    // Freshly created, so nothing here is a leftover — and the badge agrees.
    expect(fromTool.every((row) => row["stale"] === false)).toBe(true);
    const ping = (await (await fetch(`${origin}/api/ping`)).json()) as { pending: number; stale: number };
    expect(ping.pending).toBe(fromTool.filter((row) => row["status"] === "active").length);
    expect(ping.stale).toBe(0);
  });

  it("serves the session page and API over the loopback HTTP server", async () => {
    const created = await client.callTool({
      name: "ui_create",
      arguments: { spec: { title: "Served", controls: [{ id: "a", label: "A", type: "toggle", default: true }] }, open: false },
    });
    const { url, sessionId } = created.structuredContent as { url: string; sessionId: string };
    const origin = new URL(url).origin;

    const page = await fetch(url);
    expect(page.status).toBe(200);
    expect(await page.text()).toContain("<div id=\"root\">");

    const api = await fetch(`${origin}/api/session/${sessionId}`);
    expect(api.status).toBe(200);
    const body = (await api.json()) as { spec: { title: string }; state: { status: string } };
    expect(body.spec.title).toBe("Served");
    expect(body.state.status).toBe("active");
  });

  it("refuses requests with a forged Host header (DNS rebinding guard)", async () => {
    const state = await client.callTool({ name: "ui_get_state", arguments: {} });
    const { sessions } = state.structuredContent as { sessions: Array<{ url: string }> };
    const port = Number(new URL(sessions[0]!.url, "http://127.0.0.1").port);

    // `fetch` refuses to set Host (a forbidden header), so it cannot express
    // this attack at all — a raw request is the only way to actually test it.
    const status = await rawGet(port, "/api/ping", "evil.example");
    expect(status).toBe(403);

    // Control: the same request with a legitimate Host still works, proving the
    // 403 came from the guard and not from a broken request.
    expect(await rawGet(port, "/api/ping", `127.0.0.1:${port}`)).toBe(200);
  });

  it("closes a session and stops listing it", async () => {
    const created = await client.callTool({
      name: "ui_create",
      arguments: { spec: { title: "Temp", controls: [{ id: "a", label: "A", type: "toggle", default: false }] }, open: false },
    });
    const { sessionId } = created.structuredContent as { sessionId: string };

    const closed = await client.callTool({ name: "ui_close", arguments: { sessionId } });
    expect((closed.structuredContent as { closed: boolean }).closed).toBe(true);

    const list = await client.callTool({ name: "ui_get_state", arguments: {} });
    const { sessions } = list.structuredContent as { sessions: Array<{ sessionId: string }> };
    expect(sessions.some((s) => s.sessionId === sessionId)).toBe(false);
  });
});

/**
 * What the AGENT sees of the spec, and what it costs.
 *
 * ui_ask's rich parameters and ui_create.spec were `z.unknown()`, so the only
 * structural feedback an agent ever got was a failed call. The full schemas are
 * not the fix: inlining them costs roughly 5,950 tokens RESIDENT ON EVERY
 * REQUEST. Shallow-but-typed containers plus an on-demand ui_schema is, and the
 * budget below is what stops that decision being quietly undone the next time
 * someone "just adds the real schema".
 */
describe("the schema an agent can see", () => {
  it("advertises the real control vocabulary on ui_ask and ui_create", async () => {
    const { tools } = await client.listTools();
    const ask = tools.find((t) => t.name === "ui_ask");
    const create = tools.find((t) => t.name === "ui_create");

    // The enums are the part that actually prevents a failed call: an agent
    // guessing "checkbox" or "textarea" is corrected before it calls.
    expect(JSON.stringify(ask?.inputSchema)).toContain("multiselect");
    expect(JSON.stringify(create?.inputSchema)).toContain("multiselect");
    // ...but not the full detail of every variant.
    expect(JSON.stringify(create?.inputSchema)).not.toContain("weightUnit");
  });

  it("keeps those schemas inside a token budget", async () => {
    const { tools } = await client.listTools();
    const sizeOf = (name: string) => JSON.stringify(tools.find((t) => t.name === name)?.inputSchema ?? {}).length;

    // ~4 chars per token. The full InterfaceSpec alone is ~7,700 chars, so what
    // these numbers are really guarding against is someone inlining it.
    //
    // Raised once, deliberately, when `preview.views` was added: +325 tokens
    // across the three spec-bearing schemas. Paid because the list of view
    // names is exactly the thing an agent cannot guess, and a vocabulary you
    // have to discover by failed call is the problem the shallow schema exists
    // to solve. Raising these should always be a decision with a reason, which
    // is the entire point of the test.
    expect(sizeOf("ui_create"), "ui_create.inputSchema has grown past its budget").toBeLessThan(3000);
    expect(sizeOf("ui_ask"), "ui_ask.inputSchema has grown past its budget").toBeLessThan(3400);
  });

  it("has no defaulted field in an exposed schema, which zod would emit as required", async () => {
    const { tools } = await client.listTools();
    for (const name of ["ui_ask", "ui_create", "ui_update"]) {
      const schema = tools.find((t) => t.name === name)?.inputSchema as
        | { required?: string[]; properties?: Record<string, unknown> }
        | undefined;
      // Only `question` (ui_ask) and `sessionId` (ui_update) are genuinely
      // required. A `.default()` anywhere in these schemas would silently add
      // to this list under zod 4's output mode.
      const required = schema?.required ?? [];
      expect(required.length, `${name} requires ${required.join(", ")}`).toBeLessThanOrEqual(1);
    }
  });

  it("serves the full shape of one control type on demand", async () => {
    const result = (await client.callTool({
      name: "ui_schema",
      arguments: { of: "control", type: "slider" },
    })) as { content: Array<{ text: string }>; structuredContent?: Record<string, unknown> };

    const body = result.content[0]?.text ?? "";
    expect(body).toContain('"min"');
    expect(body).toContain('"max"');
    expect(body).toContain('"unit"');
    expect(result.structuredContent?.["type"]).toBe("slider");
  });

  it("names the real types when asked for one that does not exist", async () => {
    const result = (await client.callTool({
      name: "ui_schema",
      arguments: { of: "control", type: "checkbox" },
    })) as { isError?: boolean; content: Array<{ text: string }> };

    expect(result.isError).toBe(true);
    expect(result.content[0]?.text).toContain("multiselect");
  });

  it("describes the preview module contract, which no JSON Schema could", async () => {
    const result = (await client.callTool({ name: "ui_schema", arguments: { of: "preview-module" } })) as {
      content: Array<{ text: string }>;
    };
    expect(result.content[0]?.text).toContain("update(values, meta)");
    expect(result.content[0]?.text).toContain("agentui");
  });
});
