# Find and open a session

Choose **Sessions** in the Desk's main navigation. Search by keywords from chat messages, titles, project
folders, apps or session IDs. Use the Codex/Claude filters to narrow the list. **Open chat** opens a desktop conversation directly;
Claude Code terminal sessions offer **Resume in a terminal**. Extra reference/copy actions are under
Details. No Agentbolt registration is needed for these local sessions.

**Running now** is first, with a slow amber background sweep and a text label on each running card.
**Other open chats** and **Recent chats** start collapsed every time you return to Sessions. Typing a
search replaces those groups with visible, ranked matches from running, idle and recent chats. Clearing
the search returns to the quiet grouped view. View includes an optional background-agent filter. The page refreshes every two
seconds while visible, and Refresh requests an immediate update.

## Keyword search

Use words in any order, such as `invoice compass`. All words must match, but they can match different
fields: the message and its project, for example. Partial words work while typing. In titles and
project metadata, small spelling mistakes are accepted for words of four or more characters, with approximate matches labelled
“Similar wording.” Exact and whole-title matches rank above partial and approximate matches. A match
label explains whether the title, project, folder, app, session ID or message matched. Message matches
show a short excerpt and use exact words/prefixes, not typo correction or semantic similarity.

Use quotes for an exact phrase (`"window icon"`), or `-word` to exclude a term. Exclusions and quoted
phrases do not use spelling correction. Search is case-insensitive and treats punctuation as word
separators. It accepts up to 256 characters and 16 terms. Results show 50 at a time with Show more
matches; activity status and Open chat remain visible on every result.

Ranking and discovery run on the session worker, with a short typing debounce and stale-query results
discarded from the view. Messages start indexing when a nonempty search is entered. The result count
is provisional while “Searching local messages” shows progress. Live message sources are checked
about every 15 seconds after the initial scan; the session activity view still refreshes every two.
Messages, queries, and the SQLite search index stay in memory (including temporary sorting). Agentbolt
does not upload them or persist another copy. Clearing the query stops further message indexing.
Discovery and indexing also stop scheduling new batches when the Desk is hidden, minimized, unfocused,
or showing another tab. A batch already in flight may finish; its result is retained without starting
another. Returning to focused Sessions requests fresh activity metadata even if an older batch is still
finishing. Progress spinners, search-progress repaints and running-card animations use the same activity
gate. Closing the Desk process releases the cache.

Metadata covers the latest 5,000 unarchived Codex chats plus older open chats, and retained Claude
session registry entries. Message search also finds up to 5,000 local Claude transcript files, including
chats absent from the live registry. Those chats use **Resume in a terminal** when no verified Desktop
destination is available. The ordinary Recent chats disclosure shows up to 200 registry/metadata entries
across the selected apps. Archived Codex chats and deleted or inaccessible provider data are outside
this view.

Message indexing is bounded to keep the Desk responsive: up to 1,000 recent Codex message items,
512 KiB of message text per chat, the last 8 MiB of each Claude transcript, and 64 MiB of message text
overall. Codex items over 1 MiB cannot be parsed and are skipped. The UI reports incomplete coverage
under Discovery details when a bound or source-read failure occurs; no match is not proof that a term
never occurred outside the indexed history. Recent sources are processed first within the inventory.

## Running, open and recent

- Codex is Running only when the chat has a held writer lock and its latest turn reports `inProgress`,
  has a start time and has no completion time. Completed, interrupted and failed turns are idle.
  Merely keeping the chat loaded no longer puts it in the main list. Names match the app's saved name.
- Claude is Running only when its process creation time and executable still match and the registry
  reports `busy`, `working` or `running`. Idle, waiting and unrecognized statuses never animate.
- Other open chats contains idle/waiting sessions and sessions whose running status cannot be read.
  Missing metadata is not treated as proof of active work. The app's reported in-progress turn may
  include time waiting for a tool; the animation is an activity indicator, not a progress percentage.
- Recent contains entries no longer confirmed open, capped at 200 in the ordinary browsing view.
  Search includes older entries within the discovery bounds above.

The shimmer follows Windows' animation preference. Reduced motion keeps a static amber surface and
Running label. Animation requests stop for offscreen cards, unfocused/minimized windows and snapshots
more than ten seconds old. These statuses do not create Inbox questions or imply approval is needed.

## Scope and privacy

This version discovers sessions on this Windows computer: Codex, Claude Code and Code chats hosted by
Claude Desktop. It does not discover browser conversations, arbitrary other chat apps, Claude Cowork
VM sessions or sessions on another computer. Existing remote Agentbolt agents remain in Agents, with
their source-machine copy controls. A terminal resume starts a new terminal rather than focusing the
old terminal. Opening a chat sends no message and answers no question.

Discovery reads app-owned session metadata, titles and project paths. On Artem's request, keyword
search also reads local message history. Only visible user/assistant text is indexed; reasoning,
tool payloads, attachments and credential files are excluded. Nothing is sent off the computer. A dedicated worker
keeps discovery separate from the UI and answer pipe. Provider failures remain visible under Discovery
details rather than being represented as proof that every session is closed.

Codex metadata comes from the newest numbered `state_*.sqlite` database under `CODEX_HOME` (default
`%USERPROFILE%\.codex`), opened read-only with a 150 ms busy timeout. Native lock checks are nonblocking,
create no files and never remove locks. A shared coordination guard prevents the probe from racing
Codex's own writer creation/removal. The ownership interpretation follows Codex's
[writer-lock implementation](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/writer_lock.rs).
Running-state lookup reads only status, start and completion columns from the latest `thread_turns`
row in the newest `thread_history_*.sqlite` database. Message search reads only `userMessage` and
`agentMessage` item types from `thread_items` in that same database, opened read-only.

Claude metadata comes from `CLAUDE_CONFIG_DIR\sessions\<pid>.json` (default
`%USERPROFILE%\.claude\sessions`). Process IDs alone are insufficient: Windows creation time and
executable identity are also checked. Older or changed registry formats may not expose all sessions.
Message search reads validated UUID-named `.jsonl` files one directory below `CLAUDE_CONFIG_DIR/projects`;
only user/assistant text blocks enter the index. It never follows transcript-supplied file paths.
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
