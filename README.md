# Agentbolt

**A desk on your PC where any AI agent can ask you something, and get your answer back.**

You run agents everywhere: Claude Code in a terminal, Claude chats, Codex threads, scheduled jobs. When one of them needs
you, the question gets lost in a session you'll never reopen. Agentbolt gives them one place to ask: a tray icon and a
window, like an inbox. Each ask comes with its evidence, the options, a recommended default and a deadline. You answer
in one click, and the answer reaches the agent that asked, even if its session has ended since.

Local, no account, no API key. MIT licensed. Windows first; the MCP server also builds and lists its tools on Linux and
macOS.

> Status: **0.1, the Desk.** The agent board (agents claiming work from each other so none duplicate it) comes after
> the Desk has proved itself.

## How it works
```
agent ──stdio──► agentbolt mcp ──user-only pipe──► agentbolt-daemon ──► SQLite (the daemon is the only writer)
                                                        │   tray icon, silent toasts, count-only phone push
                                                        ▼
                                                   agentbolt-desk (the window, opened from the tray)
```
- **Agents** use five MCP tools:
  - `desk_register` says who they are and returns any answers waiting for them;
  - `desk_ask` asks you a question;
  - `desk_answers` reads your answers;
  - `desk_cancel` retires a question that no longer matters;
  - `desk_relay` lets one configured relay agent deliver answers to sessions that can't be reached directly.
- **You** see a count in the tray. Open the Desk and click an option, or answer straight from a toast for simple,
  reversible questions.
- **Answers are durable.** An agent that ended its session gets the answer at its next start. Silence is never a yes. A
  question can take its default at the deadline only if it can be undone and you've been shown it, and an answer
  applied that way carries no authority of yours.
- **Inbox:** every question appears immediately, ordered by priority and then deadline. Each owner can have
  three open questions; duplicates are merged. There is no daily budget or Digest.
- **Noise control:** one silent toast popup per owner per ten minutes; additional eligible questions update
  the same notification with “N more waiting”. P2 questions notify when due within two hours. Mark a question
  **Not needed** to tell the agent it was unnecessary, or **Mute this agent** to keep its questions silently at
  the bottom until unmuted. The Desk counts unnecessary questions per owner.
- **Owner names:** agents can propose `requested_owner` through `desk_register`. Review it in Settings;
  approval renames that session's owner while preserving its questions, answers and preferences. A proposal
  cannot claim a name already in use or grant relay/wake permissions. To share an existing owner across
  sessions, configure `agentbolt mcp --owner NAME` in the host. An unconfigured process without a host session
  ID gets a new provisional identity when it restarts.

## Install
```powershell
agentbolt install          # copies the three programs, registers the tray at sign-in, starts it
agentbolt clients          # prints the commands that connect Claude Code and Codex
agentbolt status
agentbolt uninstall        # removes it again (add --purge to delete your answers too)
```

Upgrades preserve unsent Desk drafts and reopen visible windows in their original Windows sessions.
If an older Desk cannot prepare its drafts, the installer stops before replacing programs and asks you
to finish or copy those drafts, quit Agentbolt from the tray, then retry. Open MCP sessions can reconnect
after replacement; restart older client sessions after upgrading so they also use the current startup path.

The sign-in task is per Windows account and channel. Installation migrates an earlier task only after
verifying its account, executable, arguments and working folder; unrelated tasks are left untouched.
The 0.2 candidate is unsigned; code signing is deferred to a later release.

## What it does and doesn't promise
- Only the Desk window and its link handler can answer: no MCP tool can. The pipes accept only this Windows user, the
  programs check each other's identity, and toast buttons carry single-use tokens.
- **Same-user limit:** every agent on your PC runs as you. An agent with a shell can rewrite the database, or read a
  live toast token, without Agentbolt being able to tell. Agentbolt stops forged answers coming through its own
  interfaces; it can't stop a program that already has your permissions.
- **Not a permission bypass:** an answer arrives at the agent as data. For actions a host gates (publishing, merging,
  spending), Agentbolt gives you a link to open that session, so you approve it there.

## Develop
See [AGENTS.md](AGENTS.md). The old Node and C# AgentUI code is on the `legacy` branch.
