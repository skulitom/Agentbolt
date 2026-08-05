---
name: agentui
description: Ask your human for input that prose cannot capture — tuning values, colours, a choice between options, approval of a diff — by opening a real interactive UI in their browser. Use when you would otherwise guess at a subjective value, describe a visual result in words, or ask a yes/no question that would be clearer with the thing visible. Also use to reuse a saved widget instead of rebuilding one.
---

# AgentUI

You can open a real UI in the user's browser: sliders, pickers, forms, a diff to
approve, or a live preview you write yourself. They adjust it while you keep
working, and you collect the answer when you are ready.

## When to reach for this

Concrete moments, because "when it would be useful" is too vague to act on:

- **You are about to invent a number.** A timeout, a threshold, a duration, a
  size. Go list the things the number applies to first, then ask with a
  `sample` — a slider over real rows is answerable; a slider alone is a guess
  with a nicer interface.
- **You are about to write "let me know if you'd prefer…"** That sentence is a
  question with no answer channel. Make it `ui_ask` with options instead.
- **Your change alters something the user will look at.** Colours, spacing,
  layout, animation, rendered output. Describing it in prose is strictly worse
  than showing it.
- **You are choosing between approaches and genuinely don't know.** Options with
  a sentence each beats a wall of prose weighing them up.
- **You want approval for something you can show.** A diff, a screenshot you just
  took, a generated file. Put it in `content` and give them buttons.
- **You are iterating on a value with them.** Two rounds of "try 0.6" in chat is
  the signal you should have opened a widget.

Not for: anything with a single obvious answer, anything you can verify
yourself, or a decision you should just make and report.

## Ask what only they can answer

The test is not "is this decision important?" — it is **"could I have found this
out myself?"** If the answer is in the code, go and read it. What you cannot
read is their taste, their priorities, how much time something is worth, and
what they meant.

Assume the person has not read the file you are in and does not want to. A
question that requires them to open the repo to answer it has been handed to the
wrong party.

| Do not ask | Ask instead |
|---|---|
| Which files should this refactor touch? | Should I do the safe version or the thorough one? (with what each changes) |
| Should this live in `core/` or `web/`? | — nothing. Decide it. |
| Which internal API should the module call? | — nothing. That is what you are for. |
| Is this the right regex? | Here are the inputs it now accepts and rejects — right? |
| Should I use a Map or an object here? | — nothing. |
| Which of these 6 files do you want rewritten? | Which of these outcomes do you want? (each with a time cost) |

The exception is when the person has told you they are working on the internals
— then the file-level question is exactly right. Read the room from what they
have been asking you, not from what is convenient to ask.

Two things always worth asking, whoever they are: **anything irreversible**, and
**anything where you are about to spend a lot of effort on a guess.**

## The one rule that matters

**Never block on a human.** `ui_ask` and `ui_create` return immediately by
design. Ask, carry on with anything else you can do, and collect the answer at
your next natural checkpoint with `ui_wait` or `ui_get_state`.

Blocking wastes exactly the time the person spends thinking. If you have three
questions, ask all three up front and gather them later.

Only pass `wait: true` when you genuinely cannot proceed — the answer decides
which file you edit next, and there is no other useful work.

**Asking is cheap — ask more than feels natural.** A tab is only opened if the
user does not already have AgentUI open; otherwise the question appears in the
window they already have. Questions that are a single choice can be answered
right there without opening anything. So three questions cost one tab, not
three, and you should stop rationing them.

## Which tool

| You want to | Use |
|---|---|
| Ask one question, maybe with options or a few fields | `ui_ask` |
| Give them something to tune while you work | `ui_create` |
| Reuse something you built before | `ui_library` then `ui_create({ template })` |
| Pick up an answer | `ui_wait` (blocks efficiently) or `ui_get_state` (snapshot) |
| Fix or change a live widget | `ui_update` |
| Keep a widget worth having again | `ui_save` |
| Tidy up when you are done with one | `ui_close` |
| Look up the exact fields of a control or block | `ui_schema` |

