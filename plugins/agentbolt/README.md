# Agentbolt for Claude Code

This plugin adds the local Agentbolt MCP connection, a skill for decisions such as “check with me before
you publish,” and a skill for authorized agent handoffs and work-overlap checks. Claude's connection permits
ten-minute answer waits. Install the Windows Agentbolt ZIP first. Load this directory with
`claude --plugin-dir ./plugins/agentbolt` from the repository, or extract the plugin ZIP and pass its folder.
Select the installed `agentbolt.exe` when Claude prompts for configuration. Use the normal `main` channel
for your own Desk; development tests must use a separate installation and channel.

The plugin does not install Agentbolt, publish anything, grant host permissions or write answers. Use one
Agentbolt connection in each client; if you already registered it manually, avoid enabling both connections
in the same session. Existing client configuration is not changed automatically.

## Tool catalog

The saved mock catalog now includes all twelve Desk, Board and hold tools and the plugin's ten-minute
wait cap. Regenerate it from the same schema source used by the server with
`cargo run -p agentbolt --example tool_catalog` (redirect stdout to
`plugins/agentbolt/evals/mocks/agentbolt/_tools.json` as UTF-8). This only generates schema JSON; it
never connects to the daemon or creates questions.

## Evaluate

The six cases cover approval before publishing, recovery of a saved human answer, a bounded wait for an
unanswered question, completion of an existing fenced handoff, a stale scan blocking PR clearance, and a
calculation that needs neither skill nor a question. Each case runs three times with the plugin and three
times without it. The [2026-10-19 evidence record](../../docs/PLUGIN-EVAL-2026-10-19.json) includes the
source hashes, tool calls, grader results and a manual wording finding with its corrected reruns.

Use Claude Code 2.1.302 or later (the version validated here). From the repository:

```powershell
claude plugin validate ./plugins/agentbolt --strict
claude plugin eval ./plugins/agentbolt --trust-plugin --mocks record --no-publish --runs 3
```

The suite uses fixed MCP mocks and the real server's saved tool schemas. Each case exposes only the tools
with mocks; case-local responses override the shared fixtures. No real Desk process starts, no question reaches a user, and
no publishing or shell tool is granted. Reports stay local. Model runs use the caller's normal Claude account;
semantic graders also use judge-model calls. Results belong in `evals/results/`, which is ignored by Git.

The publishing case checks that one irreversible approval request is sent and that its status remains pending
when no human answer exists. The calculation case requires 120 minutes and zero questions. These evaluate
agent behavior; the Windows/Linux integration suites separately test transport and answer authority.
The waiting mock returns immediately, so it checks the requested wait and pending state, not a live ten-minute
host session. Recovery uses a supplied historical reply, not a live persistence test. The comparison arm has
no plugin tools, so its score difference does not isolate the contribution of skill wording.

For real later-run recovery, save the ask ID and retain the same host session identity. Recovery across
different sessions also needs a stable owner configured by the host or approved in the Desk; agents must
not invent an owner identity to read another run's answers. Live host waits, navigation and dispatcher
rollout remain separate integration checks.
