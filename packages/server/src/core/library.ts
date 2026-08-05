import path from "node:path";
import fsp from "node:fs/promises";
import {
  InterfaceSpec,
  SavedWidget,
  WIDGET_SLUG,
  nowIso,
  rankForIndex,
  scoreWidget,
  slugifyWidgetName,
  specSimilarity,
  toSummary,
  type LibraryScope,
  type WidgetSummary,
} from "@agentui/shared";
import { projectLibraryDir, projectName, userLibraryDir } from "./paths.js";
import { ensureDir, readJson, writeFileAtomic, writeJsonAtomic } from "./storage.js";
import { log } from "../log.js";

export interface SaveOptions {
  slug: string;
  name: string;
  description?: string | undefined;
  tags?: string[] | undefined;
  triggers?: string[] | undefined;
  spec: InterfaceSpec;
  previewModule?: string | undefined;
  lastValues?: Record<string, unknown> | undefined;
  scope: LibraryScope;
  overwrite: boolean;
}

export interface LoadedWidget {
  widget: SavedWidget;
  scope: LibraryScope;
  dir: string;
}

/**
 * The widget library: interfaces an agent got right once, kept so they never
 * have to be rebuilt.
 *
 * Stored as plain directories rather than a database so they can be read,
 * diffed, hand-edited and copied between machines. Global by default — a widget
 * built in one repo should be available in the next, which is the whole point —
 * with a per-project overlay for anything genuinely repo-specific.
 */