`ui_create` and `ui_ask` advertise the *shape* of a spec and the list of control
types, not every field of every type — carrying the full schema would cost you
thousands of tokens on every request instead of once, when you need it. So when
you are reaching for something structured — a `compare`'s axes, a `sample`'s
rows, a slider's bounds — call `ui_schema({ of: "control", type: "compare" })`
rather than guessing and reading the rejection. `of` also takes `"content"`,
`"view"`, `"spec"` and `"preview-module"`.

## Previews

A widget with a magnitude in it gets a preview automatically — colours, lengths,
type sizes, positions and easing curves are inferred from the controls. You do
not have to ask for one, and `preview: { auto: false }` declines it.

**When the inferred arrangement is not what you meant, declare it** rather than
writing a whole module:

```js
preview: {
  views: [
    { view: "gradient", controls: ["ramp_from", "ramp_to"] },
    { view: "box", controls: ["padding", "corner_radius"], title: "Card" },
  ],
}
```

Declaring any view replaces inference entirely. `ui_schema({ of: "view" })` lists
what each one draws. Three of them — `gradient`, `box` and `shadow` — are only
ever reachable this way, because which two colours form a ramp and which length
is "the padding" are things you know and inference would be guessing at.

**A `previewModule` is still the answer for anything custom** — a three.js scene,
a bespoke chart. It gets `setup({ container, spec, values, data, emit, setValues })`
and `update(values, meta)`. Two things worth knowing:

- `preview: { data }` carries anything you MEASURED into the module. Real timings
  and real file sizes are what makes a preview worth looking at; inlining them
  into the module source hides them from everything that reads the spec.
- `setValues` makes a preview an INPUT. Drag a point, move a stop — call
  `setValues(values)` during the gesture and `setValues(values, { commit: true })`
  on release. Only the committed one wakes you, so a drag does not spam you.

## The normal shape

```js
// Check first — reusing keeps the settings they chose last time.
ui_library({ query: "muzzle flash" })

const { sessionId, url } = ui_create({ template: "muzzle-flash" })
// …go do other work…
ui_wait({ sessionId })   // → committed values, comments, or the final answer
```

Asking a question costs one line:

```js
ui_ask({ question: "Ship the auth refactor?",
         content: [{ kind: "diff", value: patch }],
         options: ["Ship it", "Hold"] })
// returns { sessionId, url, status: "asked" } in milliseconds
```

## Reading an answer honestly

`output` splits the values into `touched` and `untouched`.

- **`touched`** — the user actually engaged with these. Real decisions.
- **`untouched`** — still sitting at the default *you* authored. **This is not
  agreement.** They may never have noticed the control.

Treat an untouched value as "no opinion expressed". If it matters, ask about it
specifically rather than claiming they chose it. This exists because it is a
mistake that is very easy to make and impossible to spot afterwards.

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

And the corollary that decides most of the hard cases: **a yes/no policy
question is almost always a threshold in disguise. Ask the threshold, over the
real things the threshold applies to.**

### Question shape → treatment

| The question is | Do this | Never |
|---|---|---|
| A threshold over things that exist (timeouts, staleness, limits, quotas) | `sample` block, `rule: "field > control"`, rows you just read, plus the slider | Draw a bar for the number |
| An abstract count (retries, batch size, concurrency) | Compute the consequence of each candidate value; `sample` with `rule: "n == control"` and the formula in `source` | Invent a scale for it |
| N approaches with tradeoffs | `compare` control; axes are facts the human could call wrong | Axes like "elegance", "maintainability" |
| Yes/no policy | Ask the threshold behind it, over the real things. Truly binary? Evidence in a `code` block + two buttons | A two-column table of adjectives |
| Structural choice (where things live, which default) | `sample` — replay the rule against what actually happened | A Venn diagram with invented areas |
| A name, a message, any string | `bind: true` code blocks showing the real places it appears + a `text` control | A list of candidates |
| A subset of a known set | `multiselect` of **outcomes**, `weight` from something you measured or estimated | A list of filenames to tick |
| Layout or rendering | Screenshot both, two `image` blocks | Hand-draw a mock of the thing you're asking about |
| Ordering / "what first?" | `compare` on the candidates. Ask again next time | Build a ranking |
| Anything with a magnitude (colour, length, size, easing) | Nothing — the generated preview already draws it | Describe it in prose |
| Open-ended, generative, "what am I missing?" | Ask in chat | Any widget |

