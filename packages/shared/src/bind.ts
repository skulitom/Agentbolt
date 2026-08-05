import type { Control, Values } from "./spec.js";

/**
 * `bind`: put the string being decided in the places it will actually appear.
 *
 * A package name in a list of three candidates tells you nothing. The same name
 * in the import line, the npx invocation, the .mcp.json key and the directory
 * tree tells you that it reads fine as an npm name and terribly as a repeated
 * CLI verb — which is the answer, and it is not visible any other way.
 *
 * The substitution rule is deliberately timid. `{{id}}` is replaced only when
 * `id` is a DECLARED CONTROL; everything else between braces is left byte for
 * byte alone. That is why there is no escape syntax: `${{ github.token }}`,
 * `{{#each}}` and `{{ .Values.image }}` all carry a dot or a sigil, so they can
 * never be mistaken for a placeholder, and a widget about a Helm chart stays
 * authorable. An escape hatch nobody uses correctly is the single biggest
 * source of breakage in templating systems.
 */

export const BIND_TRANSFORMS = ["pascal", "camel", "kebab"] as const;
export type BindTransform = (typeof BIND_TRANSFORMS)[number];

/**
 * Kept as a source string, so `collectPlaceholders` and `interpolate` can each
 * build their own global regex rather than share one with a moving `lastIndex`.
 */
const PLACEHOLDER = String.raw`\{\{\s*([a-z][a-z0-9_]*)(?::([a-z]+))?\s*\}\}`;

export interface Placeholder {
  /** The matched text, e.g. `{{pkg:pascal}}`. */
  raw: string;
  id: string;
  /** Present but possibly not a real transform — validation reports those. */
  transform: string | undefined;
}

export function collectPlaceholders(text: string): Placeholder[] {
  const found: Placeholder[] = [];
  const regex = new RegExp(PLACEHOLDER, "g");
  for (const match of text.matchAll(regex)) {
    found.push({ raw: match[0], id: match[1]!, transform: match[2] });
  }
  return found;
}

export function isBindTransform(name: string): name is BindTransform {
  return (BIND_TRANSFORMS as readonly string[]).includes(name);
}

/**
 * Substitutes declared control values into a bound block.
 *
 * Anything it cannot render — an unknown id, an unknown transform, a toggle or a
 * multiselect — is left exactly as written. Those are spec errors caught at
 * parse time; leaving them visible is the honest fallback if one ever gets past.
 */
export function interpolate(text: string, controls: Control[], values: Values): string {
  const byId = new Map(controls.map((control) => [control.id, control]));
  return text.replace(new RegExp(PLACEHOLDER, "g"), (raw: string, id: string, transform?: string) => {
    const control = byId.get(id);
    if (!control) return raw;
    if (transform !== undefined && !isBindTransform(transform)) return raw;
    const rendered = valueToText(values[id] ?? control.default);
    if (rendered === undefined) return raw;
    return transform ? applyTransform(rendered, transform) : rendered;
  });
}

/**
 * Value → text. A number arrives bare, without its unit: the unit belongs in the
 * template (`timeout: {{ms}}ms`), where the author can see it sit next to the
 * number. A select gives its VALUE, not its label — that is what lets option
 * values be the alternative snippets, and it is why `bind` needs no `{{#if}}`.
 */
function valueToText(value: unknown): string | undefined {
  if (typeof value === "string") return value;
  if (typeof value === "number") {
    return Number.isInteger(value) ? String(value) : String(Math.round(value * 10000) / 10000);
  }
  return undefined;
}

export function applyTransform(value: string, transform: BindTransform): string {
  const parts = words(value);
  if (!parts.length) return value;
  switch (transform) {
    case "pascal":
      return parts.map(capitalize).join("");
    case "camel":
      return parts[0]!.toLowerCase() + parts.slice(1).map(capitalize).join("");
    case "kebab":
      return parts.map((word) => word.toLowerCase()).join("-");
  }
}

/** Splits on separators AND camelCase humps, so every casing round-trips. */
function words(value: string): string[] {
  return value
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .split(/[^a-zA-Z0-9]+/)
    .filter(Boolean);
}

function capitalize(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
}
