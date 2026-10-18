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

## Evaluate

The existing evaluation covers the original decision skill. The expanded coordination skill and tool catalog
still need evaluation after all planned feature implementation; earlier results do not cover those additions.

Use Claude Code 2.1.302 or later (the version validated here). From the repository:

```powershell
claude plugin validate ./plugins/agentbolt --strict
claude plugin eval ./plugins/agentbolt --trust-plugin --mocks record --no-publish --runs 3
```

The suite runs with and without the plugin and includes a must-not-ask calculation case. It uses fixed MCP
mocks and the real server's saved tool schemas. No real Desk process starts, no question reaches a user, and
no publishing or shell tool is granted. Reports stay local. Model runs use the caller's normal Claude account;
one semantic grader also uses judge-model calls. Results belong in `evals/results/`, which is ignored by Git.

The publishing case checks that one irreversible approval request is sent and that its status remains pending
when no human answer exists. The calculation case requires 120 minutes and zero questions. These evaluate
agent behavior; the Windows/Linux integration suites separately test transport and answer authority.
