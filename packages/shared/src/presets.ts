import { z } from "zod";
import {
  Control,
  ContentBlock,
  InterfaceSpec,
  PreviewOptions,
  formatSpecError,
  type Action,
  type InterfaceSpecInput,
} from "./spec.js";

/**
 * ui_ask's input, and the compiler that turns it into an ordinary Interface
 * Spec.
 *
 * The whole point is that the 90% case ("approve this?", "pick one", "fill
 * these in") costs an agent one line instead of thirty. That means being
 * generous about shorthand: options may be plain strings, fields may omit
 * labels and defaults, and content may be a bare markdown string. Everything is
 * inflated here so the renderer and the event pipeline only ever see one shape.
 */

/** A forgiving field shorthand: everything except `id` and `type` is optional. */
const FieldShorthand = z.looseObject({
  id: z.string(),
  // A new control type is invisible to `ui_ask` until it is listed here — the
  // enum rejects it before inflateField ever sees it.
  type: z
    .enum(["slider", "number", "select", "toggle", "color", "text", "vector", "compare", "multiselect"])
    .optional(),
  label: z.string().optional(),
  default: z.unknown().optional(),
  options: z.array(z.union([z.string(), z.looseObject({ value: z.string(), label: z.string().optional() })])).optional(),
});

const OptionShorthand = z.union([
  z.string(),
  z.strictObject({
    id: z.string().optional(),
    label: z.string(),
    style: z.enum(["default", "primary", "danger"]).optional(),
  }),
]);

const ContentShorthand = z.union([z.string(), ContentBlock, z.array(z.union([z.string(), ContentBlock]))]);

export const AskInput = z.strictObject({
  question: z.string().min(1),
  /** Extra prose under the question. */
  details: z.string().optional(),
  /** Buttons. Pressing one finalizes the session and reports the choice. */
  options: z.array(OptionShorthand).optional(),
  /** Controls to fill in before answering. */
  fields: z.array(FieldShorthand).optional(),
  /** Something to look at: a diff, an image, markdown, code. */
  content: ContentShorthand.optional(),
  /** Instantiate a saved widget instead of describing one inline. */
  template: z.string().optional(),
  title: z.string().optional(),
  timeoutMs: z.number().int().positive().max(3_600_000).optional(),
  /** Whether the free-text comment box is shown. Default true. */
  allowComment: z.boolean().default(true),
  /** `{ auto: false }` declines the generated preview — see PreviewOptions. */
  preview: PreviewOptions.optional(),
});

export type AskInput = z.infer<typeof AskInput>;

export function slugify(text: string): string {
  const slug = text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return /^[a-z]/.test(slug) ? slug : `opt_${slug || "x"}`;
}

