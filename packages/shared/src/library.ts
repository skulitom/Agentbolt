import { z } from "zod";
import { InterfaceSpec } from "./spec.js";

/**
 * The widget library: what makes an interface an agent got right once reusable
 * forever. Stored as plain directories so it can be edited, diffed and copied
 * by hand; index.json is only ever a derived cache.
 */

export const LIBRARY_SCOPES = ["user", "project"] as const;
export type LibraryScope = (typeof LIBRARY_SCOPES)[number];

export const WIDGET_SLUG = /^[a-z0-9][a-z0-9-]*$/;

export function slugifyWidgetName(name: string): string {
  const slug = name
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug || "widget";
}

export const SavedWidget = z.object({
  schemaVersion: z.literal(1),
  slug: z.string().regex(WIDGET_SLUG),
  name: z.string(),
  description: z.string().default(""),
  tags: z.array(z.string()).default([]),
  /**
   * Short phrases that should surface this widget in a search. The highest
   * signal field for discovery, and the cheapest for an agent to supply.
   */
  triggers: z.array(z.string()).default([]),
  spec: InterfaceSpec,
  hasPreview: z.boolean().default(false),
  /** Where the human left it last time — reuse resumes here, not at authored defaults. */
  lastValues: z.record(z.string(), z.unknown()).default({}),
  uses: z.number().int().nonnegative().default(0),
  lastUsedAt: z.string().optional(),
  usedInProjects: z.array(z.string()).default([]),
  /** Pinned widgets always appear in the agent-facing index. */
  pinned: z.boolean().default(false),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export type SavedWidget = z.infer<typeof SavedWidget>;

/** The compact row returned by search — never the full spec. */
export interface WidgetSummary {
  slug: string;
  name: string;
  description: string;
  tags: string[];
  scope: LibraryScope;
  uses: number;
  lastUsedAt?: string;
  pinned: boolean;
  hasPreview: boolean;
  score?: number;
}

export function toSummary(widget: SavedWidget, scope: LibraryScope, score?: number): WidgetSummary {
  const summary: WidgetSummary = {
    slug: widget.slug,
    name: widget.name,
    description: widget.description,
    tags: widget.tags,
    scope,
    uses: widget.uses,
    pinned: widget.pinned,
    hasPreview: widget.hasPreview,
  };
  if (widget.lastUsedAt !== undefined) summary.lastUsedAt = widget.lastUsedAt;
  if (score !== undefined) summary.score = score;
  return summary;
}

/* ------------------------------------------------------------------ search */

function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter((t) => t.length > 1);
}

function trigrams(text: string): Set<string> {
  const clean = text.toLowerCase().replace(/[^a-z0-9]+/g, " ");
  const out = new Set<string>();
  for (let i = 0; i < clean.length - 2; i++) out.add(clean.slice(i, i + 3));
  return out;
}

function diceCoefficient(a: Set<string>, b: Set<string>): number {
  if (!a.size || !b.size) return 0;
  let shared = 0;
  for (const gram of a) if (b.has(gram)) shared++;
  return (2 * shared) / (a.size + b.size);
}

/** The searchable text of a widget, weighted by putting high-signal fields first. */
export function searchCorpus(widget: SavedWidget): { weighted: string; tokens: Set<string> } {
  const controlLabels = widget.spec.controls.map((c) => `${c.id} ${c.label}`).join(" ");
  const weighted = [
    widget.name,
    widget.name,
    widget.triggers.join(" "),
    widget.triggers.join(" "),
    widget.tags.join(" "),
    widget.slug,
    widget.description,
    widget.spec.title,
    controlLabels,
  ].join(" ");
  return { weighted, tokens: new Set(tokenize(weighted)) };
}

/**
 * Hybrid local scoring: token overlap for precision, trigram similarity so
 * near-misses ("muzzleflash" vs "muzzle flash") still land. No network, no
 * embedding key — at library sizes of tens to hundreds this is plenty, and it
 * works offline like everything else here.
 */
export function scoreWidget(widget: SavedWidget, query: string): number {
  const q = query.trim();
  if (!q) return 0;

  const { weighted, tokens } = searchCorpus(widget);
  const queryTokens = tokenize(q);
  if (!queryTokens.length) return 0;

  let hits = 0;
  for (const token of queryTokens) {
    if (tokens.has(token)) hits += 1;
    else if ([...tokens].some((t) => t.startsWith(token) || token.startsWith(t))) hits += 0.5;
  }
  const overlap = hits / queryTokens.length;
  const fuzzy = diceCoefficient(trigrams(q), trigrams(weighted));

  let score = overlap * 0.75 + fuzzy * 0.25;
  if (widget.slug === slugifyWidgetName(q)) score = 1;
  return Math.min(1, score);
}

/**
 * Similarity between a freshly authored spec and a saved widget, used to fire
 * the "you already have one of these" hint at the exact moment an agent would
 * otherwise rebuild something. Compares structure, not prose.
 */
export function specSimilarity(spec: InterfaceSpec, widget: SavedWidget): number {
  const a = new Set(spec.controls.map((c) => `${c.type}:${c.id}`));
  const b = new Set(widget.spec.controls.map((c) => `${c.type}:${c.id}`));
  if (!a.size && !b.size) return 0;

  let shared = 0;
  for (const key of a) if (b.has(key)) shared++;
  const jaccard = shared / (a.size + b.size - shared || 1);

  const titleSim = diceCoefficient(trigrams(spec.title), trigrams(`${widget.name} ${widget.spec.title}`));

  // Structure dominates: two widgets with the same control ids are the same
  // widget even if the agent gave it a different title this time.
  return Math.min(1, jaccard * 0.8 + titleSim * 0.2);
}

/**
 * Ranking for the always-present index folded into the tool description.
 * Pinned first, then widgets already used in this project, then recency
 * weighted by how often it has actually earned its place.
 */
export function rankForIndex(
  widgets: Array<{ widget: SavedWidget; scope: LibraryScope }>,
  project: string,
  now: number,
): Array<{ widget: SavedWidget; scope: LibraryScope }> {
  return [...widgets].sort((x, y) => score(y) - score(x));

  function score({ widget }: { widget: SavedWidget }): number {
    let value = 0;
    if (widget.pinned) value += 1000;
    if (widget.usedInProjects.includes(project)) value += 100;
    value += Math.log1p(widget.uses) * 10;
    if (widget.lastUsedAt) {
      const days = (now - Date.parse(widget.lastUsedAt)) / 86_400_000;
      if (Number.isFinite(days)) value += Math.max(0, 30 - days);
    }
    return value;
  }
}
