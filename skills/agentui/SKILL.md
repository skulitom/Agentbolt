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
  size. Do not guess and mention it — ask, with a slider, and keep working.
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

A slider with no visible effect is worse than a text question: they end up
guessing *and* the UI looks authoritative.

**Anything with a magnitude — a colour, a length, a duration, a size — needs a
preview.** You usually get one for free: if your spec has controls like that and
you supply no `previewModule`, one is generated (colour swatches with contrast
ratios, proportional rulers, a type specimen, a sample card).

Write your own `previewModule` when the thing being tuned is specific — a 3D
model, a chart, an animation curve:

```js
export default {
  setup({ container, spec, values, emit }) { /* build into container */ },
  update(values) { /* called continuously during drags */ },
  dispose() {},
};
```

- Importable: **`three`** (vendored, offline) and **`agentui`** (built-in preview
  helpers: `swatchGrid`, `ruler`, `typeScale`, `card`, `autoPreview`). Nothing else.
- It runs sandboxed on a separate origin and cannot reach the network.
- If it throws, the stack trace reaches you as a `preview_error` event. Fix it
  with `ui_update({ previewModule })` — the frame reloads, no page refresh, and
  the user keeps their adjustments.

Toggles, selects and free text are self-describing. Do not invent a visual for
them; the generated preview already leaves them as a state line.

## Authoring checklist

- ids are `snake_case` and unique
- every control has a `default` — treat it as your *recommendation*, since the
  UI labels untouched controls "suggested" and offers a one-click revert
- sliders need `min` and `max` (shorthand in `ui_ask` fills sane ones in)
- `vector` needs `size`, or a `default` array to infer it from
- `select` needs its `default` to be one of the option values
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
