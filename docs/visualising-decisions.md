# BUILD PLAN — making non-visual questions visual in AgentUI

Grounded in a read of `packages/shared/src/{spec,auto-preview,presets,library,preview-protocol}.ts`, `packages/ui/src/{store.ts,components/*}`, `packages/ui/public/agentui/preview.js`, `packages/server/src/core/{session,session-manager,staleness}.ts`, `packages/server/src/web/preview.ts`, `packages/server/examples/usability-probe.mjs`.

Where the four proposals and their critiques disagreed, I re-checked the source and picked a side; those calls are marked **[call]**.

---

## 0. What ships, in order

| Phase | Content | New schema | Est. |
|---|---|---|---|
| **P0** | Repairs to the existing auto-preview + the predicate split + `preview.auto` | 1 boolean | ~1 day |
| **P1** | `sample` content block | 1 block kind | ~2 days |
| **P2** | `compare` control | 1 control type, value stays `string` | ~2.5 days |
| **P3** | `bind` on `markdown`/`code` | 1 boolean, 3 transforms | ~0.5 day |
| **P4** | `multiselect` control | 1 control type, **introduces `string[]`** | ~1.5 days |
| **P5** | SKILL.md rewrite | — | free, ships with P1 |

P0 is not optional and not a nicety: today a single `unit:"h"` slider renders a bar that is **100 % wide at every value** (`preview.js:156` seeds `maxByUnit` from the entries themselves, and with one entry `max === value`). The repo's own staleness probe hits this. Every proposal opened by citing the staleness ruler as decoration; the truth is worse than any of them said. Fix the lie before adding surface, or the new primitives inherit the credibility problem.

---

## 1. The rule

Replace the magnitude rule with one sentence the agent must be able to complete before it renders anything:

> **Changing `<control>` changes `<what the human sees>`, which I got from `<a file I read, a command I ran, or a formula I am showing>`.**

Three slots. All three must be fillable with something concrete, or you render nothing and ask in text.

| Slot fails | Symptom | Verdict |
|---|---|---|
| Slot 2 is empty — you drag and no pixel that encodes the value moves | the always-100 % ruler; `Position: 1,2,3`; five dots for five retries | **no visual** |
| Slot 2 is "the number, bigger" | a bar labelled with the number it is a bar of | **no visual** — it is chrome |
| Slot 3 is "I made it up so the picture would work" | plausible session names, `memorability: high`, invented LOC counts | **no visual** — a fabricated visual is worse than a sentence, because a sentence wears its uncertainty |

Two mechanical consequences the schema enforces rather than trusts:

- **Slot 3 is a required field.** Every `sample` block carries `source`, always rendered as a muted caption. An agent that has to type `"illustrative — I have not read your machine"` will usually go and read it instead, and a human who sees that string discounts correctly. This is the same move as `touched`: make the epistemically weak case visible rather than asking anyone to remember.
- **Slot 2 is validated where it can be.** `compare` rejects a spec where any option is missing any axis cell; `bind` rejects a placeholder naming an undeclared control; `sample` rejects a rule naming an undeclared control or a field absent from a row. All through `formatSpecError`'s per-path style, which is what makes an agent self-correct in one retry.

And the corollary that decides the hard half of the brief:

> **A yes/no policy question is almost always a threshold in disguise. Ask the threshold, over the real things the threshold applies to.**

That single reframe is what converts examples 1 and 5, which all four proposals struggled with as binaries.

---

## 2. The primitive set

Ranked by (examples converted) / (cost to build + cost to author). Four items. Everything else in the four proposals is cut in §4.

### P0 — repairs and the predicate split (no new primitives)

Not a primitive, but it is the cheapest value in the whole plan and P2/P4 are broken without the split.

1. **`ruler()` scale** — `autoPreview.update` (`preview.js:319-324`) passes only `{label, value, unit}` and drops the control's `min`/`max`. Pass them; scale the bar over `[min, max]` and label the ceiling. When a unit group has one entry *and* no declared bounds, render the number with **no bar at all**. (`preview.js:149-181`)
2. **`vector`** — `controlHasVisual` returns `true` (`auto-preview.ts:22`) but `autoPreview` has no vector view, and vectors are not excluded from `others` (`preview.js:280-282`), so a vector-only spec opens a whole preview pane containing the string `Position: 1,2,3`. Drop `vector` from the predicate until a view exists.
3. **`card()`** — retitle "In context" → **"Sample card"**, take the heading from `spec.title` instead of the hardcoded `"Approve the auth refactor?"` (`preview.js:225`), delete the `padding` parameter `autoPreview` never passes (`preview.js:238` vs `:326-330`). A card titled "In context" that is not the user's context is the exact failure this tool exists to prevent.
4. **`duration|delay` out of `SIZE_HINTS`** — a static bar for `400ms` is a category error; you cannot see a duration as a length. The regex is duplicated at `auto-preview.ts:16` (decides whether a frame exists) and `preview.js:256` (decides what goes in it); **edit both or you get a frame with nothing in it.** Add a test in `packages/shared/test` that reads `preview.js` and asserts the two literals are identical.
5. **Split the predicate.** `shouldAutoPreview` is `spec.controls.some(controlHasVisual)` (`auto-preview.ts:29-31`) and `groupControls` uses the same function at `SessionView.tsx:34`. The two callers want opposite answers for a self-rendering control:

```ts
/** Would the built-in auto-preview have something to draw? Gates the frame. */
export function isPreviewSubject(control: Control): boolean;   // today's controlHasVisual, minus vector

/** Does anything on screen change when this moves? Ordering only. */
export function controlIsIllustrated(spec: InterfaceSpec, control: Control): boolean;
//   isPreviewSubject(control)
// || control.type === "compare"
// || (control.type === "multiselect" && control.options.some(o => o.weight !== undefined))
// || referenced by a `sample` rule
// || referenced by a `{{placeholder}}` in a bound block

export function shouldAutoPreview(spec: InterfaceSpec): boolean {
  return spec.preview?.auto !== false && spec.controls.some(isPreviewSubject);
}
```

