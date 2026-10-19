# Agentbolt

**A desk on your PC where any AI agent can ask you something, and get your answer back.**

You run agents everywhere: Claude Code in a terminal, Claude chats, Codex threads, scheduled jobs. When one of them needs
you, the question gets lost in a session you'll never reopen. Agentbolt gives them one place to ask: a tray icon and a
window, like an inbox. Each ask comes with its evidence, the options, a recommended default and a deadline. You answer
in one click, and the answer reaches the agent that asked, even if its session has ended since.

Local by default, with no account or API key for the Desk. Optional phone delivery uses your ntfy account;
remote machines use your SSH setup. MIT licensed. Windows first; the MCP server also builds on Linux and macOS.

> Status: **0.2.0, full-plan implementation in progress.** The Board, work holds and Activity reporting are
> implemented on the candidate branch, alongside phone replies, SSH access, Beads and workflow adapters.
> Feature work comes before final verification. See [NEXT.md](NEXT.md) and [the feature tracker](docs/FULL-PLAN.md).

## How it works
```
agent ──stdio──► agentbolt mcp ──user-only pipe──► agentbolt-daemon ──► SQLite (the daemon is the only writer)
                                                        │   tray icon, silent toasts, optional phone delivery
                                                        ▼
                                                   agentbolt-desk (the window, opened from the tray)
```
- **Human decisions** use five MCP tools:
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
  the bottom for one hour, four hours, one day, or until unmuted. Timed mutes survive a restart and expire
  automatically. The Desk counts unnecessary questions per owner.
  Muted questions also stay out of phone notification counts.
- **Owner names:** agents can propose `requested_owner` through `desk_register`. Review it in Settings;
  approval renames that session's owner while preserving its questions, answers and preferences. A proposal
  cannot claim a name already in use or grant relay/wake permissions. To share an existing owner across
  sessions, configure `agentbolt mcp --owner NAME` in the host. An unconfigured process without a host session
  ID gets a new provisional identity when it restarts.
- **Codex delivery:** waking a chat is opt-in per owner. A queue command succeeding does not prove that
  a chat resumed, so its answer remains in normal `desk_answers` and `desk_register` checkpoint reads.
  Repeated reads return the same answer ID; agents should avoid acting twice on the same answer.
- **Question history:** answered and withdrawn questions keep their evidence, answer notes, delivery history
  and Open chat, including when the Inbox is empty.
- **Chat navigation:** open the originating chat, or copy a validated Claude/Codex resume command for an
  existing terminal. Remote sessions and older questions with no recorded source computer stay copy-only.
- **Sessions:** find open local Codex and Claude Code sessions, including Claude Desktop Code chats,
  even when they haven't connected to Agentbolt. Search by title or project and open a desktop chat
  directly. Recent chats stay collapsed; terminal sessions offer resume. [Discovery and limits](docs/SESSIONS.md).
- **Agent Board:** roles and current work, routed messages, deduplicated handoffs, one claimant per request,
  and renewable leases with fencing tokens. A stale claimant cannot overwrite a later claimant's result.
- **Work holds:** explicitly allowlisted working copies expose dirty/unpushed paths; agents can check for
  overlaps before opening or merging a PR. Scans are read-only. Missing, failed or stale scans stay unknown.
  See [Board and work holds](docs/BOARD-AND-HOLDS.md) for tools and configuration.
- **Activity:** a saved baseline and 14-day reporting show answer times, missed deadlines, unnecessary
  questions, one-click answer share, old unanswered questions and collision checks. Older unmeasured
  answers stay unknown. You can record your verdict in the Desk. [Measurement details](docs/ADOPTION.md).
- **Claude replies:** configurable long waits and prepared relay handoff records support live sessions and
  the next run of a stable owner. See [Claude delivery](docs/CLAUDE-DELIVERY.md).
- **Phone replies:** opt-in protected ntfy topics carry simple reversible choices. Replies are saved and
  processed by the installed Desk, with a confirmation back to your phone. [Setup and limits](docs/PHONE-REPLIES.md).
- **Other machines:** use the same Desk through SSH with verified host keys and explicit machine identity.
  Remote answers remain available through durable reads. [Remote setup](docs/REMOTE-MACHINES.md).
- **Integrations:** link an existing Beads issue to a Board handoff; give Reception durable notes delivery;
  derive PR paths from Git for work-hold checks. [Beads](docs/BEADS.md), [workflow adapters](docs/WORKFLOW-INTEGRATION.md).

## Install
Extract the Windows release ZIP, keep its three programs together, and run `agentbolt.exe install`.
For subsequent commands and MCP setup, use the installed copy in `%LOCALAPPDATA%\Programs\Agentbolt`.

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
Windows Installed Apps lists its version and can uninstall it while retaining questions and answers.
See [release preparation](docs/RELEASING.md) for archives, checksums, notices and distribution manifests.

## What it does and doesn't promise
- Only the installed Desk and its activation handlers can answer: no MCP tool can. The pipes accept only this Windows user, the
  programs check each other's identity, and toast buttons carry single-use tokens.
- **Same-user limit:** every agent on your PC runs as you. An agent with a shell can rewrite the database, or read a
  live toast token, without Agentbolt being able to tell. Agentbolt stops forged answers coming through its own
  interfaces; it can't stop a program that already has your permissions.
- **Not a permission bypass:** an answer arrives at the agent as data. For actions a host gates (publishing, merging,
  spending), Agentbolt gives you a link to open that session, so you approve it there.

## Develop
See [AGENTS.md](AGENTS.md). The old Node and C# AgentUI code is on the `legacy` branch.
