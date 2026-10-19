# Next: from 0.1 to production

The design and milestones are in Compass's `work/2026-10-18-agentbolt/PLAN.md` (v4). 0.1 is M0 plus most of M1. This
list is what's left before a public release. Do the first section first.

**Testing paused at Artem's request (2026-10-19): "Stop all testing for now."** Do not start or resume
tests from the older checklists or readiness answers below. The normal-desktop fixture and fresh
`0229139` reliability fixture were uninstalled and purged; the hourly monitor is paused. Completed
results remain evidence, while Claude Desktop/navigation and the uninterrupted 72-hour check remain
unverified. Resume only when Artem asks. His normal desktop app remains updated to 0.2.0.

**UX polish (2026-10-19):** the Desk now has consistent spacing and typography, organized Settings,
readable agent/owner cards, clearer answer controls and a shared window/taskbar/executable icon.
See the [UX implementation and review record](docs/UX-POLISH-2026-10-19.md). This development pass does
not resume the paused release tests. A follow-up Inbox refinement keeps past questions collapsed by
default, separates them from active selection, labels closed outcomes “No reply needed” and makes batch
selection an explicit mode. App-wide decluttering also puts routine permissions, phone setup, delivery
records and detailed reports behind named disclosures, while approvals and problems stay visible.
Secondary pages are available through More; paging controls appear only when needed.
Interaction animations now respect Windows' reduced-motion setting. Session navigation and review
shortcuts are direct, feedback no longer resizes the page, and unsaved notification edits survive
navigation. The new [Sessions view](docs/SESSIONS.md) discovers local Codex and Claude Code sessions
without requiring an Agentbolt connection, with title/project search and collapsed recent chats.
Sessions now separates confirmed running work from loaded-but-idle chats. Only running cards have an
amber background animation; other open chats and recent chats start collapsed. Reduced motion uses
a static highlight. These are UI improvements; the broader testing pause remains in effect.
Keyword search now ranks title/project matches, accepts partial words and small spelling mistakes,
and shows older matching chats directly while searching. Quotes and exclusions narrow results.
Ranking runs off the UI thread. At Artem's request, search also reads local Codex and Claude messages,
with matching excerpts and an in-memory index. [Search scope and bounds](docs/SESSIONS.md) are explicit.

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
`--remote-shell powershell` now supports SSH accounts using PowerShell; Command Prompt and Unix shell
launches retain their existing syntax. Real local shell checks cover paths with spaces and a PowerShell
Unicode registration/Board round trip.
Remote evidence paths and chat links now stay on the originating computer, with copy controls. Board
messages retain their original machine after sender expiry or retries; legacy origins remain unknown.
The Claude plugin now covers saved-answer recovery, bounded waits, fenced handoffs and stale work scans
in six mocked behavioral scenarios, repeated three times with and without the plugin. Manual review caught
a changed numeric meaning in a recorded result; the tool/skill guidance now preserves supplied wording,
and three focused reruns pass. See the [evaluation evidence](docs/PLUGIN-EVAL-2026-10-19.json).
Real Claude Code CLI waiting and fresh-process owner recovery now pass: a ten-minute requested wait
returned the native Desk answer after 53.862 seconds, and a new CLI session recovered it without another
ask. The real host used all twelve tool schemas and matching text/structured results. See
[the live-client evidence](docs/CLAUDE-LIVE-2026-10-19.json). A full wait timeout/background transition,
Claude Desktop navigation and authorized dispatcher/workflow rollout remain open.
Phone delivery now rechecks topic privacy after a backward clock change; a future-dated check cannot
authorize a new notification.
The expanded security review found two scanner issues: configured filter execution and metadata reads
into excluded folders. Both are fixed with metadata/configuration preflight, isolated Git settings
and conservative unknown results for unsupported repositories. Allowed linked worktrees remain supported.
See [the security record](docs/SECURITY-EXPANDED-2026-10-19.md). Final-candidate and live host validation remain open.
The full-plan audit also added the missing one-click adoption measure. Activity now distinguishes direct,
edited/batch and unmeasured answers, includes older backlog first shown during the period, and preserves
unknown evidence in older drafts, answers and saved baselines. See [Activity methodology](docs/ADOPTION.md).
Native interaction checks now confirm direct choice/form/reply/Not needed submissions, retained edits
after a restart or clearing a note, and the Activity total. An installed schema-13-shaped fixture now
migrates to schema 15, preserving old answers, unknown origins/interaction evidence and the exact saved
baseline through reinstall. This does not replace an actual older-release binary upgrade check.
Native origin checks found and fixed missing system clipboard support. File/session copies, Unicode
paste and Activity JSON export now reach the Windows clipboard. Remote/unknown references remain
copy-only. Board and Activity sections now honor native accessibility Expand/Collapse commands;
the same adapter covers recent questions, handoffs, agents and work holds.
No phone account, scan, adoption period or live workflow has been activated.

Local Claude/Codex chats now offer **Copy resume command**, including the recorded folder in its tooltip.
An older question without a source computer stays copy-only even when it retains a session ID. Native
clipboard/origin checks pass. Actual Claude terminal resume encountered this PC's administrator Terminal
profile and remains unverified; no consent or settings change was made. See
[the navigation evidence](docs/CLAUDE-NAVIGATION-2026-10-19.json).