Radar charts, 2×2 matrices and dependency graphs are forbidden outright: their
coordinates are your guesses rendered as measurement.

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
The left side is always a row field, the right side always a control. Ops are
`> >= < <=` (which sort the rows and draw the live threshold across them) and
`== !=` (which light up the matching row — the lookup-table mode). Add
`total: { "field": "kb", "unit": " kB" }` for a live sum over the matching rows.

`source` is required and always shown. Write where you read it, or the formula
you computed it with. If the honest answer is "I invented these", write that —
or better, go and read the real thing. Rows must be things the human
recognises; with fewer than three you have a sentence, not a table.

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
the point: prose lets you argue one side, a matrix shows the hole. Axis kinds
are `text`, `bool` (cells are `true`/`false`), `code` and `scale`.

**An axis must be checkable: the human could look at it and say "no, that's
wrong."** `Files touched: 5` qualifies. `Maintainability: high` does not.
`better: "none"` is the default — leave it there unless one end is genuinely
better, or you imply a winner you have not earned.

`axes[].template` generates cells from the option value: `{"kind":"code",
"template":"npm i {}"}` gives you four contexts × three names for four strings.

The answer is one option value, exactly like a `select`.

### bind — the string where it will actually appear

```json
{ "kind": "code", "bind": true, "language": "ts", "filename": "src/mcp.ts",
  "value": "import { uiAsk } from \"{{pkg}}\";\nnew {{pkg:pascal}}Client();" }
```

`{{control_id}}` substitutes live as they type, in `markdown` and `code` blocks.
Transforms: `:pascal`, `:camel`, `:kebab`. Nothing else — no arithmetic, no
conditionals. Anything with braces that isn't a declared control id is left
alone, so Helm and Actions snippets are safe (and blocks without `bind` are
never touched at all).

- Paste from the **real** file, then insert the placeholder. An invented import
  path is a fiction the human will judge confidently.
- Include the lines above and below. One line of context adds nothing over a
  quoted string.
- To vary a whole line, use a `select` whose **option values are the
  alternative lines** and bind that. This is why there are no conditionals.
- `{{id}}` on a `select` gives you the **value**, not the label; on a number it
  gives the bare number, so write the unit in the template (`{{ms}}ms`).
- Toggles, vectors and multiselects are rejected — use the pattern above.

### multiselect — a subset, with a live total

```json
{ "id": "extras", "type": "multiselect", "label": "Extras", "weightUnit": "min",
  "options": [
    { "value": "contrast", "label": "Fix the dark-mode contrast on buttons",
      "note": "fails WCAG AA right now", "weight": 5 },
    { "value": "shortcuts", "label": "Keyboard shortcuts for the toolbar", "weight": 40 } ],
  "default": ["contrast"] }
```

Options are **outcomes**, not files — "fix the dark-mode contrast", not
`Button.tsx`. Each one gets a bar and the footer sums the selection live, so
"that one option is most of the cost" is a shape rather than arithmetic.

Pre-check your proposal in `default`. Editing a proposal is far cheaper for them
than authoring a list, and `touched` tells you whether they moved off it.
`weight` is something you **estimated or measured** and can defend — minutes,
kB, requests. With no weights it is a plain checkbox list, which is fine; do not
invent weights to make it look like more.

### When there is genuinely nothing

Set `preview: { auto: false }`. A magnitude control alongside a `sample` or a
`compare` would otherwise spawn a frame that repeats what the table already says.
Do **not** drop `unit` to suppress it — `unit` drives readouts and scaling.

### The preview frame

**Anything with a magnitude — a colour, a length, a duration, a size — gets a
preview for free:** if your spec has controls like that and you supply no
`previewModule`, one is generated (colour swatches with contrast ratios,
proportional rulers scaled over the control's own `min`/`max`, a type specimen,
a sample card, an easing curve).

Write your own `previewModule` when the thing being tuned is specific — a 3D
model, a chart, an animation curve:

```js
export default {
  setup({ container, spec, values, emit }) { /* build into container */ },
  update(values, meta) { /* called continuously during drags */ },
  dispose() {},
};
```

- `meta.source` tells you who caused the update: `"user"` is a live gesture —
  track it 1:1, never animate under the finger (typed edits count as "user"
  too, by design) — while `"agent"` is a programmatic jump (your `ui_update`
  push, a reset) that you should animate so the change is legible.
