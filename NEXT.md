# Next: from 0.1 to production

The design and milestones are in Compass's `work/2026-10-18-agentbolt/PLAN.md` (v4). 0.1 is M0 plus most of M1. This
list is what's left before a public release. Do the first section first.

## From Artem's first use (2026-10-18; PLAN.md v4, "Status, and Artem's feedback")
1. **The Desk opens fast.** His words: "It took a while for the window to load."
   - Instrument process start, window shown, first frame and the first `List` answer.
   - Targets on this PC, cold: the window within 300 ms of the click, and the Inbox filled within 1 s.
   - Measure the suspects before changing anything: glow/OpenGL start-up, accesskit, the font atlas, and anything
     that runs before the first frame.
   - If a cold start can't meet the target, keep one warm Desk process that the daemon starts at sign-in, and the tray
     shows and hides it. Test it against egui #5229 and #8459.
2. **Open chat: jump to the agent that asked.** His words: "a way to navigate to the correct chat (for the agent that
   sent the request either to the specific Claude code chat (in the desktop application) or codex chat)". Add an
   **Open chat** button to every ask and every Agents row.
   - **Claude desktop sessions:** the `claude://` link the agent passes as a new `desk_register` field,
     `session_link`. In the desktop app it comes from `get_session("self")`, as the `link` field.
   - **Claude Code CLI:** "Resume in a terminal": Windows Terminal in the agent's cwd running
     `claude --resume <CLAUDE_CODE_SESSION_ID>`.
   - **Codex:** the app's deep link, if it has one (research it first). Otherwise `codex resume <_meta.threadId>`.
   - **Rules:**
     - `session_link` is display-only, never identity, and only the `claude://` (and `codex://`) schemes are
       accepted.
     - Terminal commands are built from validated ids only (UUID or hex), never from agent text.
3. **Remove the daily budget, the digest and the fair share.** His words: "not sure about the inbox budget per day,
   that doesn't seem like a reasonable metric to measure."
   - Every ask goes straight to the Inbox, sorted by priority, then deadline.
   - Keep the owner cap (3 open asks), dedupe, defaults and deadlines.
   - Toasts: at most one per owner per 10 minutes, the rest folded into "N more waiting", and no toast for P2.
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
3. **The sign-in task:** it survives 72 hours (ExecutionTimeLimit PT0S), and `schtasks /Run` works from an Anode seat
   session.
4. **Idle CPU** under 0.5% with the Desk closed, measured over 10 minutes.
5. **Toasts live:** they appear silently. A button answers through the `agentbolt://` handler with its single-use
   token, a used or wrong-option token is refused, and the body click opens the Desk.
6. **The native remake's scenarios** (`native/verification.md` on the `legacy-native-2026-10-18` tag), run against
   Agentbolt.

## Known gaps
- **Restart on failure:** Task Scheduler restarts only when the task fails to launch, so a daemon crash isn't
  restarted until the next `agentbolt mcp` call runs `schtasks /Run`. Add a watchdog, or restart from the tray process.
- **Day boundary:** a fixed UTC offset, refreshed hourly. Make it DST-aware.
- **Blocking threads:** the daemon has 16, and a `codex queue` delivery can hold one for up to 10 minutes. Give
  deliveries their own small pool.
- **Batch answering** in the Desk, and owner names an agent proposes for Artem to approve (`requested_owner`).
- **The Desk during an upgrade:** a window left open is refused by the new daemon until it's reopened.
- **The handler-key check** shows in the tray tooltip only; also flag it on the Desk.
- **`cargo fmt`:** the tree isn't formatted yet; format it in one commit with no other changes.

## Release work
- Code signing for the three exes (unsigned builds trigger SmartScreen).
- A release workflow: build on tag, zip, SHA256SUMS, attestations, and `THIRD_PARTY_NOTICES.md` from cargo-about.
- Distribution: winget and Scoop manifests, an MCP Registry entry (the MCP front-end starts on Linux, so Glama can
  score it), and a Claude Code plugin with an eval (an agent asked "check with me before you publish" uses
  `desk_ask`).
- A security review of the pipes, tokens and installer by someone new.
- M2 to M7 in PLAN.md (answers back to Claude sessions, Compass use, holds, the agent Board, phone answers, other
  machines).

## Rules that stay
See AGENTS.md:
- test channels only;
- no MCP tool may answer;
- no `outputSchema`;
- the repo guard;
- never read MCP Agent Mail.
