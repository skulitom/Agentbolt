import type { Values, WidgetSummary } from "@agentui/shared";
import type { Library } from "./library.js";
import type { Session } from "./session.js";
import type { SessionManager } from "./session-manager.js";

/**
 * Instantiating a saved widget: load, resolve its preview module, restore the
 * values the user last chose, record the use.
 *
 * Shared by `ui_create({ template })` and `POST /api/library/:slug/open` — the
 * agent side and the human side of the library, which had the same four steps
 * written out separately. Lives in core/ rather than mcp/ because http.ts must
 * not import from the MCP layer.
 *
 * What each caller still owns is what happens AROUND the session: the tool
 * watches it and opens a browser, the HTTP route notifies its own hook.
 */
export type OpenSavedWidget =
  | { ok: true; session: Session }
  | { ok: false; slug: string; near: WidgetSummary[] };

export async function openSavedWidget(
  sessions: SessionManager,
  library: Library,
  slug: string,
  overrides: { previewModule?: string | undefined; values?: Values | undefined } = {},
): Promise<OpenSavedWidget> {
  const saved = await library.load(slug);
  if (!saved) {
    // The near misses matter more than the 404: an agent that guessed a slug
    // can correct itself in one retry instead of listing the whole library.
    const { results } = await library.search(slug, 5);
    return { ok: false, slug, near: results };
  }

  const session = await sessions.create({
    spec: saved.widget.spec,
    previewModule: overrides.previewModule ?? (await library.previewModule(slug)),
    // Reopening where the user left it is what makes reuse feel like reuse
    // rather than starting from scratch with a familiar shape.
    values: overrides.values ?? (saved.widget.lastValues as Values | undefined),
    template: slug,
  });
  await library.recordUse(slug);
  return { ok: true, session };
}
