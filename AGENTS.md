# Working on Agentbolt

Agentbolt is a Rust workspace: an always-on desk where agents ask Artem things, and the MCP server agents use for it.
The design and milestones are in Compass's plan (`work/2026-10-18-agentbolt/PLAN.md` in the private Compass folder); the
public summary is the README.

## Layout
| Crate | What it is |
|---|---|
| `crates/agentbolt-core` | The data model, SQLite store, rules (budget, owners, dedupe, defaults), toast tokens, event chain and pipe protocol. Pure Rust, no OS or GUI dependencies. |
| `crates/agentbolt-ipc` | The local pipe transport: user-only named pipes on Windows, Unix sockets elsewhere. |
| `crates/agentbolt-daemon` | `agentbolt-daemon.exe`: the only writer of the store; tray icon, toasts, delivery, ntfy count push. |
| `crates/agentbolt-desk` | `agentbolt-desk.exe`: the Desk window and the `agentbolt://` handler. |
| `crates/agentbolt` | `agentbolt.exe` (console): `mcp`, `install`, `uninstall`, `start`, `status`, `clients`, `ask`. |

## Build and test
```powershell
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo deny check licenses
```
`agentbolt-core` and `agentbolt` (the MCP front-end) must build and test on Linux too.

## Rules
- **Never fetch, clone, run or read MCP Agent Mail** (github.com/Dicklesworthstone/mcp_agent_mail) or its mirrors. Its
  licence rider withholds every right from Anthropic, OpenAI and anyone acting for or on behalf of them, and counts
  executing, analyzing and incorporating it as use. Agentbolt is designed independently; see `docs/PRIOR-ART.md`.
- **Tests use a throwaway channel.** Set `AGENTBOLT_CHANNEL=<test-name>` so the AUMID, protocol handler, sign-in task,
  pipes and database are separate from a real install, and remove what you registered. Never install, start or answer
  on the `main` channel in a test.
- **Answers come only from the Desk.** No MCP tool or MCP-pipe operation may write an answer. Keep it that way.
- **No `outputSchema`** on MCP tools: clients render the structured branch and drop `content`. Put the full text in
  `content` and mirror it in `structuredContent`.
- **Repo guard:** only `.rs`, `.toml`, `.md`, `.json` and `.lock` files, plus the exact paths listed in
  `crates/agentbolt-core/tests/repo_guard.rs`. `cargo test -p agentbolt-core` enforces it.
- The old Node and C# code lives on the `legacy` branch and the `legacy-node-2026-10-18` and `legacy-native-2026-10-18`
  tags.