export class Library {
  /** Both roots, most-specific first: a project widget shadows a global one. */
  #roots(): Array<{ dir: string; scope: LibraryScope }> {
    return [
      { dir: projectLibraryDir(), scope: "project" },
      { dir: userLibraryDir(), scope: "user" },
    ];
  }

  #dirFor(slug: string, scope: LibraryScope): string {
    return path.join(scope === "project" ? projectLibraryDir() : userLibraryDir(), slug);
  }

  async load(slug: string): Promise<LoadedWidget | undefined> {
    if (!WIDGET_SLUG.test(slug)) return undefined;
    for (const { dir, scope } of this.#roots()) {
      const widgetDir = path.join(dir, slug);
      const raw = await readJson<unknown>(path.join(widgetDir, "widget.json"));
      if (!raw) continue;
      const parsed = SavedWidget.safeParse(raw);
      if (!parsed.success) {
        // A hand-edited widget that no longer validates must not break the
        // library; skip it loudly rather than crashing every lookup.
        log.warn("skipping invalid saved widget", { slug, scope });
        continue;
      }
      return { widget: parsed.data, scope, dir: widgetDir };
    }
    return undefined;
  }

  async previewModule(slug: string): Promise<string | undefined> {
    const found = await this.load(slug);
    if (!found?.widget.hasPreview) return undefined;
    return fsp.readFile(path.join(found.dir, "preview.mjs"), "utf8").catch(() => undefined);
  }

  async all(): Promise<LoadedWidget[]> {
    const seen = new Set<string>();
    const out: LoadedWidget[] = [];
    for (const { dir, scope } of this.#roots()) {
      const entries = await fsp.readdir(dir, { withFileTypes: true }).catch(() => []);
      for (const entry of entries) {
        if (!entry.isDirectory() || seen.has(entry.name)) continue;
        const loaded = await this.load(entry.name);
        if (!loaded) continue;
        seen.add(entry.name);
        out.push({ ...loaded, scope });
      }
    }
    return out;
  }

  async exists(slug: string): Promise<boolean> {
    return (await this.load(slug)) !== undefined;
  }

  /** Suggests a free slug when the requested one is taken. */
  async freeSlug(base: string): Promise<string> {
    const root = slugifyWidgetName(base);
    if (!(await this.exists(root))) return root;
    for (let n = 2; n < 100; n++) {
      const candidate = `${root}-${n}`;
      if (!(await this.exists(candidate))) return candidate;
    }
    return `${root}-${Date.now()}`;
  }

  /** Loads only from one scope. Saving must not see a widget it would shadow. */
  async #loadFrom(slug: string, scope: LibraryScope): Promise<SavedWidget | undefined> {
    const raw = await readJson<unknown>(path.join(this.#dirFor(slug, scope), "widget.json"));
    if (!raw) return undefined;
    const parsed = SavedWidget.safeParse(raw);
    return parsed.success ? parsed.data : undefined;
  }

  async save(options: SaveOptions): Promise<{ widget: SavedWidget; dir: string }> {
    // Scope-local on purpose: a project widget deliberately shadows a global one
    // of the same name, so the collision check must not look across both roots.
    const previous = await this.#loadFrom(options.slug, options.scope);
    const existing = previous ? { widget: previous } : undefined;
    if (existing && !options.overwrite) {
      throw new Error(
        `A widget called "${options.slug}" already exists. Pass overwrite:true to replace it, ` +
          `or use "${await this.freeSlug(options.slug)}".`,
      );
    }

    const dir = this.#dirFor(options.slug, options.scope);
    await ensureDir(dir);

    const widget: SavedWidget = {
      schemaVersion: 1,
      slug: options.slug,
      name: options.name,
      description: options.description ?? existing?.widget.description ?? "",
      tags: options.tags ?? existing?.widget.tags ?? [],
      triggers: options.triggers ?? existing?.widget.triggers ?? [],
      spec: options.spec,
      hasPreview: Boolean(options.previewModule) || Boolean(existing?.widget.hasPreview),
      lastValues: options.lastValues ?? existing?.widget.lastValues ?? {},
      uses: existing?.widget.uses ?? 0,
      usedInProjects: existing?.widget.usedInProjects ?? [],
      pinned: existing?.widget.pinned ?? false,
      createdAt: existing?.widget.createdAt ?? nowIso(),
      updatedAt: nowIso(),
      ...(existing?.widget.lastUsedAt === undefined ? {} : { lastUsedAt: existing.widget.lastUsedAt }),
    };

    await writeJsonAtomic(path.join(dir, "widget.json"), widget);
    if (options.previewModule) await writeFileAtomic(path.join(dir, "preview.mjs"), options.previewModule);

    log.info("widget saved", { slug: widget.slug, scope: options.scope });
    return { widget, dir };
  }

  /**
   * Records that a widget was instantiated. Usage is what the agent-facing index
   * ranks on, so this is what keeps the shortlist honest over time.
   */
  async recordUse(slug: string, lastValues?: Record<string, unknown>): Promise<void> {
    const found = await this.load(slug);
    if (!found) return;
    const project = projectName();
    const widget: SavedWidget = {
      ...found.widget,
      uses: found.widget.uses + 1,
      lastUsedAt: nowIso(),
      updatedAt: nowIso(),
      usedInProjects: found.widget.usedInProjects.includes(project)
        ? found.widget.usedInProjects
        : [...found.widget.usedInProjects, project],
      ...(lastValues ? { lastValues } : {}),
    };
    await writeJsonAtomic(path.join(found.dir, "widget.json"), widget).catch((error: unknown) =>
      log.warn("could not record widget use", { slug, error: String(error) }),
    );
  }

  async setPinned(slug: string, pinned: boolean): Promise<boolean> {
    const found = await this.load(slug);
    if (!found) return false;
    await writeJsonAtomic(path.join(found.dir, "widget.json"), { ...found.widget, pinned, updatedAt: nowIso() });
    return true;
  }

  async remove(slug: string): Promise<boolean> {
    const found = await this.load(slug);
    if (!found) return false;
    await fsp.rm(found.dir, { recursive: true, force: true });
    return true;
  }

  /** Compact search results — never the full spec, which is what `get` is for. */
  async search(query: string, limit = 10): Promise<{ results: WidgetSummary[]; total: number }> {
    const all = await this.all();
    if (!query.trim()) {
      const ranked = rankForIndex(all.map(({ widget, scope }) => ({ widget, scope })), projectName(), Date.now());
      return {
        results: ranked.slice(0, limit).map(({ widget, scope }) => toSummary(widget, scope)),
        total: all.length,
      };
    }

    const scored = all
      .map(({ widget, scope }) => ({ widget, scope, score: scoreWidget(widget, query) }))
      .filter((row) => row.score > 0.15)
      .sort((a, b) => b.score - a.score);

    return {
      results: scored.slice(0, limit).map((row) => toSummary(row.widget, row.scope, Math.round(row.score * 100) / 100)),
      total: scored.length,
    };
  }

  /**
   * Finds a saved widget close enough to a freshly authored spec to be worth
   * mentioning. This is the anti-forgetting mechanism that actually earns its
   * keep: it costs nothing until it fires, and it fires exactly when an agent
   * was about to rebuild something it already has.
   */
  async findSimilar(spec: InterfaceSpec, threshold = 0.6): Promise<{ summary: WidgetSummary; score: number } | undefined> {
    let best: { summary: WidgetSummary; score: number } | undefined;
    for (const { widget, scope } of await this.all()) {
      const score = specSimilarity(spec, widget);
      if (score >= threshold && (!best || score > best.score)) {
        best = { summary: toSummary(widget, scope, score), score };
      }
    }
    return best;
  }

  /**
   * The ranked shortlist folded into the ui_create tool description.
   *
   * Tool descriptions are always in context, which makes them the cheapest place
   * to keep the library discoverable — and the most expensive place to be
   * careless, hence the hard cap. The long tail is reached by search, not by
   * listing everything.
   */
  async indexForToolDescription(limit = Number(process.env["AGENT_UI_INDEX_LIMIT"] ?? 12)): Promise<string> {
    const all = await this.all();
    if (!all.length) return "";

    const ranked = rankForIndex(all.map(({ widget, scope }) => ({ widget, scope })), projectName(), Date.now());
    const shown = ranked.slice(0, limit);
    const width = Math.max(...shown.map((r) => r.widget.slug.length));

    const lines = shown.map(({ widget }) => {
      const summary = (widget.description || widget.spec.title).replace(/\s+/g, " ").slice(0, 68);
      return `  ${widget.slug.padEnd(width)}  ${summary}`;
    });

    if (ranked.length > shown.length) {
      lines.push(`  … ${ranked.length - shown.length} more — search with ui_library({ query })`);
    }

    return ['Saved widgets you can reuse with { template: "<slug>" }:', ...lines].join("\n");
  }
}
