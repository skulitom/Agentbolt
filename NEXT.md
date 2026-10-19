# Next: from 0.1 to production

The design and milestones are in Compass's `work/2026-10-18-agentbolt/PLAN.md` (v4). 0.1 is M0 plus most of M1. This
list is what's left before a public release. Do the first section first.

**Current scope (Artem, 2026-10-18): implement the full plan, including M2–M7.** This overrides the older
handoff's M0/M1 limit and the evidence gate before building the Board. Finish all features before heavy
testing, licensing or release validation. [Full feature work](docs/FULL-PLAN.md) tracks the expanded scope.

The expanded implementation now includes selectable question history, Board tools and Desk tab,
working-copy/agent holds, Activity reporting and saved baselines, Claude long waits, protected phone
replies, Beads links and authenticated SSH access for remote agents. Concrete `relay-notes` and `pr-check`
commands implement durable Reception handoffs and PR/merge hold checks. These additions now have
[focused local validation](docs/EXPANDED-VALIDATION.md). No phone account, live workflow, scan configuration or baseline has been activated.
Earlier test results below predate these additions.

Timed mute, partial Board profile updates, Inbox/History/Board/hold navigation, filtered relay and answer
paging, linked Board messages and failed handoff outcomes are implemented. The twelve-tool plugin
catalog now comes from the server schema source. Agents/Owners directories, closed-draft reconciliation,
complete delivery history and fixed-period adoption reporting are implemented too. The
[feature inventory](docs/FEATURE-INVENTORY.md) maps the full plan to code. Implementation coverage is
recorded; Windows workspace tests and Linux core/MCP/IPC/daemon checks pass, including phone HTTP mocks.
All-target Clippy passes on both platforms, and native ordinary/warm Desk lifecycle checks pass in Anode.
Native 60-item paging/search and draft navigation now pass. Board sections and working-copy settings
are directly accessible; delivery history and Activity breakdowns are bounded. Off-page/inactive
claimants retain their name and chat context, and pre-install Activity periods show unknown history.
Remote MCP hosts can now select their own SSH configuration file. A real encrypted loopback relay
passes authentication, origin, durable-state, reconnect, uncertain-write and concurrency checks, including
the MCP command and matching text/structured results. Actual cross-machine deployment remains open.
Remote evidence paths and chat links now stay on the originating computer, with copy controls. Board
messages retain their original machine after sender expiry or retries; legacy origins remain unknown.
Expanded security, final-candidate and live host validation remain open.
No phone account, scan, adoption period or live workflow has been activated.


**Current order: all features first, then validation.** The long CPU
probe and 72-hour run were stopped, and the hourly monitor is paused. The full-plan source inventory is now recorded. Address any newly discovered feature gaps first; then
validate the expanded features before packaging and heavy release checks.
See [the implementation audit](docs/IMPLEMENTATION-AUDIT.md). It found and fixed queue acceptance hiding
unread answers, phone counts including muted owners, and stale toasts bypassing mute/pause. Legacy Digest
wording was removed from notification code; older-client wire compatibility remains.

**0.2 work in progress:** isolated formatting commit `729fc82`; measured warm Desk startup now passes
five Anode reopen cycles (11–13 ms window, 27–58 ms Inbox), including hidden-ask safety and crash cleanup.
Validated chat navigation, direct Inbox, Not needed, persistent mute and per-owner toast batching are implemented.
Dedicated Codex delivery workers and execution-time wake-permission checks are implemented.
Batch answering passed live GUI checks for edited choices, replies and required numeric forms, including
disabled Send on invalid input and atomic delivery of all answers.
Owner-name proposals now require explicit Desk approval and passed a live reject/approve MCP/GUI test;
renaming preserves asks, answers, caps and preferences, with cross-owner claims refused.
The installed supervisor recovers from a killed daemon in 2 seconds; install, MCP-open upgrade and uninstall
passed on a throwaway channel. Open-Desk upgrade now passes; the 72-hour soak remains pending.
Mute, unmute, Not needed, its count and draft retention passed live GUI checks; live-client navigation remains
unverified. Notification registration is rechecked per toast and flagged on the Desk. The full ten-minute CPU/reopen probe now
passes after a foreground-permission fix (0.276% of one core; 10–14 ms window, 23–61 ms Inbox afterward).
See [the verification log](docs/PRODUCTION-READINESS.md) for commands, baseline and remaining evidence.
The release is not yet ready; the rest of this checklist remains open unless explicitly recorded there.
The independent security review found two medium Windows issues; fixes now use absolute OS utility paths,
private instance locks and protected discovery of fresh pipe addresses. Regression and compatibility tests
pass. Per-user scheduler task names, verified legacy migration, live open-Desk upgrade coordination and
mixed-version MCP exchanges now pass. A follow-up review found two rollback races; both were fixed and
covered by live regression fixtures. The follow-up review found no remaining actionable security issues.

