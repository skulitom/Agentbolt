# Find and open a session

Choose **Sessions** in the Desk's main navigation. Search by chat title, project folder, app or session
ID. Use the Codex/Claude filters to narrow the list. **Open chat** opens a desktop conversation directly;
Claude Code terminal sessions offer **Resume in a terminal**. Extra reference/copy actions are under
Details. No Agentbolt registration is needed for these local sessions.

**Running now** is first, with a slow amber background sweep and a text label on each running card.
**Other open chats** and **Recent chats** start collapsed every time you return to Sessions, including
while searching. View includes an optional background-agent filter. The page refreshes every two
seconds while visible, and Refresh requests an immediate update.

## Running, open and recent

- Codex is Running only when the chat has a held writer lock and its latest turn reports `inProgress`,
  has a start time and has no completion time. Completed, interrupted and failed turns are idle.
  Merely keeping the chat loaded no longer puts it in the main list. Names match the app's saved name.
- Claude is Running only when its process creation time and executable still match and the registry
  reports `busy`, `working` or `running`. Idle, waiting and unrecognized statuses never animate.
- Other open chats contains idle/waiting sessions and sessions whose running status cannot be read.
  Missing metadata is not treated as proof of active work. The app's reported in-progress turn may
  include time waiting for a tool; the animation is an activity indicator, not a progress percentage.
- Recent includes the latest 200 unarchived Codex chats plus retained Claude session entries that are
  no longer confirmed open. Open Codex chats older than those 200 are included separately.

The shimmer follows Windows' animation preference. Reduced motion keeps a static amber surface and
Running label. Animation requests stop for offscreen cards, unfocused/minimized windows and snapshots
more than ten seconds old. These statuses do not create Inbox questions or imply approval is needed.

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
Running-state lookup reads only status, start and completion columns from the latest `thread_turns`
row in the newest `thread_history_*.sqlite` database. No thread items, transcripts or messages are read.

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
