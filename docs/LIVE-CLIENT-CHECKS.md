# Isolated live-client checks

These are synthetic test conversations, not messages to existing working chats. Each Agentbolt installation
uses a fresh non-main channel. No publication, merge, spending or external messaging is part of a test.

## Proposed conversations and messages

Create three temporary Codex chats named **Agentbolt test — active**, **Agentbolt test — idle** and
**Agentbolt test — finished**, plus one temporary Claude desktop chat named **Agentbolt test — navigation**.
The initial prompt explains that this is a transport test and instructs the agent to use only the isolated
Agentbolt connection, ask the synthetic question below, and stop after reporting the result. It must not
change files, publish, send messages outside these test chats, or treat the answer as permission for any action.

Question: **“Synthetic Agentbolt check: choose Test A or Test B.”** The tester selects **Test B** in the test
Desk. The delivered message must be labeled **“Synthetic Agentbolt test answer — no real-world authorization.”**
No real user preference is inferred from this value.

## Evidence to collect

For both hosts, inspect actual tool discovery and the full text/structured result, then check that **Open chat**
from the question and Agents row reaches this exact test conversation. Record whether Claude's MCP process
receives `CLAUDE_CODE_HOST_SESSION_ID`, whether it also receives the CLI session ID, and whether Codex actually
supplies per-call `_meta.threadId`; do not infer those transport facts from a unit test or a generated link.

For Codex, answer once while the test turn is active, once while the target is idle, and once after it has
finished. Let the real daemon invoke the configured queue command. Record receipt, retained durable answer,
and the original versus resumed sandbox and approval policy. Record a missing/unsupported host capability
as a failure or limitation, never as a pass from exit code alone. Keep automatic wake permission restricted
to explicitly marked codex-interactive test owners during this verification.

After collecting evidence, close/archive only these temporary conversations and uninstall/purge the fresh
test channel, including task, shortcut and per-user registration. Existing working conversations and the
main-channel installation stay outside the fixture.

Artem authorized this checklist through Desk answer `ask_8f5aedf0b2f55197` at 2026-10-18 21:32:13 UTC.

## Observed Codex results, 2026-10-18

The installed candidate at `e7e1ff0` ran on `client-1792359530619`. A byte-preserving stdio relay logged actual
host JSON-RPC; it supplied no session metadata. Codex Desktop 0.163.0-alpha.10.2 supplied per-call
`_meta.threadId`. All five tools had no `outputSchema`, and real calls exposed both full text and structured
results. The daemon invoked native Codex CLI 0.160.0, with no sandbox, approval or profile overrides.

| Case | Result |
|---|---|
| Idle, loaded chat | Pass. Desk answer at 21:41:38 UTC started the next turn, which read Test B and the exact synthetic note. |
| Answer during an active turn | Pass. Queued at 21:42:08 UTC; received after the original turn finished, in a new turn starting 21:45:51 UTC. |
| Completed, unloaded, unarchived chat | Limitation. Queue exited 0 at 21:43:05 UTC, but the chat stayed unloaded for over five minutes. A later explicit test follow-up could read the durable answer. That follow-up is not an automatic-wake pass. |
| Archived chat | Queue exited 1. The answer remained durable and was readable after unarchiving. |

The idle and active chats' original and automatically resumed `turn_context` records both specify
`danger-full-access` and approval policy `never`; equality was checked independently of the agents' reports.
This checks preservation of the tested policy, not every possible host policy configuration.
`CODEX-LIVE-2026-10-18.json` contains bounded trace summaries, trace hashes and those policy records.

Invoking Open chat on the unloaded chat's Agents row returned no Desk error, but no destination window appeared
on the isolated desktop and the target stayed unloaded. Correct visible navigation is therefore still
unverified. Claude Desktop runs in the main session and was not moved or controlled through the test desktop.
Its actual MCP environment and visible navigation still need a normal-session check.

The three temporary Codex chats were archived after testing. The project-only MCP override was removed.
The installed test channel uninstalled/purged successfully, including its task, registrations, shortcut,
programs and data; the main connection and global client configuration were unchanged.
