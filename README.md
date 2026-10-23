# Agentbolt

**A desk on your PC where any AI agent can ask you something, and get your answer back.**

You run agents everywhere: Claude Code in a terminal, Claude chats, Codex threads, scheduled jobs. When one of them needs
you, the question gets lost in a session you'll never reopen. Agentbolt gives them one place to ask: a tray icon and a
window, like an inbox. Each ask comes with its evidence, the options, a recommended default and a deadline. You answer
in one click, and the answer stays available for the agent's next checkpoint, even if its session has ended since.

Local by default, with no account or API key for the Desk. Optional phone delivery uses your ntfy account;
remote machines use your SSH setup. [MIT licensed](LICENSE). The desktop targets Windows; Linux CLI/MCP
builds are available from source. macOS support has not been validated.

> **Public intermediate snapshot — 0.2.0 development, 23 October 2026.** The M2–M7 features are implemented,
> alongside the later UI, session search and automatic agent access changes. **Production readiness is
> unverified.** Testing and the production-readiness goal are paused at the maintainer's request.
> This source publication does not resume them or establish a stable release.

## Known gaps and unfinished work

The latest application source (`753f12d`) has successful Windows and Linux release builds, locally
prepared archives, and a normal Windows app update. The [candidate record](docs/CANDIDATE-753f12d.json)
pins their source and hashes. Earlier test reports cover older revisions; they do not validate the
current snapshot. The following gaps remain:

| Area | Current implementation and remaining gap |
|---|---|
| Chat navigation and replies | Exact-chat links and terminal resume controls are implemented. End-to-end Claude Desktop/Codex navigation and terminal resume still need verification on the current build. Codex wake-up is opt-in; a successful queue command does not prove delivery. Earlier checks found that a finished, unloaded chat did not resume. Answers remain available through durable reads. |
| Claude delivery (M2) | Long waits, relay handoffs and next-run recovery are implemented. Full timeout/background behavior, Claude Desktop use and the live dispatcher workflow remain unverified or undeployed. |
| Activity and adoption (M3) | Baseline and 14-day reporting are implemented. A real baseline, observation period and maintainer verdict have not been completed. No adoption or productivity benefit has been established. |
| Working-copy holds (M4) | Allowlisted Git scans, advisory holds and PR/merge checks are implemented. Real repository/workflow rollout and Windows junction/permission cases remain open. Missing or stale scans mean unknown, never clearance to merge. |
| Agent Board (M5) | Messages, requests, claims and renewable leases are implemented. Current-build multi-client, crash/restart and cross-machine behavior still needs validation. |
| Phone replies and integrations (M6) | Protected ntfy replies, Beads links, the Claude plugin and workflow adapters are implemented. Real phone-service delivery, offline/replay/revocation behavior and external configurations remain unverified. Phone buttons support only simple reversible questions with at most three short choices; other questions require the Desk. |
| Other machines (M7) | SSH transport and remote identity are implemented. Earlier checks used encrypted local loopback; deployment to an actual second computer remains unverified. Remote sessions use copy controls rather than local chat launch. |
| Session discovery and search | Discovery covers local Windows Codex and Claude Code sessions, including Claude Desktop Code chats. Browser chats, Cowork VM sessions, remote sessions and archived Codex chats are outside discovery. Search is local keyword search with bounded history, so it can miss older messages. Provider-format changes may require adapter updates. [Full limits](docs/SESSIONS.md). |
| Desktop polish and automatic access | Collapsed history, activity animation, local message search and automatic agent registration are implemented. Their current-build UI, accessibility and regression checks have not been completed. |

Release verification also remains unfinished:

- Current-source Windows/Linux tests, lint checks, repository checks, dependency-policy review and
  security review have not been completed after the later changes.
- Final-build installation, upgrade/rollback, draft preservation, uninstall and silent notification
  checks need repeating. The successful normal app update is narrower evidence.
- Startup latency, hidden-window behavior and idle CPU need measuring again after the session/search
  changes. Earlier performance measurements apply to older binaries.
- **No uninterrupted 72-hour reliability run has passed.** Previous runs were stopped; testing remains paused.
- Windows binaries are unsigned; signing is deferred beyond 0.2. Locally prepared archives have not
  been published as a GitHub release. Hosted release execution, package-manager distribution and
  registry acceptance remain unverified.

See [the feature inventory](docs/FEATURE-INVENTORY.md), [implementation tracker](docs/FULL-PLAN.md)
and [remaining work](NEXT.md) for detailed evidence and scope. These are recorded gaps, not claims
that the features have passed production validation.

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
- **Agents connect automatically:** no approval or owner review is needed. Optional notification and
  reply-delivery controls live in Settings → Agent preferences. Agents can set an available name using
  `requested_owner` in `desk_register`; their questions, answers and preferences follow the name immediately.
  Names cannot claim another agent's answers or enable relay/wake permissions. To share an existing identity
  across sessions, configure `agentbolt mcp --owner NAME` in the host. An unconfigured process without a host
  session ID gets a new identity when it restarts.
- **Codex delivery:** waking a chat is opt-in per owner. A queue command succeeding does not prove that
  a chat resumed, so its answer remains in normal `desk_answers` and `desk_register` checkpoint reads.
  Repeated reads return the same answer ID; agents should avoid acting twice on the same answer.
- **Question history:** answered and withdrawn questions keep their evidence, answer notes, delivery history
  and Open chat, including when the Inbox is empty.
- **Chat navigation:** open the originating chat, or copy a validated Claude/Codex resume command for an
  existing terminal. Remote sessions and older questions with no recorded source computer stay copy-only.
- **Sessions:** find open local Codex and Claude Code sessions, including Claude Desktop Code chats,
  even when they haven't connected to Agentbolt. Search local chat messages, titles and projects, then
  open a desktop chat directly. Running sessions have an activity animation; idle and recent chats stay
  collapsed. Terminal sessions offer resume. [Discovery and limits](docs/SESSIONS.md).
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

## Build and install this snapshot

There is no published binary release for this snapshot. With Rust/rustup and the Windows C++ build
tools installed, build the pinned source from this repository:

```powershell
git clone https://github.com/skulitom/Agentbolt.git
cd Agentbolt
cargo build --release --locked -p agentbolt -p agentbolt-daemon -p agentbolt-desk
.\target\release\agentbolt.exe install
```

The repository pins its Rust toolchain in [rust-toolchain.toml](rust-toolchain.toml). On Linux,
`cargo build --release --locked -p agentbolt` builds the CLI/MCP executable; the Desk is Windows-only.

If using a separately prepared Windows ZIP, extract it, keep its three programs together, and run
`agentbolt.exe install`.
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

## License

Agentbolt source is available under the [MIT License](LICENSE), copyright © 2026 Artem Skulimovskiy.
Third-party dependencies retain their own licenses; packaged builds include their notices.
