import { z } from "zod";
import { idField, Control } from "./control-schemas.js";
import { behaviourFor } from "./control-registry.js";
import { ContentBlock, contentBehaviourFor } from "./content-blocks.js";
import { formatSpecError } from "./errors.js";

/**
 * The Interface Spec: the JSON an agent authors to describe a widget.
 *
 * The superRefine here is now only the checks that span the WHOLE spec —
 * duplicate ids, "a widget needs something in it", and the content/control
 * cross-references. Everything type-specific dispatches through the two
 * registries, so this function stops growing when the vocabulary does.
 */

/**
 * Action-bar buttons. When a spec declares actions, clicking one finalizes the
 * session and records which was chosen — this is how ui_ask's option buttons
 * are expressed without a second code path.
 */
export const Action = z.strictObject({
  id: idField,
  label: z.string().min(1),
  style: z.enum(["default", "primary", "danger"]).default("default"),
});

export type Action = z.infer<typeof Action>;

/**
 * Named rather than inline so `ui_ask` can accept the same object: a shorthand
 * question with a `sample` block and a slider that carries a unit needs to be
 * able to decline the generated frame, or it gets a bar redrawing what the
 * table already said.
 */
export const PreviewOptions = z.strictObject({
  height: z.number().int().min(120).max(1200).default(400),
  /**
   * Set false to decline the generated preview. For a widget whose values
   * genuinely have nothing to show, an empty frame is worse than none — and the
   * alternative workaround (dropping `unit`) would corrupt the data to fix the
   * presentation.
   */
  auto: z.boolean().default(true),
  /**
   * Where the frame sits relative to the controls.
   *
   * "right" is the default because a preview you cannot see while dragging is
   * not a preview: a wide single column wastes half the screen and pushes the
   * controls below the fold. Narrow viewports fall back to stacked via CSS
   * regardless. "top" is for a preview that is the point and the controls are
   * an afterthought.
   *
   * This lives on `preview` rather than in a `layout` object of its own —
   * there is exactly one layout decision in the whole spec, and its previous
   * home defaulted to "top" while the renderer defaulted to "right", so
   * declaring any unrelated layout field silently moved the frame.
   */
  position: z.enum(["top", "right"]).default("right"),
});

export const InterfaceSpec = z
  .strictObject({
    schemaVersion: z.literal(1).default(1),
    title: z.string().min(1),
    description: z.string().optional(),
    content: z.array(ContentBlock).default([]),
    controls: z.array(Control).default([]),
    actions: z.array(Action).default([]),
    /** Free-text box for the human. Defaults on — it is how you say "why". */
    comment: z
      .strictObject({
        enabled: z.boolean().default(true),
        placeholder: z.string().optional(),
        label: z.string().optional(),
      })
      .default({ enabled: true }),
    preview: PreviewOptions.optional(),
  })
  .superRefine((spec, ctx) => {
    if (spec.controls.length === 0 && spec.actions.length === 0 && spec.content.length === 0) {
      ctx.addIssue({
        code: "custom",
        path: [],
        message: "a widget needs at least one of: controls, actions, or content",
      });
    }

    const seen = new Map<string, number>();
    spec.controls.forEach((control, index) => {
      const first = seen.get(control.id);
      if (first !== undefined) {
        ctx.addIssue({
          code: "custom",
          path: ["controls", index, "id"],
          message: `duplicate control id "${control.id}" (already used by controls[${first}])`,
        });
      } else {
        seen.set(control.id, index);
      }

      // One dispatch, instead of an if-chain that grew a branch per type and
      // silently did nothing for a type nobody remembered to add.
      const at = (...rest: Array<string | number>) => ["controls", index, ...rest];
      behaviourFor(control).refine?.(control, at, ctx);
    });

    const actionIds = new Set<string>();
    spec.actions.forEach((action, index) => {
      if (actionIds.has(action.id)) {
        ctx.addIssue({
          code: "custom",
          path: ["actions", index, "id"],
          message: `duplicate action id "${action.id}"`,
        });
      }
      actionIds.add(action.id);
    });

    const controlsById = new Map(spec.controls.map((control) => [control.id, control]));
    spec.content.forEach((block, index) => {
      const at = (...rest: Array<string | number>) => ["content", index, ...rest];
      contentBehaviourFor(block).refine?.(block, at, controlsById, ctx);
    });
  });

/** What an agent writes (defaults still optional). */
export type InterfaceSpecInput = z.input<typeof InterfaceSpec>;
/** What the rest of the system sees (defaults filled in). */
export type InterfaceSpec = z.output<typeof InterfaceSpec>;

export interface ParsedSpec {
  ok: true;
  spec: InterfaceSpec;
}
export interface FailedSpec {
  ok: false;
  message: string;
}

export function parseSpec(input: unknown): ParsedSpec | FailedSpec {
  const result = InterfaceSpec.safeParse(input);
  if (result.success) return { ok: true, spec: result.data };
  return { ok: false, message: formatSpecError(result.error) };
}