function titleize(id: string): string {
  const words = id.replace(/[_-]+/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

function normalizeContent(content: AskInput["content"]): ContentBlock[] {
  if (content === undefined) return [];
  const list = Array.isArray(content) ? content : [content];
  return list.map((block) =>
    typeof block === "string" ? { kind: "markdown" as const, value: block, bind: false } : block,
  );
}

function normalizeOptions(options: AskInput["options"]): Action[] {
  if (!options?.length) return [];
  const used = new Set<string>();
  return options.map((option, index) => {
    const label = typeof option === "string" ? option : option.label;
    const explicit = typeof option === "string" ? undefined : option.id;
    let id = explicit ?? slugify(label);
    while (used.has(id)) id = `${id}_${index}`;
    used.add(id);
    const style = typeof option === "string" ? undefined : option.style;
    return { id, label, style: style ?? (index === 0 ? "primary" : "default") };
  });
}

/**
 * Inflates a field shorthand into a real Control, inferring the type from the
 * shape of `default` when it was not given and supplying a sane default when it
 * was omitted. Anything that cannot be inferred is reported by the caller
 * through normal spec validation.
 */
function inflateField(field: z.infer<typeof FieldShorthand>): unknown {
  const raw = field as Record<string, unknown>;
  const label = field.label ?? titleize(field.id);
  const type = field.type ?? inferType(field);

  const options = field.options?.map((option) => (typeof option === "string" ? { value: option } : option));

  const base: Record<string, unknown> = { ...raw, id: field.id, label, type };
  if (options) base["options"] = options;

  if (base["default"] === undefined) {
    switch (type) {
      case "text":
        base["default"] = "";
        break;
      case "toggle":
        base["default"] = false;
        break;
      case "color":
        base["default"] = "#888888";
        break;
      case "select":
        base["default"] = options?.[0]?.value ?? "";
        break;
      case "slider": {
        const min = typeof raw["min"] === "number" ? raw["min"] : 0;
        const max = typeof raw["max"] === "number" ? raw["max"] : 1;
        base["default"] = (min + max) / 2;
        break;
      }
      case "number":
        base["default"] = typeof raw["min"] === "number" ? raw["min"] : 0;
        break;
      case "vector": {
        const size = raw["size"] === 3 ? 3 : 2;
        base["size"] = size;
        base["default"] = Array.from({ length: size }, () => 0);
        break;
      }
      case "compare":
        base["default"] = options?.[0]?.value ?? "";
        break;
      // Nothing pre-checked is the only defensible guess: `default` on a
      // multiselect is a PROPOSAL, and inventing one would put words in the
      // agent's mouth that `touched` would then report back as a decision.
      case "multiselect":
        base["default"] = [];
        break;
    }
  }

  // A slider without bounds is the single most common authoring slip; give it
  // usable ones rather than rejecting the whole call.
  if (type === "slider") {
    if (typeof base["min"] !== "number") base["min"] = 0;
    if (typeof base["max"] !== "number") base["max"] = 1;
  }

  // A vector inferred from `default: [x, y]` carries its arity in the default,
  // so derive `size` rather than making the author repeat it.
  if (type === "vector" && base["size"] !== 2 && base["size"] !== 3) {
    const fromDefault = Array.isArray(base["default"]) ? base["default"].length : 0;
    base["size"] = fromDefault === 3 ? 3 : 2;
    if (!Array.isArray(base["default"])) base["default"] = [0, 0];
  }

  return base;
}

function inferType(field: z.infer<typeof FieldShorthand>): string {
  const value = field.default;
  // An array of STRINGS is a selection, an array of numbers is a position. The
  // distinction has to be made here or every multiselect shorthand arrives as a
  // vector and fails with a message about arity.
  const chosen = Array.isArray(value) && value.length > 0 && value.every((item) => typeof item === "string");
  if (field.options?.length) return chosen ? "multiselect" : "select";
  if (typeof value === "boolean") return "toggle";
  if (typeof value === "number") return "number";
  if (Array.isArray(value)) return chosen ? "multiselect" : "vector";
  if (typeof value === "string" && /^#[0-9a-fA-F]{6}$/.test(value)) return "color";
  return "text";
}

export interface CompiledAsk {
  spec: InterfaceSpecInput;
  /** True when the widget has buttons, i.e. the answer is a choice. */
  hasChoices: boolean;
}

/**
 * Compiles ui_ask input into a spec. ui_ask is not a separate code path — this
 * produces the same JSON an agent could have written by hand, so there is
 * exactly one renderer and one event pipeline to reason about.
 */
export function compileAsk(input: AskInput): CompiledAsk {
  const content = normalizeContent(input.content);
  const actions = normalizeOptions(input.options);
  const controls = (input.fields ?? []).map(inflateField);

  // The question leads the content, so it reads as the prompt rather than as a
  // page title buried in chrome.
  const heading: ContentBlock = {
    kind: "markdown",
    value: input.details ? `**${input.question}**\n\n${input.details}` : `**${input.question}**`,
    bind: false,
  };

  const spec: InterfaceSpecInput = {
    schemaVersion: 1,
    title: input.title ?? input.question,
    content: [heading, ...content],
    controls: controls as InterfaceSpecInput["controls"],
    // With no buttons the widget still needs a way to finish; the renderer
    // falls back to a plain submit action.
    actions: actions.length ? actions : [{ id: "submit", label: controls.length ? "Submit" : "OK", style: "primary" }],
    comment: { enabled: input.allowComment },
  };
  if (input.preview) spec.preview = input.preview;

  return { spec, hasChoices: actions.length > 0 };
}

/** Validates a compiled ask, surfacing errors in the same format as ui_create. */
export function compileAskChecked(input: AskInput): { ok: true; spec: InterfaceSpec } | { ok: false; message: string } {
  const { spec } = compileAsk(input);
  const parsed = InterfaceSpec.safeParse(spec);
  if (parsed.success) return { ok: true, spec: parsed.data };
  return { ok: false, message: formatSpecError(parsed.error) };
}

export type { Control };
