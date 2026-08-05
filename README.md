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

## Opening a widget on your phone

Off by default, because binding to `127.0.0.1` is not a detail — it is the whole
security model. There is no login anywhere in this server; loopback is what made
one unnecessary.

Set `AGENT_UI_HOST=lan` in the `env` of your agent's MCP registration and
restart it. On startup the server prints an address and a key:

```
Open on another device on this network:
  http://192.168.1.112:4608/?k=vckqvceqbzz7
```

Open that once on the phone; a cookie carries the key from then on, and the
inbox lists whatever is waiting, so no one types a session id. If the address it
picks is wrong — a VPN tunnel often outranks your real card — pin the right one
with `AGENT_UI_HOST=192.168.1.112`.

**What you are trading.** Traffic is plain HTTP, because a self-signed
certificate on a LAN address produces a full-page warning on exactly the phone
this exists to serve. Anyone on that network holding the key can read what the
agent attached — diffs, file contents, screenshots — and *answer for you*: their
decision reaches your agent as though it were yours. That is a fair trade on a
home network and a bad one on cafe or hotel wifi. Your phone also has to be on
the same network; this does nothing over cellular.

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
