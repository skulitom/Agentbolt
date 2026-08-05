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
pnpm test      # 255 tests; the server suite runs against the real built binary over stdio
pnpm typecheck
```

`pnpm build` before running anything: the server serves the *built* shell, and
the installer registers the built `dist/mcp.js` by absolute path.

Examples in `packages/server/examples/` are dogfood — each asks something real
about this project, so building the tool and using it are the same activity.

## Things that will bite you

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
  is the forbidden failure. The shell's motion tokens (`--t-*` in styles.css)
  are hand-mirrored in `packages/ui/public/agentui/motion.js`; change both.
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
- **A new control type is invisible until it is in `FieldShorthand`'s enum**
  (`shared/src/presets.ts`) — `ui_ask` rejects the whole call otherwise, with an
  error about the type rather than about the enum.
