# Next: from 0.1 to production

The design and milestones are in Compass's `work/2026-10-18-agentbolt/PLAN.md` (v3). 0.1 is M0 plus most of M1. This
list is what's left before a public release.

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