## From Artem's first use (2026-10-18; PLAN.md v4, "Status, and Artem's feedback")
1. **The Desk opens fast.** His words: "It took a while for the window to load."
   - Instrument process start, window shown, first frame and the first `List` answer.
   - Targets on this PC, cold: the window within 300 ms of the click, and the Inbox filled within 1 s.
   - Measure the suspects before changing anything: glow/OpenGL start-up, accesskit, the font atlas, and anything
     that runs before the first frame.
   - If a cold start can't meet the target, keep one warm Desk process that the daemon starts at sign-in, and the tray
     shows and hides it. Test it against egui #5229 and #8459. A hidden warm Desk must still list the Inbox (which
     marks asks shown) only while its window is visible and focused, or it would start default clocks unseen.
2. **Open chat: jump to the agent that asked.** His words: "a way to navigate to the correct chat (for the agent that
   sent the request either to the specific Claude code chat (in the desktop application) or codex chat)". Add an
   **Open chat** button to every ask and every Agents row.
   - **Claude desktop sessions:** the MCP server builds the link itself, `claude://claude.ai/epitaxy/local_<uuid>`,
     from the host's `CLAUDE_CODE_HOST_SESSION_ID` (`local_<uuid>`), and doesn't trust a link an agent passes.
     - First check that MCP server processes inherit that variable.
     - `get_session("self").link` gives the same link, but the app leaves it out when the organisation turns app
       links off.
     - A desktop session is recognised by the presence of the host id, not by `CLAUDE_CODE_SESSION_ID`: that
       variable is set in desktop sessions too, with a different UUID.
   - **Claude Code CLI** (no host id): "Resume in a terminal" runs `claude --resume <CLAUDE_CODE_SESSION_ID>`.
   - **Codex:** first verify that Codex really sends `_meta.threadId`; main reads it, but nobody has checked. Then use
     the app's deep link, if it has one (research it), or else `codex resume <thread id>`.
   - **Terminal launch rules (security):**
     - The command is built only from ids the Desk validates (UUID or hex), never from agent text.
     - The working folder may be the agent's `cwd` only if it is an existing absolute local folder containing no `;`,
       quotes or control characters. Otherwise use the user's home folder.
     - Pass everything as separate arguments (`wt.exe -d <folder> -- claude --resume <id>`), never as a shell
       string: `wt.exe` treats `;` as a command separator.
   - **Links** keep the existing link filter's character checks for `claude://`, and for `codex://` if added.
3. **Remove the daily budget, the digest and the fair share.** His words: "not sure about the inbox budget per day,
   that doesn't seem like a reasonable metric to measure."
   - Every ask goes straight to the Inbox, sorted by priority, then deadline.
   - Keep the owner cap (3 open asks), dedupe, defaults and deadlines.
   - Toasts: at most one per owner per 10 minutes, the rest folded into "N more waiting". P2 asks toast only when
     due within 2 hours, as today (rules.rs).
   - Add a **"Not needed"** answer for any ask (the agent learns it shouldn't have asked; the Desk counts it per
     owner) and **"Mute this agent"** (its asks land silently at the bottom).
   - Remove the Digest tab and the `daily_budget` setting.
   - P0 stays as a priority with nothing to skip, and a provisional owner's P0 is still stored as P1.
   - Update the tests, the tool descriptions and the README's "Noise control" paragraph to match.

## M1 checks not yet run
1. **Clients:** Claude Desktop and Codex list the tools, then ask and answer live; only Claude Code has run live. Test
   both result branches (`content` text and `structuredContent`) in each client.
   The exact synthetic-chat plan is in `docs/LIVE-CLIENT-CHECKS.md`. Desk ask `ask_8f5aedf0b2f55197` requests
   explicit authorization to create/message those temporary app chats, as the chat tools require.
   Artem authorized these tests at 21:32 UTC on 2026-10-18. Codex's real metadata, tools and both result branches
   pass; Claude Desktop and visible Open chat navigation remain unverified. See the checklist's results.
2. **Codex delivery:** `codex queue --thread --message` (CLI 0.160) on an active, an idle and a finished thread, run
   from the daemon. Confirm the resumed turn's sandbox and approval policy equal the original's. Until confirmed, only
   owners marked codex-interactive on the Desk get it. Active and loaded-idle cases now pass, with matching
   original/resumed policies. A completed unloaded chat did not resume despite queue exit 0; an archived chat
   rejected the command. Both answers remained durable. Keep the opt-in restriction and documented limitation.
   The implementation audit also fixed successful queue attempts removing answers from no-ID checkpoint
   reads. Queue acceptance and confirmed relay delivery are now separate, including database migration.
3. **The sign-in task:** `/Run` from Anode and recovery from a killed worker pass. The isolated 72-hour
   `PT0S` probe started 2026-10-18 19:53 UTC but was interrupted by a logged tray Quit at 20:45 UTC.
   It does not count as a pass; a fresh uninterrupted run on the final candidate is required. The stopped
   fixture remains installed because automatic approval review rejected its cleanup (“blocked by policy”).
   See `docs/SOAK-2026-10-18.json`; do not restart or count later uptime toward that interrupted run.
   A fresh candidate run started at 21:56:49 UTC on 2026-10-18, channel `soak-7e4d081-1018`, in Anode session 4.
   It was stopped at Artem's implementation-first instruction; its fixture uninstalled and purged successfully.
   The hourly monitor is paused. See `docs/SOAK-CANDIDATE-7e4d081.json`; this is not a reliability pass.
