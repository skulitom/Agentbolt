import { fileURLToPath } from "node:url";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { log, protectStdout } from "./log.js";
import { adoptProjectRoot, dataDir, projectName } from "./core/paths.js";
import { AgentUiContext } from "./mcp/context.js";
import { registerTools } from "./mcp/tools/index.js";

/**
 * Adopts the client's declared workspace root, if it declares one.
 *
 * Wrapped in a catch because a client that does not support `roots` answers
 * with a method-not-found error, which is a normal outcome here and not a
 * failure worth surfacing.
 */
async function adoptRootsFromClient(server: McpServer, ctx: AgentUiContext): Promise<void> {
  try {
    const capabilities = server.server.getClientCapabilities();
    if (!capabilities?.roots) return;

    const { roots } = await server.server.listRoots();
    const first = roots?.find((root) => root.uri.startsWith("file://"));
    if (!first) return;

    if (adoptProjectRoot(fileURLToPath(first.uri))) {
      // The data dir moved, so anything hydrated from the old guess is wrong.
      await ctx.hydrate();
    }
  } catch (error) {
    log.debug("client does not report roots; keeping the cwd-based data dir", String(error));
  }
}

async function main(): Promise<void> {
  // Before anything else: stdout belongs to the JSON-RPC stream alone. A single
  // stray console.log from us or from a dependency corrupts every message that
  // follows, and the failure looks like a mysterious protocol error rather than
  // a print statement.
  const transportStdout = protectStdout();

  const ctx = new AgentUiContext();
  // ctx.hydrate, not sessions.hydrate: restoring the session objects is only
  // half the job — each one must also be watched, or answering a restored
  // widget would neither reach a second open tab nor wake a parked ui_wait.
  await ctx.hydrate();

  const server = new McpServer(
    { name: "agentui", version: "0.1.0" },
    {
      instructions: [
        "AgentUI opens real interactive UIs in the user's browser so you can ask for input that prose",
        "cannot capture: tuning values, colours, a choice between options, approval of a diff.",
        "Use ui_ask for a single question you need answered now; ui_create for something the user",
        "adjusts while you keep working, then ui_wait to be woken when they respond.",
      ].join(" "),
    },
  );

  await registerTools(server, ctx);

  // The guarded writable is the only remaining path to the real stdout.
  await server.connect(new StdioServerTransport(process.stdin, transportStdout));

  // Now that a client is attached, ask it where the workspace actually is.
  // The cwd is only ever a guess — Claude Code launches us from the project,
  // Claude Desktop from C:\WINDOWS\system32 — and `roots` is the client telling
  // us directly. Entirely best-effort: clients that do not implement it leave
  // the cwd-based choice in place.
  await adoptRootsFromClient(server, ctx);

  log.info("agentui ready", {
    project: projectName(),
    dataDir: dataDir(),
    sessions: ctx.sessions.size,
  });
}

main().catch((error: unknown) => {
  log.error("fatal", error);
  process.exit(1);
});
