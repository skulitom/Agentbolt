# AgentUI

Interactive UIs for coding agents: an MCP server that lets any agent ask its
human with sliders, forms, diffs and live previews instead of guessing.

When an agent is about to invent a number, describe a colour in prose, or write
"let me know if you'd prefer…", it can instead open a real UI in your browser.
You adjust it while the agent keeps working, and it collects the answer when it
is ready.

## Install

```bash
npx agentui install
```

The installer detects the agents on your machine — Claude Code, Claude Desktop,
Cursor, Windsurf, VS Code (Copilot), Gemini CLI, Codex CLI — and registers the
server in each one's own config format.

## The tools

| Tool | What it does |
| --- | --- |
| `ui_ask` | One question, answered now — blocks until the user responds |
| `ui_create` | Open a UI the user adjusts while the agent keeps working |
| `ui_wait` | Long-poll: sleep until the user commits a change or comments |
| `ui_get_state` | Non-blocking read of the current values |
| `ui_update` | Change a live UI in place |
| `ui_save` / `ui_library` | Save a widget and reuse it later |
| `ui_close` | Close the session |

Full usage guidance for agents lives in
[skills/agentui/SKILL.md](skills/agentui/SKILL.md).

## How it works

One Node process launched by the agent over stdio, acting as both an MCP server
and a local HTTP + WebSocket server bound to `127.0.0.1`. The agent authors a
zod-validated JSON interface spec, optionally alongside a single-file ESM
preview module rendered in a sandboxed iframe on a second origin. All durable
state is plain JSON/NDJSON under `.agent-ui/`, so the agent can also just read
it off disk.

```
Agent ──stdio/MCP──> [agentui process] ──HTTP/WS──> Browser (React shell)
                            │                            │
                            └── .agent-ui/sessions/<id>/  └── sandboxed iframe (preview.mjs)
```

## Development

```bash
pnpm install
pnpm build      # shared -> ui -> server (vendors three.js, copies the shell into server/web)
pnpm test       # the server suite runs against the real built binary over stdio
pnpm typecheck
```

`pnpm build` before running anything: the server serves the *built* shell, and
the installer registers the built `dist/mcp.js` by absolute path.

Examples in `packages/server/examples/` are dogfood — each asks something real
about this project, so building the tool and using it are the same activity.

The design decisions and their evidence live in the header comments of the files
they affect, particularly `packages/server/src/core/{waiters,session,library}.ts`,
`packages/server/src/web/preview.ts` and `packages/ui/public/agentui/preview.js`.

## License

MIT