Keep `controlHasVisual` exported as an alias of `isPreviewSubject` so `packages/shared/test/auto-preview.test.ts` stays green. Without this split, adding "referenced by an effect ⇒ visual" (which the *consequence* proposal wanted) would flip `shouldAutoPreview` too, and example 10 would ship a half-screen preview pane whose entire content is `Retries: 3`.

6. **`preview: { auto: false }`** — two lines in `spec.ts:158-162`, one at `session.ts:287`. Today an agent has no way to decline a frame; the only workaround is deleting `unit`, which `SKILL.md:133` correctly forbids. A boolean is the one schema addition a model cannot author wrong.

7. **`quickChoicesFor`** (`session-manager.ts:80`) treats every markdown block as skippable prose. A *bound* markdown block is something to look at. One line: `block.kind !== "markdown" || block.bind`.

---

### P1 — `sample`: a rule replayed against real rows

**Converts: 1, 2, 5, 10. Partially 3.** Highest coverage per line of schema, and it is the only primitive that structurally cannot lie about scale, because it has no scale — it has rows the reader recognises.

```ts
z.strictObject({
  kind: z.literal("sample"),
  label: z.string().min(1),
  /** Required. A path, a command, or the formula. This is the provenance gate. */
  source: z.string().min(1),
  /** Exactly "<row_field> <op> <control_id>". Not an expression language. */
  rule: z.string(),
  /** [when the rule is true, when it is false]. */
  verdict: z.tuple([z.string(), z.string()]).optional(),
  /** Numeric field to sort by and display. Defaults to the rule's field. */
  field: z.string().optional(),
  unit: z.string().optional(),
  /** Live sum over matching rows. */
  total: z.strictObject({ field: z.string(), unit: z.string().optional(), label: z.string().optional() }).optional(),
  rows: z.array(z.looseObject({ label: z.string(), note: z.string().optional() })).min(2).max(60),
})
```

**Rule grammar — one regex, no parser, no precedence table:**

```
/^([a-z][a-z0-9_]*)\s*(>=|<=|==|!=|>|<)\s*([a-z][a-z0-9_]*)$/
   ^ always a row field        ^ op        ^ always a control id
```

