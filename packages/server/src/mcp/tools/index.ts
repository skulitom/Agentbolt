import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { AgentUiContext } from "../context.js";
import { registerLibraryTools } from "../library-tools.js";
import { registerAsk } from "./ask.js";
import { registerCreate, createDescription } from "./create.js";
import { registerUpdate } from "./update.js";
import { registerWait } from "./wait.js";
import { registerGetState } from "./get-state.js";
import { registerClose } from "./close.js";
import { registerSchemaTool } from "./schema-tool.js";
import { log } from "../../log.js";

/**
 * The ui_* tools.
 *
 * One file per tool, because they are read one at a time — by a person changing
 * one, and by nobody at all otherwise. What they share lives in shared.ts
 * (result shaping) and session-ops.ts (the create and wait sequences that
 * ui_ask and ui_create/ui_wait each used to carry their own copy of).
 */
export async function registerTools(server: McpServer, ctx: AgentUiContext): Promise<void> {
  // The library shortlist is folded into ui_create's description at startup.
  // Tool descriptions are always in context, which makes them the cheapest place
  // to keep saved widgets discoverable — and the reason the list is hard-capped.
  const index = await ctx.library.indexForToolDescription();

  registerAsk(server, ctx);
  const create = registerCreate(server, ctx, index);
  registerLibraryTools(server, ctx);
  registerUpdate(server, ctx);
  registerWait(server, ctx);
  registerGetState(server, ctx);
  registerClose(server, ctx);
  registerSchemaTool(server);

  // Best-effort refresh after a save. Many clients fetch tools/list once and
  // never again, so this is a bonus rather than the mechanism discovery relies on.
  ctx.onLibraryChange(() => {
    void ctx.library
      .indexForToolDescription()
      .then((next) => create.update({ description: createDescription(next) }))
      .catch((error: unknown) => log.debug("could not refresh library index", String(error)));
  });
}