Beads imports now keep different issue IDs separate even when their titles match, and cannot reuse an
unlinked handoff. The adapter invokes Beads with write-command and automatic-push restrictions. A real
Beads 1.3.1 disposable workspace confirms repeat/export deduplication, Unicode preservation and unchanged
source issue data. See [the adapter record](docs/EXPANDED-VALIDATION.md#beads-issue-identity-and-real-cli-import-2026-10-19).


**Implementation order remains all features first, then validation.** The full-plan source inventory
is recorded and the expanded local/package checks below have run. The latest testing pause above
overrides the earlier validation instructions; do not restart either interrupted reliability fixture.
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
   On 2026-10-19 Artem reported duplicate instances: this old task had started again at Windows sign-in.
   Its exact task is now disabled and stopped, and its Start-menu shortcut is parked under
   `target/duplicate-instance-1019`. Only the main Desk and its normal helpers remain running. The old
   fixture's files/data are retained; do not re-enable its task or count this uptime as validation.
   See `docs/SOAK-2026-10-18.json`; do not restart or count later uptime toward that interrupted run.
   A fresh candidate run started at 21:56:49 UTC on 2026-10-18, channel `soak-7e4d081-1018`, in Anode session 4.
   It was stopped at Artem's implementation-first instruction; its fixture uninstalled and purged successfully.
   The hourly monitor is paused. See `docs/SOAK-CANDIDATE-7e4d081.json`; this is not a reliability pass.
   The expanded `0229139` run started 2026-10-19 11:15:31 UTC in Anode, then stopped a few minutes later
   when Artem requested all testing stop. Its task, registrations, programs and data were removed;
   see [the stopped run](docs/SOAK-CANDIDATE-0229139.json). It is not a pass and must not be resumed.
4. **Idle CPU and warm opening pass on the final expanded archive:** `0229139` measured
   **0.437% of one core over 600.186 seconds**, including supervisor, daemon and hidden Desk.
   Five reopen cycles took **15.18–36.76 ms window / 52.46–78.41 ms Inbox**. Hidden-question/default-clock
   and child-cleanup checks passed. [Pinned evidence](docs/CANDIDATE-0229139.json).
5. **Toasts live:** they appear silently. A button answers through the `agentbolt://` handler with its single-use
   token, a used or wrong-option token is refused, and the body click opens the Desk.
   **Normal-session appearance, silence, body and button checks pass on `0229139`.** Artem confirmed
   the notification was silent and opened the test Desk; a later direct Blue button produced `via: toast`.
   The first attempt exposed different effective registry views between the shell host and background
   app. Running the same installer in the app's context repaired the test registration. Core tests cover
   replay/wrong-option refusal; those were not replayed live. [Evidence](docs/NORMAL-DESKTOP-2026-10-19.json).
   No synthetic test used `main`. Exact-chat navigation is deferred at Artem's latest request.
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
- **Current packaged review candidate — `0229139`:** Windows, Linux and Claude-plugin ZIPs are in
  `target/dist-0229139`, with checksums, source/binary manifests and complete current notices. Both
  extracted CLIs pass twelve-tool discovery under the 2025 and 2026 protocols without output schemas.
  Official winget/Scoop/MCP Registry schemas, `winget validate` and extracted-plugin strict validation
  pass. [Candidate hashes and scope](docs/CANDIDATE-0229139.json). The first expanded package attempt
  exposed an overlong Registry description; the generator is fixed and these archives replace it.
  Application source is unchanged from the desktop build installed at Artem's request. These exact
  archives now pass installed open-Desk/MCP upgrade, older-schema preservation and clean uninstall in
  Anode on `insttest-186943b533b40` (8.87 seconds), final warm startup/idle checks and normal-session
  silent notification/body/button checks. Remaining live integrations and 72-hour evidence are unverified;
  testing is paused. Nothing was published.
- **Full local preflight passes (2026-10-19):** Windows workspace tests and Linux core/IPC/daemon/MCP/
  packaging tests pass, with all-target Clippy on both platforms. Current target-specific license policy
  and notice reports pass too. The release workflow now rejects generic notice fallbacks and uncovered
  crates, and Linux CI covers the expanded daemon/IPC/packager code. [Scope and exclusions](docs/EXPANDED-VALIDATION.md#full-local-preflight-and-current-notices-2026-10-19).
- **Desktop updated at Artem's request (2026-10-19):** the real installed app was 0.1.0. Its old open
  Desk could not prepare drafts, so the installer refused before replacing files. After Artem confirmed
  he had closed it, the remaining Desk process was closed and a private SQLite backup passed its integrity
  check. The normal installer then upgraded the main installation to the `d33c261` 0.2.0 build, migrated
  its historical sign-in task and started the supervisor, daemon and warm Desk. All three installed hashes
  match the preserved build; status reports an intact event chain, and the existing MCP connection still
  reads its durable answers. This was the requested app update, not a synthetic main-channel test. Older
  connected MCP hosts may need reconnection to discover the expanded tool catalog. Final release checks
  remain outstanding; the original working clones were not changed.
  The later normal-desktop check found the Installed Apps entry visible only in the shell-host registry
  view. Re-running the same preserved `d33c261` installer through Anode repaired the normal Windows
  registration; it now reports 0.2.0 there too. All three program hashes still match the requested build,
  and supervisor, worker and Desk remain in Artem's normal session 1. This completed the requested
  real app update, independently of the synthetic test fixtures.
- **Current implementation build — `d33c261`:** all three Windows release programs and the Linux CLI
  compile from the locked dependency graph. Both CLIs report version 0.2.0. Copies and checksums are preserved
  under `target/prepared-d33c261`; [the build record](docs/BUILD-d33c261.json) pins their source and scope.
  These are raw unsigned binaries, not new release ZIPs. The named feature implementation is present;
  local checks and current notices now pass; next comes packaging, followed by packaged/native/live
  checks and a fresh idle/reliability run. Existing archives below remain historical evidence.
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