4. **Idle CPU:** the extracted `e7e1ff0` candidate passed at 0.318% of one core over 600 seconds, including
   supervisor, daemon and hidden Desk. Five subsequent reopen cycles took 13–16 ms window / 30–59 ms Inbox.
   The repeat on `7e4d081` was cancelled after 488 seconds at Artem's implementation-first instruction.
   Its fixture was purged. Repeat against the final implementation during release validation.
5. **Toasts live:** they appear silently. A button answers through the `agentbolt://` handler with its single-use
   token, a used or wrong-option token is refused, and the body click opens the Desk.
   Anode session 4 rejects toast emission with Windows error 0x803E0105 (notification platform unavailable)
   and has no isolated audio endpoint. A normal-session check remains necessary; this is not a pass.
   The prepared checklist is `docs/NORMAL-DESKTOP-CHECKS.md`; Artem answered `ask_97958c01f34e55e5` ready
   at 22:00 UTC. His subsequent implementation-first instruction defers this check. No test ran on `main`.
6. **The native remake's scenarios** (`native/verification.md` on the `legacy-native-2026-10-18` tag), run against
   Agentbolt. Mapped in `docs/NATIVE-SCENARIO-AUDIT.md`; fresh GUI checks now pass focus preservation, blank
   required validation, 860 × 620 layout, one restored Desk/draft, exact numeric submission with untouched
   optional boolean omitted, and a zero-TCP/UDP snapshot. Host/toast rows retain their explicit limitations.

## Known gaps
- **Done — restart on failure:** installed supervision recovered a killed worker in 2 seconds without a client
  start. Children are contained before they run. The interrupted 72-hour check must be replaced.
- **Resolved — day boundary:** daily budgets, digest and fair-share day rollover were removed per Artem's feedback;
  the former DST calculation no longer controls admission.
- **Done — blocking threads:** Codex deliveries have two dedicated workers and a bounded queue, with execution-time
  permission checks. Actual active/idle/finished-host delivery checks remain above.
- **Done — batch answering and owner proposals:** live GUI/MCP fixtures verify edited batches, atomic answers,
  proposal rejection/approval, preserved state and refusal of cross-owner claims.
- **Done — open-Desk upgrade:** the installer prepares every verified installed Desk before replacing
  files, waits for actual old processes and reopens visible windows in their original sessions. Live warm-Desk
  and cross-session tests preserve drafts and exchange MCP requests through an already-open old client.
  Third-file copy failure restores all original files without stopping existing processes. An older Desk
  without lifecycle support is refused before mutation. Startup and changed-task rollback races are covered.
- **Done — handler-key check:** the Desk displays it and the daemon rechecks registration before every toast.
  A live installed fixture now verifies the warning appears after a changed command and clears after repair.
- **Done — formatting:** isolated commit `729fc82`; format and lint checks pass.
- **Done — Windows scheduler isolation:** task names include the token's user SID. Start, stop, delete and
  migration verify the account, executable, arguments and working folder; changed task snapshots are refused.

## Release work
- **Deferred by Artem (Desk answer, 2026-10-18 20:33 UTC):** code signing may wait until after 0.2.
  Label the 0.2 candidate unsigned; do not claim signed-build validation.
- **Prepared and locally verified — release tooling:** version 0.2.0, Installed Apps registration, a locked-build
  ZIP packager, checksums, cargo-about notices and a tag/manual workflow that creates a draft only. Both ZIPs,
  official schemas, winget validation and workflow lint pass. The extracted Windows ZIP passes live install,
  upgrade, rollback, held-MCP and uninstall tests; the Linux ZIP starts and lists the five MCP tools.
  Hosted execution and attestations remain unverified. Nothing has been tagged or published.
- **Prepared — distribution manifests:** winget, Scoop and an MCP Registry custom-installation entry. Scoop's
  hooks pass a live isolated install/update/uninstall harness; Scoop itself and hosted downloads have not run.
  Public Registry acceptance and Glama scoring remain unmeasured, pending an authorized public release.
- **Done — Claude Code plugin and behavioral eval:** strict validation passes. Three repeated publishing
  runs each call `desk_ask` exactly once with `reversible: false` and report pending approval; three routine
  calculation runs answer directly with no ask. Fixed mocks keep tests off the real Desk. The no-plugin
  comparison and full evidence are in `docs/PLUGIN-EVAL-2026-10-18.json`. The release includes a plugin ZIP.
- **Done — independent security review:** the original two medium Windows findings and two subsequent upgrade
  rollback findings were fixed and tested. Follow-up review of lifecycle, task migration, rollback and uninstall
  found no remaining actionable issue. Review later changes if they affect these boundaries.
- **Original handoff boundary, superseded by Artem's full-plan instruction above:** M2 to M7 in PLAN.md (answers back to Claude sessions, Compass use, holds, the
  agent Board, phone answers, other machines).

## Rules that stay
See AGENTS.md:
- test channels only;
- no MCP tool may answer;
- no `outputSchema`;
- the repo guard;
- never read MCP Agent Mail.