The left side is *always* a row field and the right side is *always* a control id. That construction eliminates by design the shadowing hazard the *consequence* critique found in `{...values, ...row}` — there is no merged scope. `==`/`!=` compare as strings (so a `select` value can be matched against a row's classification); `> >= < <=` require numbers on both sides, checked **at spec-parse time against every row**, so a non-numeric field is a per-path error, not a `NaN` on screen.

**Rendering (shell, not the preview frame):**

- **Ordering ops** — sort by `field`, draw a divider labelled with the live control value at the crossover, print `verdict[0]`/`verdict[1]` per row (defaults `"matches"` / `"—"`), footer `"2 of 4 marked stale"` plus the `total` if present.
- **Equality ops** — no sort, highlight the matching row(s), footer names it. This is the lookup-table mode that converts example 10 with zero arithmetic in the renderer: the agent precomputes the consequence per candidate value and shows its formula in `source`.
- `source` always rendered as a muted caption under `label`.

**Why it is free at runtime:** it reads local `values`, which `setLocalValue` updates synchronously (`store.ts:141`) while wire frames are separately throttled at 100 ms (`store.ts:52`). Live at 60 fps, zero extra wire traffic, and no path to waking a waiter. `ContentRenderer` takes one new prop (`SessionView.tsx:71`).

**Two authoring rules for the docs, not the schema:** rows must be things the human recognises; and if `rows.length < 3` you have a sentence, not a table.

---

### P2 — `compare`: a select whose options carry a column

**Converts: 3, 5, 7. Partially 1, 6, 8.** Broadest coverage of the four; the most expensive renderer.

```ts
export const CompareControl = z.strictObject({
  ...controlBase,
  type: z.literal("compare"),
  axes: z.array(z.strictObject({
    id: idField,
    label: z.string().min(1),
    kind: z.enum(["text", "bool", "code", "scale"]).default("text"),
    scale: z.array(z.string()).optional(),      // required when kind === "scale"
    better: z.enum(["low", "high", "none"]).default("none"),
    template: z.string().optional(),            // "{}" ← option value; cells generated
  })).min(2, "a compare with one axis is a select — use that instead"),
  options: z.array(z.strictObject({
    value: z.string(),
    label: z.string().optional(),
    summary: z.string().optional(),
    cells: z.record(z.string(), z.union([z.string(), z.boolean()])).default({}),
  })).min(2),
  default: z.string(),
});
```

**Value is `options[].value` — a plain string.** No change to `ControlValue`, `normalizeValues`, `defaultsFromSpec`, `state.json`, or the ws protocol; `validateValuesAgainstSpec`'s `select` arm (`spec.ts:372-381`) is reused verbatim. That is the entire cost argument, and it is real.

**Validation (this is where the value is):** every option must supply every non-template axis id → `controls[2].options[1].cells: missing cell for axis "offline"`. That check is the forced-parallelism the proposals were reaching for. Prose lets an agent write two sentences about approach A's performance and none about B's; a matrix has a hole where B's cell goes, and the validator refuses to ship the hole. Also: unknown axis id in `cells`, `kind:"scale"` without `scale`, cell not in the declared scale, boolean cell on a non-`bool` axis, duplicate axis ids, default not among option values.

**`better: "none"` is the default and is load-bearing.** The tool must be able to say "these differ and neither is better" rather than implying a winner it has not earned. Only `low`/`high` colour a scale axis.

**`axes[].template`** is `template.replaceAll("{}", option.value)`, rendered as `code`. It is the one piece of sugar that *removes* authoring error rather than adding it: four templates instead of sixteen hand-written cells, and the cells cannot drift out of parallel. It is what makes naming-as-comparison nearly free.

**Cut from every proposal's version:** `number` cells (the only kind needing cross-cell scaling maths — `scale` with `S/M/L` is what agents actually write for effort and risk); nested `ContentBlock[]` per option (§4); positional-array shorthand (§4).

**Layout:** `.controls` is `flex-direction: column` (`styles.css:183`), so a `compare` already takes the full main column. With `isPreviewSubject(compare) === false` no side panel spawns, so full width falls out of the predicate split. One media query below ~720 px stacks the matrix into one card per option. This is not the renderer risk the *comparison* proposal claimed.

**Also update:** `FieldShorthand`'s type enum (`presets.ts:25`) — it is a `z.enum`, so a new control type is rejected by `ui_ask` shorthand until listed there. Easy to miss; it bites both P2 and P4.

---

### P3 — `bind`: the string, in the places it will appear

**Converts: 6, 8.** Only two examples, but it is ~40 lines total and it converts `text` — the one control the system explicitly refuses to preview (`auto-preview.ts:26`, and `preview.js:281` filters it out of even the fallback state line) — into the most previewable one. Best ratio in the plan.

```ts
z.strictObject({ kind: z.literal("markdown"), value: z.string(), bind: z.boolean().default(false) }),
z.strictObject({ kind: z.literal("code"), value: z.string(), language: z.string().optional(),
                 filename: z.string().optional(), bind: z.boolean().default(false) }),
```

Substitution happens in the **shell**, in `ContentRenderer`, before markdown parsing and before `DOMPurify` (`ContentRenderer.tsx:48`). `code` and `diff` are already `textContent`.

- `{{id}}` where `id` matches `^[a-z][a-z0-9_]*$` **and is a declared control id** → the value.
- `{{id:pascal}}` / `:camel` / `:kebab`. Three transforms, a `switch`, no arguments, no chaining.
- **Everything else inside braces is left byte-for-byte alone.** `${{ github.token }}`, `{{#each}}`, `{{ .Values.image }}` all have dots, spaces or sigils and survive. This is why there is no escape syntax — the single biggest source of "nobody authors it correctly" in templating systems.

Value → text: `string` → itself; `number` → the bare number (write the unit in the template: `timeout: {{ms}}ms`); `color` → the hex; `select` → **the value, not the label** (documented, and deliberate — it is what lets option values *be* the alternative snippets).

**Validation, one direction only:** inside a **bound** block, a placeholder naming an undeclared control is an error listing the valid ids. Do **not** error on `{{...}}` in unbound blocks — that makes a widget about a Helm chart or a GitHub Action unauthorable. Plus two refusals with teaching messages: `{{toggle_id}}` (→ "use a select whose option *values* are the alternative lines") and `{{multiselect_id}}` (→ "use a `sample` block").

**Blocks stay in the content flow.** No relocation into the preview slot. `code`/`markdown` are rendered by React with the shell's CSS, `DOMPurify` and `parseUnifiedDiff`; moving them across the origin boundary means reimplementing all of that inside `preview.js`. The scroll objection is overstated — the naming widget has one text input about fifteen lines down.

---

### P4 — `multiselect`: the genuinely missing control

**Converts: 9. Partially 3.** Thin coverage on the ten, but it closes a real gap in `Control` (`spec.ts:93`) and serves a permanent family: which files, which tests to run, which clients to install for, which lint rules, which packages to publish.

```ts
export const MultiSelectControl = z.strictObject({
  ...controlBase,
  type: z.literal("multiselect"),
  options: z.array(z.strictObject({
    value: z.string(), label: z.string().optional(),
    note: z.string().optional(), weight: z.number().optional(),
  })).min(2),
  weightUnit: z.string().optional(),
  default: z.array(z.string()),
});
```

**Cost it honestly: this is the release that introduces `string[]`.** `ControlValue` (`spec.ts:274`) is `string | number | boolean | number[]`. Touching:
`ControlValue` · a new arm in `validateValuesAgainstSpec` (array of strings, each a declared option value, no duplicates) · `defaultsFromSpec` (clone, like `vector`) · `normalizeValues` (`spec.ts:411` — the `type === "vector" ? [...] : incoming` clone needs the new case) · `ControlRenderer` · `presets.ts:25` enum · `inferType` (`presets.ts:166` currently maps any array to `vector`; split on element type). `state.json` is `z.record(z.string(), z.unknown())`, so persistence is fine.

**[call]** `weightUnit` stays on the control, against the *comparison* critique. Its objection was "two shapes for the same concept" — but I dropped `number` cells from `compare`, so there is no second shape, and one unit on the control is what the totals row needs.

Renders as a checkbox list with `note` muted and `weight` in mono; footer `"2 of 3 files · 420 lines"` recomputed live. With no weights it renders as plain checkboxes and says `"2 of 3 selected"` — a form, not a chart, and the docs must not oversell it. That is honest degradation. **No path-tree heuristic** (§4).

---

## 3. The ten, converted

### 1. "Should `ui_close` also delete the session directory by default?"

The binary is a threshold in disguise. Ask the threshold; keep buttons for the endpoints.

```json
{
  "title": "Should ui_close delete the session directory?",
  "content": [
    { "kind": "markdown", "value": "**Should `ui_close` delete the session directory?** \"Always\" and \"never\" are the ends of one dial: *delete when the session is older than N days*. Below is what is actually on this machine." },
    { "kind": "sample",
      "label": "Session directories on this machine",
      "source": "du -sk ~/.agentui/sessions/* && stat -c %Y — run just now, 14 dirs",
      "rule": "age_d > keep_days",
      "verdict": ["deleted on close", "kept"],
      "field": "age_d", "unit": "d",
      "total": { "field": "kb", "unit": " kB", "label": "reclaimed" },
      "rows": [
        { "label": "muzzle-flash",      "age_d": 0.01, "kb": 96,   "note": "you adjusted this 20 min ago" },
        { "label": "theme-preview",     "age_d": 0.2,  "kb": 140 },
        { "label": "changelog approval","age_d": 1.1,  "kb": 88 },
        { "label": "three.js demo",     "age_d": 8,    "kb": 4180, "note": "holds assets/shot.png" },
        { "label": "usability probe",   "age_d": 23,   "kb": 72 }
      ] }
  ],
  "controls": [
    { "id": "keep_days", "type": "slider", "label": "Keep for", "min": 0, "max": 30, "step": 1, "default": 7, "unit": "d" }
  ],
  "actions": [
    { "id": "use_ttl",  "label": "Use this TTL", "style": "primary" },
    { "id": "never",    "label": "Never delete" },
    { "id": "always",   "label": "Always delete on close" }
  ],
  "preview": { "auto": false }
}
```

`keep_days` carries `unit: "d"`, so without `preview.auto:false` this would spawn a frame drawing a bar next to a table that already says everything. That is exactly why P0 item 6 exists.

The decisive fact here is irreversibility, and no primitive draws that — it goes in the markdown, one sentence: "`ui_get_state` returns nothing afterwards; there is no undo."

### 2. "How long before a session counts as stale?"

The flagship. Today `usability-probe.mjs:59` asks this with `{min:1, max:48, default:6, unit:"h"}`, and the human gets a bar that is 100 % wide at 1 h and 100 % wide at 48 h.

```json
{
  "title": "When does a session stop counting as \"waiting for you\"?",
  "content": [
    { "kind": "markdown", "value": "**How long before a session counts as stale?** Staleness only changes presentation — a stale session is still open and still answerable (`core/staleness.ts`). Drag until the line falls where you'd want it." },
    { "kind": "sample",
      "label": "Your open sessions, by time since last touch",
      "source": "~/.agentui/sessions/*/state.json — updatedAt, read just now (11 open)",
      "rule": "idle_h > stale_hours",
      "verdict": ["marked stale", "stays listed"],
      "field": "idle_h", "unit": "h",
      "rows": [
        { "label": "muzzle-flash",         "idle_h": 0.33, "note": "you were dragging this before lunch" },
        { "label": "theme-preview",        "idle_h": 5.2 },
        { "label": "shader params",        "idle_h": 6.8 },
        { "label": "changelog approval",   "idle_h": 27 },
        { "label": "last week's three.js demo", "idle_h": 190 }
      ] }
  ],
  "controls": [
    { "id": "stale_hours", "type": "slider", "label": "Stale after", "min": 1, "max": 48, "step": 1, "default": 4, "unit": "h" }
  ],
  "preview": { "auto": false }
}
```

You drag to 5, watch `theme-preview` drop below the line, and drag back to 8. That is not in the number 4, and `staleness.ts`'s own comment — "long enough to outlast lunch and a long meeting" — is evidence that nobody ever arrived at this number by looking at a slider.

### 3. "Which of these four improvements should I build first?"

**No rank control.** "First" is a single choice, and asking again next time costs one line. `compare`, with axes that are facts.

```json
{ "id": "first", "type": "compare", "label": "Build first",
  "axes": [
    { "id": "covers",  "label": "Questions it converts", "kind": "text" },
    { "id": "cost",    "label": "Est. build",  "kind": "scale", "scale": ["S", "M", "L"], "better": "low" },
    { "id": "schema",  "label": "New schema",  "kind": "text" },
    { "id": "blocked", "label": "Blocks anything else?", "kind": "bool", "better": "low" }
  ],
  "options": [
    { "value": "sample",   "label": "sample block",
      "cells": { "covers": "1, 2, 5, 10", "cost": "M", "schema": "1 block kind", "blocked": false } },
    { "value": "compare",  "label": "compare control",
      "cells": { "covers": "3, 5, 7", "cost": "M", "schema": "1 control, value stays string", "blocked": false } },
    { "value": "bind",     "label": "bind on content",
      "cells": { "covers": "6, 8", "cost": "S", "schema": "1 boolean", "blocked": false } },
    { "value": "multi",    "label": "multiselect",
      "cells": { "covers": "9", "cost": "M", "schema": "ControlValue gains string[]", "blocked": true } }
  ],
  "default": "sample" }
```

Every cell is checkable — the human can say "no, that's wrong" about "1 block kind" or "blocks anything else". None of them is `elegance: high`.

**If you do not have real effort numbers, this fails slot 3 and goes to chat.**

### 4. "Should non-previewable controls be grouped separately?" — no new primitive

Two `image` blocks of **screenshots you actually took**, plus buttons. That works today.

```json
{ "content": [
    { "kind": "markdown", "value": "**Should controls the preview can't illustrate be grouped separately?** Same widget, two arrangements — screenshots taken just now at 1280×800." },
    { "kind": "image", "path": "/abs/path/grouped-sunk.png",     "caption": "A — today: whole groups with nothing visual sink below the ones that do (groupControls visualFirst)" },
    { "kind": "image", "path": "/abs/path/grouped-labelled.png", "caption": "B — separate labelled section, \"No live preview\"" }
  ],
  "actions": [{ "id": "a", "label": "Keep A", "style": "primary" }, { "id": "b", "label": "Switch to B" }] }
```

**The test that produced this answer.** The tempting version is a bound `html` block that re-renders a mock panel as you pick. It fails on two counts. Slot 3: I would be hand-drawing a mock of `ControlRenderer.tsx` and asking you to judge it as if it were the real panel — the fidelity divergence the whole plan is written to prevent. And mechanically: `html` blocks do not render at all (`ContentRenderer.tsx:39-41` returns the literal string "HTML content renders in the preview frame", `(wired in P4)`), and the preview frame is a separate origin whose import map admits only `three` (`preview-protocol.ts:29`) — it cannot reach the shell's React renderer.

**No screenshots ⇒ this goes to chat.** And note the policy already shipped (`spec.ts:294-318`, tested at `auto-preview.test.ts:63-91`) — the honest ask is "here are both, which?", not "imagine both".

### 5. "Global or project library scope?"

The rule replayed against the library index. `SavedWidget` already carries `usedInProjects: string[]`, `uses` and `lastUsedAt` (`library.ts:39-41`), so this is a filter, not filesystem archaeology.

```json
{
  "title": "Where should ui_save default to?",
  "content": [
    { "kind": "markdown", "value": "**Which scope should saving default to?** `core/library.ts` argues global — \"a widget built in one repo should be available in the next\". Here is that claim replayed against what you actually saved." },
    { "kind": "sample",
      "label": "Your saved widgets, and where they were reused",
      "source": "~/.agentui/library/index.json + .agent-ui/library/index.json — usedInProjects, read just now",
      "rule": "needs != scope",
      "verdict": ["would have been rebuilt from scratch", "found on reuse"],
      "rows": [
        { "label": "muzzle-flash",  "needs": "global",  "note": "saved in AgentUI, reused in rasterizer (3×)" },
        { "label": "theme-probe",   "needs": "project", "note": "saved and reused in AgentUI only" },
        { "label": "shader-params", "needs": "global",  "note": "saved in shaderbench, reused in rasterizer" },
        { "label": "diff-approve",  "needs": "global",  "note": "reused in 4 repos" }
      ] }
  ],
  "controls": [
    { "id": "scope", "type": "select", "label": "ui_save defaults to",
      "options": [{ "value": "global", "label": "~/.agentui/library — every project" },
                  { "value": "project", "label": ".agent-ui/library — this repo, travels if committed" }],
      "default": "global" }
  ],
  "actions": [{ "id": "use", "label": "Use this default", "style": "primary" }]
}
```

`3 of 4 would have been rebuilt from scratch` is unanswerable in a way that "global is more convenient" is not. This is the pattern the whole plan is built around, and `source` is what keeps it from becoming fiction.

### 6. "What should this package be called?"

```json
{
  "title": "Name for the MCP server package",
  "content": [
    { "kind": "markdown", "value": "**What should this package be called?** Type anything — the four places it actually appears update as you go. Current candidates: `agentui`, `agentui-mcp`, `ask-human`." },
    { "kind": "code", "bind": true, "language": "ts", "filename": "packages/server/src/mcp.ts",
      "value": "import { uiAsk, uiWait } from \"{{pkg}}\";\nconst client = new {{pkg:pascal}}Client();" },
    { "kind": "code", "bind": true, "language": "bash",
      "value": "npx {{pkg}} install --client claude-code\n{{pkg}} --version\n{{pkg}} doctor" },
    { "kind": "code", "bind": true, "language": "json", "filename": ".mcp.json",
      "value": "{\n  \"mcpServers\": {\n    \"{{pkg}}\": {\n      \"command\": \"npx\",\n      \"args\": [\"-y\", \"{{pkg}}@latest\", \"serve\"]\n    }\n  }\n}" },
    { "kind": "code", "bind": true,
      "value": "packages/\n  {{pkg}}/\n  {{pkg}}-ui/\n  {{pkg}}-shared/\nnode_modules/.bin/{{pkg}}" }
  ],
  "controls": [
    { "id": "pkg", "type": "text", "label": "Package name", "default": "agentui-mcp",
      "description": "the npm name, the binary, and the key in .mcp.json" }
  ],
  "actions": [{ "id": "use_it", "label": "Use this name", "style": "primary" }]
}
```

`agentui-mcp` reads fine as an npm name and terribly as a repeated CLI verb, and `{{pkg:pascal}}` → `AgentuiMcpClient` is ugly. Neither is visible in a list of three strings.

`default` is the recommendation, so `touched`/`untouched` tells you whether the name was accepted or replaced.

**`bind` over `compare` here [call]**, against the *comparison* proposal: the answer to a naming question is very often a fourth name nobody listed, and only a text control can receive it. Use `compare` + `template` when the shortlist is genuinely closed.

### 7. "Which of these three approaches?"

```json
{ "id": "approach", "type": "compare", "label": "How should the preview frame get its data?",
  "axes": [
    { "id": "files",   "label": "Files touched", "kind": "text" },
    { "id": "wire",    "label": "New protocol message", "kind": "bool", "better": "low" },
    { "id": "offline", "label": "Works with no network", "kind": "bool", "better": "high" },
    { "id": "risk",    "label": "Blast radius if wrong", "kind": "scale", "scale": ["one widget", "all previews", "the wake path"], "better": "low" },
    { "id": "call",    "label": "What the module writes", "kind": "code" }
  ],
  "options": [
    { "value": "poll",  "label": "Poll from the frame", "summary": "the module fetches its own data",
      "cells": { "files": "preview.ts, preview.js", "wire": false, "offline": false, "risk": "all previews",
                 "call": "await fetch('/s/'+id+'/rows')" } },
    { "value": "push",  "label": "Push in `init`", "summary": "rows ride along with the spec",
      "cells": { "files": "preview-protocol.ts, PreviewFrame.tsx, session.ts", "wire": true, "offline": true, "risk": "all previews",
                 "call": "setup({ spec, values, data })" } },
    { "value": "inline","label": "Render in the shell", "summary": "no frame involved",
      "cells": { "files": "ContentRenderer.tsx", "wire": false, "offline": true, "risk": "one widget",
                 "call": "// nothing — it is a content block" } }
  ],
  "default": "inline" }
```

Every axis is checkable. The one that would not be — `maintainability: medium` — is exactly what the validator cannot catch and the docs must forbid.

### 8. "Is this error message clear?"

```json
{
  "title": "Wording for the install-conflict error",
  "content": [
    { "kind": "markdown", "value": "**Is this clear?** What prints when `agentui install` finds an existing entry. Edit the ✗ line; everything around it is the real output from `install/install.ts`." },
    { "kind": "code", "bind": true, "language": "text", "filename": "terminal, 80 cols",
      "value": "$ agentui install --client cursor\n  ok  found Cursor at ~/.cursor/mcp.json\n  x   {{message}}\n{{hint}}\n\n  1 of 2 clients configured. Run `agentui doctor` to check." }
  ],
  "controls": [
    { "id": "message", "type": "text", "multiline": true, "label": "The error line",
      "default": "an \"agentui\" server is already registered — refusing to overwrite it" },
    { "id": "hint", "type": "select", "label": "Follow-up line",
      "options": [
        { "value": "", "label": "no follow-up" },
        { "value": "      Re-run with --force to replace it.", "label": "suggest --force" },
        { "value": "      It points at /old/path/mcp.js — re-run with --force to replace.", "label": "suggest --force, and say what's there" }
      ],
      "default": "      Re-run with --force to replace it." }
  ],
  "actions": [{ "id": "ship", "label": "Use this", "style": "primary" }, { "id": "neither", "label": "Neither — I'll say why" }]
}
```

The `select` whose **option values are the alternative snippets** is the single most useful pattern here: it is conditionality with no conditionals, and it is why `bind` never needs `{{#if}}`. Note the message is *editable* — the answer to "is this clear?" is usually "here's a better one".

**No ANSI.** Hand-authored `\u001b[31m` inside a JSON string is a coin flip, and a wrong colour is worse than no colour when the entire claimed value is "you can see it sits dim-red between a green tick and a normal line". Plain `ok`/`x` at real width carries the wrap, which is the part that matters.

### 9. "Which files should this refactor touch?"

```json
{
  "title": "Scope of the waiters refactor",
  "content": [
    { "kind": "markdown", "value": "**Which files should this touch?** Checked files move to the new `WaiterSet` API in one commit; unchecked ones keep calling the old helpers. Line counts are from `wc -l`, call sites from ripgrep." }
  ],
  "controls": [
    { "id": "files", "type": "multiselect", "label": "Files", "weightUnit": "lines",
      "options": [
        { "value": "core/waiters.ts",         "note": "the extraction itself",        "weight": 180 },
        { "value": "core/session.ts",         "note": "3 call sites",                 "weight": 240 },
        { "value": "core/session-manager.ts", "note": "1 call site",                  "weight": 96 },
        { "value": "mcp/tools.ts",            "note": "ui_wait entry point",          "weight": 210 },
        { "value": "mcp/heartbeat.ts",        "note": "only if we unify the timeout", "weight": 64 },
        { "value": "test/async.test.ts",      "note": "needs rewriting either way",   "weight": 310 }
      ],
      "default": ["core/waiters.ts", "core/session.ts", "mcp/tools.ts"] }
  ],
  "actions": [{ "id": "go", "label": "This scope", "style": "primary" }]
}
```

Two things prose structurally cannot do: the total recomputes as you check, and the human is **editing a proposal** rather than authoring a list — which is what `touched` was built to record.

### 10. "What retry count / batch size / concurrency?"

`sample` in lookup mode. The agent precomputes the consequence for each candidate value and puts the formula in `source`, so a wrong formula is catchable — no expression evaluator, no templating syntax, nothing that can silently disagree with its own prose.

```json
{
  "title": "Retry budget for the MCP fetch",
  "content": [
    { "kind": "markdown", "value": "**How many retries?** The number alone doesn't say much; what matters is how long we stall before giving up, against the 60 s tool timeout." },
    { "kind": "sample",
      "label": "What each retry count costs, worst case",
      "source": "computed: sum of 500ms * 2^k for k in 0..n  (exponential backoff, no jitter)",
      "rule": "n == retries",
      "field": "stall_s", "unit": "s",
      "rows": [
        { "n": 0, "label": "no retry",   "stall_s": 0,     "note": "one request; a blip fails the call" },
        { "n": 1, "label": "1 retry",    "stall_s": 0.5 },
        { "n": 2, "label": "2 retries",  "stall_s": 1.5 },
        { "n": 3, "label": "3 retries",  "stall_s": 3.5,   "note": "covers a restart of the far side" },
        { "n": 4, "label": "4 retries",  "stall_s": 7.5 },
        { "n": 5, "label": "5 retries",  "stall_s": 15.5 },
        { "n": 6, "label": "6 retries",  "stall_s": 31.5 },
        { "n": 7, "label": "7 retries",  "stall_s": 63.5,  "note": "past the 60 s tool timeout — the agent gives up first and you get a confusing error instead of a retry" }
      ] }
  ],
  "controls": [
    { "id": "retries", "type": "slider", "label": "Retries", "min": 0, "max": 7, "step": 1, "default": 3 }
  ]
}
```

`retries` has no `unit` and no size-hint match, so it gets no frame today — honest, and useless. Here the arithmetic the human will not do is done, shown, and falsifiable. That row-7 note is the entire value of the widget: it converts "7 is a bigger number" into "7 is a bug".

### Scoreboard, honestly

| | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
|---|---|---|---|---|---|---|---|---|---|---|
| | sample | sample | compare | images (or chat) | sample | bind | compare | bind | multiselect | sample |

Nine of ten get a real visual; one gets screenshots and no new primitive. Every one of them is gated on the agent having gone and got the rows first — and that behaviour change ships for free, in the docs, before any of this code lands.

---

## 4. What NOT to build

**An expression language / templating arithmetic** (`expr: "base_ms * (2^(retries+1) - 1) / 1000"`, `{{n+1}}`, `warn_above`). It sounds like the cheapest thing in the room and it is not. Priced honestly it needs: a Pratt parser, did-you-mean id validation, defined behaviour for vector operands (`size * 2` where `size` is `[3,4]`), select-value string comparison, NaN and div-by-zero display, and — the real trap — a defined precedence between row fields and control ids when both are in scope, because a row field silently shadowing a control id makes `idle_h > stale_hours` compare the wrong thing with **no error**. Worse, a wrong formula is unfalsifiable in exactly the way this tool exists to prevent: the *skeptic* proposal's own worked example, "3 retries × 2s backoff = up to 14s stall", is only true under exponential backoff — linear gives 6 s, and the reader cannot tell. `sample`'s lookup mode gets the same outcome by making the agent do the arithmetic and print the formula. `rule` is one regex, and its left/right asymmetry makes the shadowing bug unrepresentable.

**`when` on content blocks** (show a different diff per selected option). Its flagship use — a real diff per approach — requires the agent to have *written* all N branches. It hasn't; that is why it is asking. So the strong case fails slot 3, and the weak case (a five-line excerpt per option) is already a `code` cell in `compare`. The follow-on argument about collapse-to-first-line is worse: it has no defined meaning for `image`, `diff` or `html`, so "reuses every existing renderer" is false for three of five kinds.

**A `rank` control with drag-and-drop.** Covers exactly one of the ten. It is the most expensive renderer in any proposal (pointer capture, touch, autoscroll, plus a keyboard fallback you must build anyway for accessibility), it shares `multiselect`'s `string[]` cost, and its own advocates concede that a rank with no weights should not exist and that intra-sprint ordering rarely matters. "Which first?" is a `compare`, and asking again next time costs one line. Revisit if somebody asks for a full permutation twice.

**`bind` on `html` blocks — "a preview module with no JavaScript".** The most seductive idea in the four documents and the one that would burn the most time. Factually: `html` blocks do not render at all today (`ContentRenderer.tsx:39-41`), the `values` stream goes to *one* iframe hosting *one* ES module (`PreviewFrame.tsx:83`), there is one `preview.height`, and `hasPreview = #authoredPreview || shouldAutoPreview(spec)` (`session.ts:85-87`) means a spec with both a `previewModule` and bound html has two claimants for one slot. The "~20 lines" estimate is off by a large multiple. And design-wise it is the one thing an LLM authors wrong most often: the worked example is a 700-character string whose CSS attribute selectors (`[data-m='sink']`) must stay in sync with a select's option values. Rename an option and the panel silently renders the wrong arrangement. No validator catches that, and the failure looks fine.

**Nested `ContentBlock[]` inside `compare` options.** Sold as the highest-leverage reuse; it is the most broken part. `session-manager.ts:210` guards asset rewriting with `spec.content.some(block => block.kind === "image" && block.path)` and only walks the **top level**, so a nested image never gets copied into `assets/` and the browser gets a raw `C:\...` path it refuses. `html` is dead. So "zero new rendering code" is true for `markdown`, `code` and `diff` only — and `code` is already a cell kind. If diff-per-option turns up for real, add `{kind:"diff"}` as a *cell kind*. Do not open the content union inside a control.

**`ui_ask` shorthand with positional cell arrays.** The defence gives it away: "a cell-count mismatch is a clean per-option validation error." A shorthand whose failure mode is a validation error on every retry is not a shorthand. Positional arrays force the model to hold column order in working memory across N rows while inventing the content — precisely what `spec.ts`'s own header rule #1 exists to avoid. Keyed cells are self-checking and cost one line more. `FieldShorthand` is a `z.looseObject` and `inflateField` spreads `...raw`, so `fields: [{ id, type: "compare", axes, options }]` flows through nearly unchanged once the type is in the enum at `presets.ts:25`.

**`language: "ansi"` with escape-code rendering.** See example 8.

**A path-tree rendering heuristic on `multiselect`.** Single-purpose dressed as general: it serves one question, needs a "looks like a path" detector that will misfire on `a.b.c` and package names, and hoisting the common prefix rewrites what the labels say. Ship the checkbox list.

**Shipping `touched` into the preview frame.** Real gap, wrong size. The shell already labels untouched controls "suggested" with a one-click revert (`ControlRenderer.tsx:40-52`), six inches from the frame; agent-authored modules would not honour the flag anyway. A protocol change plus every helper updated, for a duplicate label.

**Radar charts, 2×2 matrices, force-directed dependency graphs.** Name them in the docs as forbidden. Radar area depends on axis *order*, the axes are incommensurable, and the coordinates are agent guesses rendered as measurement — the purest form of slot-3 failure. In a force layout, node position carries no data at all.

**Self-demonstrating widgets** (wire a control to the shell's own `layout`/`groupControls`). Cute, cheap, and only works for questions about AgentUI itself. If you want it, it belongs in `packages/server/examples/`, not in the schema, where its presence invites generalisation.

---

## 5. SKILL.md guidance

Replaces the "Give them something to look at" section (`SKILL.md:96-123`) and the last bullet of the authoring checklist.

````markdown
## Give them something to look at

A control whose effect you cannot see is worse than a text question: they guess,
and the UI looks authoritative while they do it.

**Before you render anything, finish this sentence:**

> Changing `<control>` changes `<what they see>`, which I got from `<a file I
> read, a command I ran, or a formula I am showing>`.

If you cannot fill all three blanks, **render nothing** and ask in text. In
particular:

- **Nothing moves when you drag it?** No visual. A bar labelled with the number
  it is a bar of is decoration.
- **You made the numbers up so the picture would work?** No visual. A fabricated
  chart is worse than a sentence, because a sentence wears its uncertainty.

**Go and get the fact first.** Most questions that "have nothing to see" are
questions where nobody has looked yet. Before asking about a threshold, list the
things the threshold applies to and paste them in. `ls`, `du`, `wc -l`, the
library index — one command turns an unanswerable slider into an obvious one.

### Question shape → treatment

| The question is | Do this | Never |
|---|---|---|
| A threshold over things that exist (timeouts, staleness, limits, quotas) | `sample` block, `rule: "field > control"`, rows you just read, plus the slider | Draw a bar for the number |
| An abstract count (retries, batch size, concurrency) | Compute the consequence of each candidate value; `sample` with `rule: "n == control"` and the formula in `source` | Invent a scale for it |
| N approaches with tradeoffs | `compare` control; axes are facts the human could call wrong | Axes like "elegance", "maintainability" |
| Yes/no policy | Ask the threshold behind it, over the real things. Truly binary? Evidence in a `code` block + two buttons | A two-column table of adjectives |
| Structural choice (where things live, which default) | `sample` — replay the rule against what actually happened | A Venn diagram with invented areas |
| A name, a message, any string | `bind: true` code blocks showing the real places it appears + a `text` control | A list of candidates |
| A subset of a known set | `multiselect`, `weight` from something you measured | Prose list of filenames |
| Layout or rendering | Screenshot both, two `image` blocks | Hand-draw a mock of the thing you're asking about |
| Ordering / "what first?" | `compare` on the candidates. Ask again next time | Build a ranking |
| Open-ended, generative, "what am I missing?" | Ask in chat | Any widget |

### sample — a rule replayed against real rows

```json
{ "kind": "sample",
  "label": "Your open sessions, by time since last touch",
  "source": "~/.agentui/sessions/*/state.json — read just now",
  "rule": "idle_h > stale_hours",
  "verdict": ["marked stale", "stays listed"],
  "field": "idle_h", "unit": "h",
  "rows": [ { "label": "muzzle-flash", "idle_h": 0.33 },
            { "label": "changelog approval", "idle_h": 27 } ] }
```

`rule` is exactly `<row field> <op> <control id>` — one comparison, no maths.
`source` is required and always shown. Write where you read it, or the formula
you computed it with. If the honest answer is "I invented these", write that —
or better, go and read the real thing.

### compare — options side by side

```json
{ "id": "approach", "type": "compare", "label": "How should X work?",
  "axes": [ { "id": "files", "label": "Files touched", "kind": "text" },
            { "id": "cost",  "label": "Build", "kind": "scale",
              "scale": ["S","M","L"], "better": "low" } ],
  "options": [
    { "value": "a", "label": "Poll",  "cells": { "files": "2", "cost": "S" } },
    { "value": "b", "label": "Watch", "cells": { "files": "5", "cost": "M" } } ],
  "default": "b" }
```

Every option must fill every axis — a missing cell is a spec error, and that is
the point: prose lets you argue one side, a matrix shows the hole.

**An axis must be checkable: the human could look at it and say "no, that's
wrong."** `Files touched: 5` qualifies. `Maintainability: high` does not.
`better: "none"` is the default — leave it there unless one end is genuinely
better, or you imply a winner you have not earned.

`axes[].template` generates cells from the option value: `{"kind":"code",
"template":"npm i {}"}` gives you four contexts × three names for four strings.

### bind — the string where it will actually appear

```json
{ "kind": "code", "bind": true, "language": "ts", "filename": "src/mcp.ts",
  "value": "import { uiAsk } from \"{{pkg}}\";\nnew {{pkg:pascal}}Client();" }
```

`{{control_id}}` substitutes live as they type. Transforms: `:pascal`,
`:camel`, `:kebab`. Nothing else — no arithmetic, no conditionals. Anything with
braces that isn't a declared control id is left alone, so Helm and Actions
snippets are safe.

- Paste from the **real** file, then insert the placeholder. An invented import
  path is a fiction the human will judge confidently.
- Include the lines above and below. One line of context adds nothing over a
  quoted string.
- To vary a whole line, use a `select` whose **option values are the
  alternative lines** and bind that. This is why there are no conditionals.
- `{{id}}` on a `select` gives you the **value**, not the label.
- Toggles and multiselects are rejected — use the pattern above.

### multiselect — a subset, with a live total

```json
{ "id": "files", "type": "multiselect", "label": "Files", "weightUnit": "lines",
  "options": [ { "value": "core/waiters.ts", "note": "the extraction", "weight": 180 } ],
  "default": ["core/waiters.ts"] }
```

Pre-check your proposal in `default`. Editing a proposal is far cheaper for them
than authoring a list, and `touched` tells you whether they moved off it.

### When there is genuinely nothing

Set `preview: { auto: false }`. A magnitude control alongside a `sample` or a
`compare` would otherwise spawn a frame that repeats what the table already says.
Do **not** drop `unit` to suppress it — `unit` drives readouts and scaling.

Write your own `previewModule` when the thing being tuned is specific — a 3D
model, a chart, an animation curve. Importable: `three` and `agentui`
(`swatchGrid`, `ruler`, `typeScale`, `card`, `autoPreview`). Nothing else.
````

Also update the checklist line and the "when to reach for this" section: **"You are about to invent a number"** becomes **"You are about to invent a number — go list the things it applies to first, then ask with a `sample`."**

---

## 6. Implementation map

| Change | File | Notes |
|---|---|---|
| `ruler` min/max, no-bar case | `ui/public/agentui/preview.js:149-181, 319-324` | P0-1 |
| drop `vector` from predicate | `shared/src/auto-preview.ts:22` | P0-2 |
| `card` title + copy from `spec.title`, drop `padding` | `preview.js:210-251, 304-307` | P0-3 |
| `duration\|delay` out of both SIZE_HINTS + sync test | `auto-preview.ts:16`, `preview.js:256`, `shared/test/` | P0-4; **edit both** |
| `isPreviewSubject` / `controlIsIllustrated` split | `shared/src/auto-preview.ts`, `ui/src/components/SessionView.tsx:34` | keep `controlHasVisual` as alias |
| `preview.auto` | `shared/src/spec.ts:158-162`, `server/src/core/session.ts:287` | 3 lines |
| `sample` schema + rule regex + row/field validation | `shared/src/spec.ts` (union at :110, superRefine at :258) | P1 |
| `sample` renderer; `values` prop | `ui/src/components/ContentRenderer.tsx`, `SessionView.tsx:71`, `styles.css` | P1; reads local values, never commits, never touches `touched` |
| `bind` + `interpolate()` + placeholder validation | `shared/src/spec.ts`, new `shared/src/bind.ts`, `ContentRenderer.tsx:45` | P3; substitute **before** `marked`/DOMPurify |
| `quickChoicesFor` respects `bind` | `server/src/core/session-manager.ts:80` | one line |
| `CompareControl` + cell/axis validation | `shared/src/spec.ts:93, 258`, `presets.ts:25` | P2 |
| `compare` renderer + narrow-viewport stack | `ui/src/components/ControlRenderer.tsx`, `styles.css` | P2 |
| `MultiSelectControl`, `ControlValue \|= string[]` | `spec.ts:93, 274, 280, 342-391, 411`, `presets.ts:25, 166` | P4; `inferType` must split string[] from number[] |
| `multiselect` renderer + total | `ControlRenderer.tsx`, `styles.css` | P4 |
| Docs | `skills/agentui/SKILL.md` | §5 |
| Dogfood | `server/examples/` | rewrite `usability-probe.mjs` q1/q2 as §3 examples 1 and 2 — building the tool and using it stay the same activity |

Tests: extend `shared/test/spec.test.ts` (rule grammar, missing-cell, placeholder validation, `string[]` round-trip through `normalizeValues`), `shared/test/auto-preview.test.ts` (the predicate split — both callers, opposite answers for `compare`), and the SIZE_HINTS sync guard.

---

## 7. The one sentence

**The visual is not a picture of the value — it is the value's verdict on rows the reader already recognises, and `source` being a required field is what keeps it from becoming fiction.** Everything above is scaffolding for that, plus the repairs needed so the tool has the credibility to say it.