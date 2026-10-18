# Next: from 0.1 to production

The design and milestones are in Compass's `work/2026-10-18-agentbolt/PLAN.md` (v4). 0.1 is M0 plus most of M1. This
list is what's left before a public release. Do the first section first.

**0.2 work in progress:** isolated formatting commit `729fc82`; measured warm Desk startup now passes
five Anode reopen cycles (11–13 ms window, 27–58 ms Inbox), including hidden-ask safety and crash cleanup.
Validated chat navigation, direct Inbox, Not needed, persistent mute and per-owner toast batching are implemented.
Dedicated Codex delivery workers and execution-time wake-permission checks are implemented.
Batch answering passed live GUI checks for edited choices, replies and required numeric forms, including
disabled Send on invalid input and atomic delivery of all answers.
Owner-name proposals now require explicit Desk approval and passed a live reject/approve MCP/GUI test;
renaming preserves asks, answers, caps and preferences, with cross-owner claims refused.
The installed supervisor recovers from a killed daemon in 2 seconds; install, MCP-open upgrade and uninstall
passed on a throwaway channel. Open-Desk upgrade and the 72-hour soak remain pending.
Mute, unmute, Not needed, its count and draft retention passed live GUI checks; live-client navigation remains
unverified. Notification registration is rechecked per toast and flagged on the Desk. The full ten-minute CPU/reopen probe now
passes after a foreground-permission fix (0.276% of one core; 10–14 ms window, 23–61 ms Inbox afterward).
See [the verification log](docs/PRODUCTION-READINESS.md) for commands, baseline and remaining evidence.
The release is not yet ready; the rest of this checklist remains open unless explicitly recorded there.
The independent security review found two medium Windows issues; fixes now use absolute OS utility paths,
private instance locks and protected discovery of fresh pipe addresses. Regression and compatibility tests
pass. Per-user scheduler task names/safe legacy migration and a full mixed-version installer/MCP exchange
remain to do alongside open-Desk upgrade coordination.

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
2. **Codex delivery:** `codex queue --thread --message` (CLI 0.160) on an active, an idle and a finished thread, run
   from the daemon. Confirm the resumed turn's sandbox and approval policy equal the original's. Until confirmed, only
   owners marked codex-interactive on the Desk get it.
3. **The sign-in task:** `/Run` from Anode and recovery from a killed worker pass. The isolated 72-hour
   `PT0S` probe started 2026-10-18 19:53 UTC and cannot finish before 2026-10-21 19:54 UTC; see
   `docs/SOAK-2026-10-18.json`. Do not change that pinned installation during measurement.
4. **Idle CPU:** the earlier warm build passed at 0.276% of one core over ten minutes. Repeat on the final
   candidate, including the supervisor and new lifecycle listener, with the Desk closed.
5. **Toasts live:** they appear silently. A button answers through the `agentbolt://` handler with its single-use
   token, a used or wrong-option token is refused, and the body click opens the Desk.
6. **The native remake's scenarios** (`native/verification.md` on the `legacy-native-2026-10-18` tag), run against
   Agentbolt.

## Known gaps
- **Done — restart on failure:** installed supervision recovered a killed worker in 2 seconds without a client
  start. Children are contained before they run. The 72-hour check is still running.
- **Resolved — day boundary:** daily budgets, digest and fair-share day rollover were removed per Artem's feedback;
  the former DST calculation no longer controls admission.
- **Done — blocking threads:** Codex deliveries have two dedicated workers and a bounded queue, with execution-time
  permission checks. Actual active/idle/finished-host delivery checks remain above.
- **Done — batch answering and owner proposals:** live GUI/MCP fixtures verify edited batches, atomic answers,
  proposal rejection/approval, preserved state and refusal of cross-owner claims.
- **Implemented — open-Desk upgrade:** the installer prepares every verified installed Desk before replacing
  files, waits for actual old processes and reopens visible windows in their original sessions. Live warm-Desk
  and cross-session tests preserve drafts and exchange MCP requests through an already-open old client.
  Fault/rollback coverage and the final security review remain.
- **Done — handler-key check:** the Desk displays it and the daemon rechecks registration before every toast.
  A live registration-change/repair check remains.
- **Done — formatting:** isolated commit `729fc82`; format and lint checks pass.
- **Pending — Windows scheduler isolation:** qualify task names per user and migrate only a verified legacy task.

## Release work
- **Deferred by Artem (Desk answer, 2026-10-18 20:33 UTC):** code signing may wait until after 0.2.
  Label the 0.2 candidate unsigned; do not claim signed-build validation.
- A release workflow: build on tag, zip, SHA256SUMS, attestations, and `THIRD_PARTY_NOTICES.md` from cargo-about.
- Distribution: winget and Scoop manifests, an MCP Registry entry (the MCP front-end starts on Linux, so Glama can
  score it), and a Claude Code plugin with an eval (an agent asked "check with me before you publish" uses
  `desk_ask`).
- **Done — independent security review:** two medium Windows findings were fixed and covered by regression tests.
  The new lifecycle/installer work must also be reviewed before release.
- **Out of scope for this handoff:** M2 to M7 in PLAN.md (answers back to Claude sessions, Compass use, holds, the
  agent Board, phone answers, other machines).

## Rules that stay
See AGENTS.md:
- test channels only;
- no MCP tool may answer;
- no `outputSchema`;
- the repo guard;
- never read MCP Agent Mail.
