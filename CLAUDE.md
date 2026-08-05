# AgentUI

Interactive UIs for coding agents: an MCP server that lets any agent ask its
human with sliders, forms, diffs and live previews instead of guessing.

- **How to use the tools:** [skills/agentui/SKILL.md](skills/agentui/SKILL.md)
- **Why it is built this way:** the design decisions and their evidence are in
  the header comments of the files they affect — particularly
  `packages/server/src/core/{waiters,session,library}.ts`,
  `packages/server/src/web/preview.ts` and `packages/ui/public/agentui/preview.js`.

## Working on it

```bash
pnpm build     # shared -> ui -> server (vendors three.js, copies the shell into server/web)
pnpm test      # 394 tests; the server suite runs against the real built binary over stdio
pnpm typecheck
```

`pnpm build` before running anything: the server serves the *built* shell, and
the installer registers the built `dist/mcp.js` by absolute path.

Examples in `packages/server/examples/` are dogfood — each asks something real
about this project, so building the tool and using it are the same activity.

## Things that will bite you

- **Loopback is the entire security model, until it is not.** There is no login
  anywhere in this server: 127.0.0.1 is what makes it safe. `AGENT_UI_HOST=lan`
  (or an explicit address) trades that for reachability and MUST carry the token
  in `web/access.ts` — the shell, the preview origin AND the WebSocket upgrade.
  Anyone who reaches a widget can read what the agent attached and *answer as
  you*. It is off by default and prints what it is giving away.
- **stdout belongs to the MCP transport.** `protectStdout()` redirects
  `console.*` to stderr and blocks stray writes; the transport gets the only
  remaining handle. A single stray print corrupts every message after it.
- **The preview frame is a second origin** (shell port + 1). Sandboxing on one
  origin gives an opaque origin, and module imports then fail the *CORS* check —
  the DevTools error blames CSP and sends you the wrong way.
- **`values_interim` must never wake a waiter.** It is drag noise. If it wakes
  `ui_wait`, one slider drag wakes the agent hundreds of times.
- **`touched` is replaced, not merged.** Clearing a flag is how a user says "no
  opinion here" after poking a control.
- **Previews animate only agent-sourced changes.** `update(values, meta)` gets
  `meta.source`; `"user"` frames must track 1:1 — animating under a live drag
  is the forbidden failure. The shell's motion tokens (`--t-*` in
  `styles/tokens-and-base.css`) are hand-mirrored in
  `packages/ui/public/agentui/motion.js`; change both.
- **A visual must encode something the agent went and got.** `sample`, `compare`
  and `bind` render in the SHELL, not the frame, and read local values — live at
  60fps, nothing on the wire. `sample.source` is required on purpose: a table of
  invented numbers is worse than a sentence, and the field is what makes that
  visible instead of asking anyone to remember.
- **Closing has no message of its own.** A tab learns a session ended from the
  relayed `closed` *event* — nothing sends a `closed` frame — so anything that
  ends a session (`ui_close`, the user dismissing it) reaches the browser only
  through `store.ts`'s event case. Miss it and the page keeps offering controls
  the server has already stopped accepting.
- **A new control type is four edits, and the compiler names two of them.** A
  schema in `shared/src/spec/control-schemas.ts`, a member of the `Control`
  union, an entry in `CONTROLS` (`spec/control-registry.ts`), and an entry in
  `CONTROL_INPUTS` (`ui/src/components/controls/index.tsx`). Both registries are
  mapped `Record`s over the union, so a missing entry will not compile. Content
  kinds work the same way via `CONTENT_BLOCKS` and `BLOCK_RENDERERS`. This used
  to be ~15 sites across 8 files, two of which failed *silently*.
- **`shared/src/vocab.js` is plain JS on purpose, and it is COPIED.** The preview
  frame is a second origin with no bundler, so anything both halves must agree on
  (hint regexes, `SELF_SHOWN`, display rounding) lives there and is synced to
  `ui/public/agentui/vocab.js` by a `prebuild`/`predev` script. The copy is
  checked in; `vocab-sync.test.ts` compares the two byte for byte.
- **One function decides whether a preview opens AND what goes in it.**
  `planPreview` (`shared/src/preview-plan.js`, synced to the frame) returns an
  ordered list of panels; `shouldAutoPreview` is just "does any planned view
  draw". These used to be two implementations a test compared, and they
  disagreed — a "type_scale" control landed in the frame's font bucket while the
  gate said there was nothing to show, so the frame never opened.
- **A new preview view is two entries.** `PREVIEW_VIEWS` in `preview-plan.js`
  (the vocabulary — label, accepted control types, and whether drawing it
  justifies opening a frame) and `VIEW_IMPLS` in `preview.js` (the DOM). A test
  asserts the key sets match: a view with `draws: true` and no implementation
  opens a frame and then renders nothing, which is worse than not having it.
- **A preview can write back, and must do it through the shell's own path.**
  `setValues` in the frame posts `set_values`; `PreviewFrame` replays it through
  `setLocalValue`/`commit`. Going around that would lose `touched`, animate
  under the user's finger, skip the interim throttle, and ignore whether the
  session is still open — every invariant lives on that path.
- **MCP tool schemas are shallow on purpose.** `ui_ask` and `ui_create` advertise
  the containers and the enumerated vocabulary, not the full spec — inlining the
  real schemas costs ~5,000 tokens on every request. Detail comes from
  `ui_schema` on demand. A size budget in `mcp-e2e.test.ts` guards it, and
  nothing exposed may use `.default()`: zod 4 emits defaulted fields as
  *required*, turning a convenience into a mandatory argument.
