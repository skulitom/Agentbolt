# Find and open a session

Choose **Sessions** in the Desk's main navigation. Search by chat title, project folder, app or session
ID. Use the Codex/Claude filters to narrow the list. **Open chat** opens a desktop conversation directly;
Claude Code terminal sessions offer **Resume in a terminal**. Extra reference/copy actions are under
Details. No Agentbolt registration is needed for these local sessions.

Open sessions are first. Recent chats start collapsed every time you return to Sessions, including
while searching. View includes an optional background-agent filter. The page refreshes every five
seconds while visible, and Refresh requests an immediate update.

## What “Open” means

- Codex has the chat loaded and holds its writer lock. This does not prove a turn is running or that
  it needs a reply. Names match the app's saved chat name when available.
- Claude Code's session registry identifies a process whose creation time and executable still match.
  Claude's own idle/waiting/working label is shown where available. These labels do not create Inbox
  questions or imply that a waiting session requires your answer.
- Recent includes the latest 200 unarchived Codex chats plus retained Claude session entries that are
  no longer confirmed open. Open Codex chats older than those 200 are included separately.

## Scope and privacy

This version discovers sessions on this Windows computer: Codex, Claude Code and Code chats hosted by
Claude Desktop. It does not discover browser conversations, arbitrary other chat apps, Claude Cowork
VM sessions or sessions on another computer. Existing remote Agentbolt agents remain in Agents, with
their source-machine copy controls. A terminal resume starts a new terminal rather than focusing the
old terminal. Opening a chat sends no message and answers no question.

Discovery reads app-owned session metadata, titles and project paths. It does not read conversation
transcripts, credentials or message history, and sends nothing off the computer. A dedicated worker
keeps discovery separate from the UI and answer pipe. Provider failures remain visible under Discovery
details rather than being represented as proof that every session is closed.

Codex metadata comes from the newest numbered `state_*.sqlite` database under `CODEX_HOME` (default
`%USERPROFILE%\.codex`), opened read-only with a 150 ms busy timeout. Native lock checks are nonblocking,
create no files and never remove locks. A shared coordination guard prevents the probe from racing
Codex's own writer creation/removal. The ownership interpretation follows Codex's
[writer-lock implementation](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/writer_lock.rs).

Claude metadata comes from `CLAUDE_CONFIG_DIR\sessions\<pid>.json` (default
`%USERPROFILE%\.claude\sessions`). Process IDs alone are insufficient: Windows creation time and
executable identity are also checked. Older or changed registry formats may not expose all sessions.
The providers are adapters to local app formats, so future app changes may require an adapter update.
Network and linked/reparse provider paths are not supported. Provider entries and file reads are bounded.

Desktop destinations are rebuilt from validated session IDs. Raw metadata cannot select an arbitrary
URL or executable. Terminal launch continues through the existing validated argument-array path.
On non-Windows platforms Claude liveness is not inferred; Windows is the supported desktop target.

## Development isolation

Non-main channels do not discover real sessions by default. An explicit
`AGENTBOLT_SESSION_PREVIEW_ROOT` supplies a disposable profile containing `.codex` and `.claude` folders
for layout work. The release-testing pause remains in effect; adding this view does not resume
live-client navigation or reliability testing.