- Importable: **`three`** (vendored, offline) and **`agentui`** (built-in preview
  helpers: `swatchGrid`, `ruler`, `typeScale`, `card`, `easingCurve`,
  `autoPreview` — plus motion utilities: `spring`, `tween`, `easings`,
  `cubicBezier`, `resolveEasing`, `durations`, `stopAll`, also importable alone
  as **`agentui/motion`**). Nothing else.
- Motion in one line: `const s = spring(0, { onUpdate: draw }); s.set(target)`
  glides there carrying velocity; `s.jump(v)` writes instantly — use `set` for
  `"agent"` frames and `jump` for `"user"` frames. `tween({ duration, easing,
  onUpdate })` is for one-shot replays. Both settle and stop on their own,
  honour reduced-motion, and keep animating in background tabs. Call
  `stopAll()` in `dispose()` if you used them.
- Tuning an animation? Give the spec an easing `select` (options like
  `ease-out`, `spring`, `cubic-bezier(…)`) next to a duration slider with
  `unit: "ms"` and the generated preview plots the curve and replays it at the
  real duration — often you need no module at all.
- It runs sandboxed on a separate origin and cannot reach the network.
- If it throws, the stack trace reaches you as a `preview_error` event. Fix it
  with `ui_update({ previewModule })` — the frame reloads, no page refresh, and
  the user keeps their adjustments.

Toggles, selects and free text are self-describing. Do not invent a visual for
them in the frame — a `sample`, a `compare` or a bound block is how those get
illustrated, and all three live in the page rather than the preview.

## Authoring checklist

- ids are `snake_case` and unique
- every control has a `default` — treat it as your *recommendation*, since the
  UI labels untouched controls "suggested" and offers a one-click revert
- sliders need `min` and `max` (shorthand in `ui_ask` fills sane ones in)
- `vector` needs `size`, or a `default` array to infer it from
- `select` needs its `default` to be one of the option values
- `sample` needs a `source`, and every row needs the field its `rule` names
- `compare` needs every option to fill every axis, and axes you could be
  contradicted about
- `bind` placeholders must name declared controls (anything else is left alone)
- use `unit` (`px`, `ms`, `°`) — it drives the preview's scaling and readouts

Invalid specs come back with the exact path of each problem
(`controls[3].default: expected number, received string`). Objects are strict, so
a typo like `minimum` instead of `min` is reported rather than silently ignored.

## Waiting properly

```js
ui_wait({ sessionId, sinceSeq })   // sinceSeq = the lastEventSeq you last saw
```

- `reason: "timeout"` is **normal, not an error**. It means they have not acted.
  Go do something else, or call again with the same cursor — nothing is lost.
- Omit `sessionId` to wake on *any* open widget when several are outstanding.
- Slider drags do **not** wake you. Only committed changes, comments, button
  presses, preview errors and finishing do.
- `reason: "closed"` with an event saying the user **dismissed** it *is* an
  answer: they read the question and chose not to answer it. Decide yourself,
  say which way you went, and do not ask again. Dismissing is theirs to do from
  the dashboard or from the widget's own page — a question they are ignoring is
  one they can now get rid of, which is what keeps the rest of the list worth
  reading.
- Dismissing offers an optional reason, and when they type one it arrives on the
  same event: `#7 dismissed by the user: "wrong question — I already did this"`.
  **Read it as the answer and act on it.** It is usually more useful than the
  options you offered, because it is the thing you did not think to ask.

## Saving what worked

```js
ui_save({ sessionId, name: "Muzzle flash", triggers: ["vfx timing", "gun flash"] })
```

Saves globally, so it is there in every project. `triggers` are the phrases that
should find it later — the highest-signal field, and cheap to supply. Reopening
with `template:` restores the values the user last left, not your defaults.

If you hand-author something close to a saved widget, `ui_create` tells you so in
its result. Take the hint: reuse keeps their previous choices.

## Telling the user

Give them the URL, and if your host can render inline content, show a small card
alongside it — title, current values, which are still suggestions. A bare link in
a transcript reads as a brush-off. The interaction still happens in the browser
tab; the card is only presence.
