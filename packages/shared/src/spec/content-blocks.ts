import { z } from "zod";
import { collectPlaceholders, isBindTransform } from "../bind.js";
import { RULE_SHAPE, parseSampleRule } from "../sample.js";
import type { Ctx } from "./errors.js";
import { behaviourFor } from "./control-registry.js";
import type { Control } from "./control-schemas.js";

/**
 * Read-only content shown above the controls, so an agent can *show* something
 * and ask about it — a diff, a screenshot it just took, a block of prose.
 *
 * Same shape as control-registry.ts and for the same reason: the per-kind
 * knowledge was spread across a superRefine, two auto-preview predicates, an
 * asset-copying branch in session-manager, an inbox heuristic, and a renderer
 * switch. `CONTENT_BLOCKS` is a mapped Record, so a new kind cannot be added
 * without the compiler naming what it still owes.
 */
export const ContentBlock = z.discriminatedUnion("kind", [
  /** `bind` substitutes `{{control_id}}` live — see bind.ts for the rules. */
  z.strictObject({ kind: z.literal("markdown"), value: z.string(), bind: z.boolean().default(false) }),
  z.strictObject({
    kind: z.literal("code"),
    value: z.string(),
    language: z.string().optional(),
    filename: z.string().optional(),
    bind: z.boolean().default(false),
  }),
  z.strictObject({ kind: z.literal("diff"), value: z.string(), filename: z.string().optional() }),
  /**
   * A rule replayed against rows the reader recognises.
   *
   * `source` is REQUIRED and always rendered, because the failure this block
   * exists to prevent is a convincing table of numbers the agent made up. An
   * agent that has to type "illustrative — I have not read your machine" will
   * usually go and read it instead, and a human who sees that string discounts
   * correctly. Same move as `touched`: make the weak case visible rather than
   * asking anyone to remember it.
   */
  z.strictObject({
    kind: z.literal("sample"),
    label: z.string().min(1),
    /** A path, a command, or the formula. The provenance gate. */
    source: z.string().min(1),
    /** Exactly "<row_field> <op> <control_id>". Not an expression language. */
    rule: z.string(),
    /** [when the rule holds, when it does not]. */
    verdict: z.tuple([z.string(), z.string()]).optional(),
    /** Numeric field to sort by and show. Defaults to the rule's field. */
    field: z.string().optional(),
    unit: z.string().optional(),
    /** Live sum over matching rows. */
    total: z
      .strictObject({ field: z.string(), unit: z.string().optional(), label: z.string().optional() })
      .optional(),
    rows: z
      .array(z.looseObject({ label: z.string().min(1), note: z.string().optional() }))
      .min(2, "with fewer than two rows you have a sentence, not a table")
      .max(60),
  }),
  z.strictObject({
    kind: z.literal("image"),
    /** Absolute local path; the server copies it into the session's assets dir. */
    path: z.string().optional(),
    /** Or an inline data: URI, for something the agent generated in memory. */
    dataUri: z.string().optional(),
    alt: z.string().optional(),
    caption: z.string().optional(),
  }),
]);

export type ContentBlock = z.infer<typeof ContentBlock>;
export type ContentKind = ContentBlock["kind"];
export type BlockOfKind<K extends ContentKind> = Extract<ContentBlock, { kind: K }>;

export interface ContentBehaviour<B extends ContentBlock = ContentBlock> {
  /** Structural checks against the rest of the spec. */
  refine?(block: B, at: (...rest: Array<string | number>) => Array<string | number>, controlsById: Map<string, Control>, ctx: Ctx): void;

  /** Whether `{{placeholders}}` in this block's value are substituted. */
  bindable?: boolean;

  /** Which control ids this block illustrates, for control ordering. */
  illustrates?(block: B): string[];

  /**
   * A local path the server must copy into the session's assets directory.
   *
   * Declared here rather than hand-coded as `kind === "image" && block.path`
   * in session-manager, which would silently miss a second path-carrying kind.
   */
  localPath?(block: B): string | undefined;

  /**
   * The same block with its local path replaced by the served URL, once the
   * file has been copied. Paired with `localPath` so a kind that declares one
   * declares the other — otherwise the copier would know how to FIND an asset
   * it does not know how to rewrite.
   */
  withLocalPath?(block: B, url: string): B;

  /**
   * Is this block something the human has to LOOK at, as opposed to read in a
   * notification? False only for plain prose. Decides whether the inbox may
   * offer one-click answers instead of opening the widget.
   */
  needsLookingAt(block: B): boolean;
}

type Registry = { [K in ContentKind]: ContentBehaviour<BlockOfKind<K>> };

