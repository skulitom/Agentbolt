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
pnpm test      # 81 tests; the server suite runs against the real built binary over stdio
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
