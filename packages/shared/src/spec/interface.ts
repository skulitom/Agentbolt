import { z } from "zod";
import { idField, Control } from "./control-schemas.js";
import { behaviourFor } from "./control-registry.js";
import { ContentBlock, contentBehaviourFor } from "./content-blocks.js";
import { formatSpecError } from "./errors.js";
import { PREVIEW_VIEWS, PREVIEW_VIEW_NAMES, viewAccepts } from "../preview-plan.js";

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
  /**
   * Arrange the generated preview yourself, instead of letting it infer.
   *
   * This is the middle ground the vocabulary was missing. Before it there were
   * two options: accept whatever inference produced, or write an entire
   * `previewModule` — a hundred lines of DOM to say "put these two colours in a
   * gradient rather than two swatches". Declaring panels costs one line each and
   * still gets the animation policy, the keyed reconciliation and the range
   * handling for free.
   *
   * Declaring ANY view replaces inference completely rather than adding to it.
   * Merging would mean an author who wanted one thing moved could not stop the
   * inferred copy of it appearing as well.
   */
  views: z
    .array(
      z.strictObject({
        view: z.enum(PREVIEW_VIEW_NAMES as [string, ...string[]]),
        /** Control ids, in the order they should be drawn. */
        controls: z.array(z.string()).min(1),
        /** Overrides the view's own heading. */
        title: z.string().optional(),
      }),
    )
    .optional(),
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

    // A declared panel that names a control the view cannot draw would render
    // as a heading with nothing under it. Caught here so the author is told
    // which control and which view, rather than discovering an empty pane.
    spec.preview?.views?.forEach((declared, index) => {
      const view = PREVIEW_VIEWS[declared.view];
      const at = (...rest: Array<string | number>) => ["preview", "views", index, ...rest];

      declared.controls.forEach((id, c) => {
        const control = controlsById.get(id);
        if (!control) {
          const ids = [...controlsById.keys()];
          ctx.addIssue({
            code: "custom",
            path: at("controls", c),
            message: `"${id}" is not a declared control id (${ids.length ? ids.join(", ") : "this widget has no controls"})`,
          });
          return;
        }
        if (!viewAccepts(declared.view, control.type)) {
          const accepts = view?.accepts === "*" ? "any control" : (view?.accepts ?? []).join(", ");
          ctx.addIssue({
            code: "custom",
            path: at("controls", c),
            message: `the "${declared.view}" view draws ${accepts}, but "${id}" is a ${control.type}`,
          });
        }
      });

      const min = view?.min ?? 1;
      if (declared.controls.length < min) {
        ctx.addIssue({
          code: "custom",
          path: at("controls"),
          message: `the "${declared.view}" view needs at least ${min} control${min === 1 ? "" : "s"} to say anything`,
        });
      }
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