export const CONTENT_BLOCKS: Registry = {
  markdown: {
    bindable: true,
    refine: (block, at, controlsById, ctx) => {
      if (block.bind) checkBoundValue(block.value, at("value"), controlsById, ctx);
    },
    illustrates: (block) => (block.bind ? collectPlaceholders(block.value).map((p) => p.id) : []),
    // Prose is the one thing you can act on without looking: it is the whole
    // content of the notification.
    needsLookingAt: (block) => block.bind,
  },

  code: {
    bindable: true,
    refine: (block, at, controlsById, ctx) => {
      if (block.bind) checkBoundValue(block.value, at("value"), controlsById, ctx);
    },
    illustrates: (block) => (block.bind ? collectPlaceholders(block.value).map((p) => p.id) : []),
    needsLookingAt: () => true,
  },

  diff: {
    needsLookingAt: () => true,
  },

  sample: {
    refine: refineSample,
    illustrates: (block) => {
      const control = parseSampleRule(block.rule)?.control;
      return control ? [control] : [];
    },
    needsLookingAt: () => true,
  },

  image: {
    refine: (block, at, _controls, ctx) => {
      if (!block.path && !block.dataUri) {
        ctx.addIssue({
          code: "custom",
          path: at(),
          message: "an image block needs either `path` (absolute local path) or `dataUri`",
        });
      }
    },
    localPath: (block) => block.path,
    withLocalPath: (block, url) => {
      // Rebuilt rather than spread, so the original absolute path cannot ride
      // along into something that gets served.
      const rewritten: BlockOfKind<"image"> = { kind: "image", path: url };
      if (block.alt !== undefined) rewritten.alt = block.alt;
      if (block.caption !== undefined) rewritten.caption = block.caption;
      return rewritten;
    },
    needsLookingAt: () => true,
  },
};

export const CONTENT_KINDS = Object.keys(CONTENT_BLOCKS) as ContentKind[];

const UNKNOWN_BLOCK: ContentBehaviour = { needsLookingAt: () => true };

export function contentBehaviourFor(block: ContentBlock): ContentBehaviour {
  return (CONTENT_BLOCKS as Record<string, ContentBehaviour | undefined>)[block.kind] ?? UNKNOWN_BLOCK;
}

/* ----------------------------------------------------------------- sample */

/** Not exported: `SampleBlock` is sample.ts's name, and one owner is enough. */
type SampleBlock = BlockOfKind<"sample">;

/**
 * Everything a `sample` claims is checked here, against the rows themselves —
 * a rule naming a field no row carries, or a `>` over something that is not a
 * number, is a per-path error at parse time rather than a NaN on screen.
 */
function refineSample(
  block: SampleBlock,
  at: (...rest: Array<string | number>) => Array<string | number>,
  controlsById: Map<string, Control>,
  ctx: Ctx,
): void {
  const rule = parseSampleRule(block.rule);
  if (!rule) {
    ctx.addIssue({ code: "custom", path: at("rule"), message: `${RULE_SHAPE} — got "${block.rule}"` });
    return;
  }

  const control = controlsById.get(rule.control);
  if (!control) {
    const ids = [...controlsById.keys()];
    ctx.addIssue({
      code: "custom",
      path: at("rule"),
      message: `"${rule.control}" is not a declared control id (${ids.length ? ids.join(", ") : "this widget has no controls"}); the right-hand side of a rule is always a control`,
    });
  } else if (rule.ordering && control.type !== "slider" && control.type !== "number") {
    ctx.addIssue({
      code: "custom",
      path: at("rule"),
      message: `"${rule.op}" compares numbers, but "${rule.control}" is a ${control.type} — use == or != to match it as a string`,
    });
  }

  const rows = block.rows as Array<Record<string, unknown>>;
  const requireField = (field: string, where: string, numeric: boolean): void => {
    rows.forEach((row, r) => {
      const value = row[field];
      if (value === undefined) {
        ctx.addIssue({ code: "custom", path: at("rows", r), message: `no "${field}" on this row, but ${where} names it` });
        return;
      }
      if (numeric && !Number.isFinite(Number(value))) {
        ctx.addIssue({
          code: "custom",
          path: at("rows", r, field),
          message: `${where} needs a number here, got ${JSON.stringify(value)}`,
        });
      }
    });
  };

  requireField(rule.field, "`rule`", rule.ordering);
  // An explicit `field` is declared as the numeric column to sort by and show;
  // when it is omitted the rule's own field does that job and is already checked.
  if (block.field !== undefined && block.field !== rule.field) requireField(block.field, "`field`", true);
  if (block.total) requireField(block.total.field, "`total.field`", true);
}

/* ------------------------------------------------------------------ bind */

/**
 * Placeholder validation runs in ONE direction only: inside a bound block, a
 * `{{name}}` that is not a declared control is an error. Braces in an UNBOUND
 * block are left entirely alone — erroring on those would make a widget about a
 * Helm chart or a GitHub Action unauthorable.
 */
function checkBoundValue(
  value: string,
  path: Array<string | number>,
  controlsById: Map<string, Control>,
  ctx: Ctx,
): void {
  for (const placeholder of collectPlaceholders(value)) {
    if (placeholder.transform !== undefined && !isBindTransform(placeholder.transform)) {
      ctx.addIssue({
        code: "custom",
        path,
        message: `${placeholder.raw}: unknown transform ":${placeholder.transform}" (pascal, camel, kebab)`,
      });
      continue;
    }
    const control = controlsById.get(placeholder.id);
    if (!control) {
      const ids = [...controlsById.keys()];
      ctx.addIssue({
        code: "custom",
        path,
        message: `${placeholder.raw} names no control (declared: ${ids.length ? ids.join(", ") : "none"}). Drop \`bind\` if the braces are meant literally.`,
      });
      continue;
    }
    // Which types cannot be interpolated is the CONTROL's business, not this
    // file's — it used to be a hardcoded list here that a new list-valued type
    // would have quietly fallen through. The behaviour supplies the clause
    // after "is a <type>", so the composed sentence reads as one.
    const rejection = behaviourFor(control).bindRejection?.(control);
    if (rejection) {
      ctx.addIssue({ code: "custom", path, message: `${placeholder.raw} is a ${control.type}${rejection}` });
    }
  }
}
