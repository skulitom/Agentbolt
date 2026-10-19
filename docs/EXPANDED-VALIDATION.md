# Expanded feature checks — 2026-10-19

Scope: the full M2–M7 implementation on top of `53a44e7`, after recording the named feature inventory.
These are focused local checks, not a new release candidate or production-readiness claim. Earlier
packaging, live-client, idle and installer reports describe older code.

## Passed on Windows

| Check | Result |
|---|---|
| `cargo test -p agentbolt-core --quiet` | 90 unit tests and two repository-guard tests pass. |
| `cargo test -p agentbolt --lib --quiet` | 36 tests pass. |
| `cargo test -p agentbolt --test mcp_stdio --quiet` | Six checks pass, including both protocol discovery paths, twelve tools, no output schema and the size limit. |
| `cargo test -p agentbolt-desk --bin agentbolt-desk --quiet` | 19 non-GUI unit tests pass, including draft persistence, view rules and upgrade state. |
| `cargo test -p agentbolt-daemon holds::tests --quiet` | Five scanner checks pass against temporary local Git repositories and boundary fixtures. |
| `cargo clippy --workspace -- -D warnings` | All production crates pass. |

Commands used throwaway `expanded-core-1019`, `expanded-cli-1019`, `expanded-scanner-1019` or
`expanded-desk-1019`/`expanded-check-1019` channels. Core tests use in-memory stores. MCP subprocesses generate their own
unique test channels. The Git and notes fixtures use temporary directories and remove them; no remote
fetch, installed daemon, real notes file, phone service, SSH connection or main-channel action was used.

## Behavior covered

- Twenty agents race for one Board claim: exactly one wins. Expiry permits a new fence, stale renewal
  and completion fail, and an identical terminal retry is idempotent. Owner caps span handles;
  activity renews only the calling handle, and lowering a lease ceiling expires existing work correctly.
- Routed messages, deduplication, references and cursor traversal across filtered pages; partial profile
  updates preserve omitted fields. Timed mute expires and indefinite mute persists.
- Inbox pagination starts default clocks only for shown pages. Answer, History, relay, delivery and
  directory pages retain older matching records. Closed-draft reconciliation preserves unknown IDs.
- Collision checks examine more than 50 holds while returning bounded details; stale scans remain
  unknown. Glob matching handles component boundaries, Unicode and deep paths without recursion.
- Phone receipts cannot answer through MCP or before Desk consumption; two taps do not create two
  answers, replay is idempotent, a Desk answer wins over a queued receipt, and settings changes revoke
  old authority while allowing a newly issued notification. Schema 12 upgrades preserve legacy receipts.
- Adoption reporting uses the fixed first fourteen days and submitted-question noise denominator.
- SSH argument grammar and MCP-only preflight refuse answer operations and caller-forged machine
  registration. Retry classification does not replay claims, completion, cancellation or recorded checks.
  Fragmented relay lines respect size and completeness limits. No SSH transport session was tested.
- Beads imports select one exact issue, preserve scope and acceptance criteria, and refuse ambiguous,
  closed, oversized or invalid input. The `bd show --id` syntax was checked against the
  [upstream command](https://github.com/gastownhall/beads/blob/main/cmd/bd/show.go); no live Beads database was accessed.
- Notes append preserves UTF-8 and existing content, retries an identical handoff without duplication,
  and refuses locked, non-UTF-8 or oversized writes without changing the file.
- PR paths are either preserved exactly or conservatively replaced by a whole-repository check.
  Truncated/non-UTF-8 output fails; count, aggregate-size, whitespace and glob-character cases are covered.
- Real Git scanning finds dirty/untracked/renamed/unpushed paths, preserves the index and HEAD,
  holds submodule descendants without opening their checkout, and reports unknown for wrong origin,
  missing upstream, excluded subtrees and a non-root source.

## Fixes found during these checks

1. Phone deduplication used the question ID, preventing a fresh notification from accepting a reply after
   settings invalidated an earlier receipt. Migration 13 adds notification identity to receipts; dedupe
   is now per notification, with conservative handling of legacy pending rows.
2. Notes could grow past the read limit, preventing recovery if the delivery mark failed afterward.
   Append now checks the resulting size before writing.
3. PR filenames with leading/trailing whitespace could be normalized into different paths. The adapter
   now checks the complete path set round trip, falling back to `**` when any representation changes
   or the aggregate bound is exceeded. Scanner paths with literal backslashes become unknown.
4. The scanner now checks directory entries without following links before asking Git for status.
   Symbolic links/reparse points, unlisted nested repositories, unreadable entries or a boundary walk
   over 100,000 entries/two seconds make the result unknown. Indexed submodules are skipped and held
   conservatively. Git metadata links are refused before Git runs. This is not a sandbox against a
   same-user process racing directory replacement; that remains part of Agentbolt's stated trust limit.
5. Claude's client-command expectation now includes its ten-minute wait. MCP integration expectations
   cover twelve tools. The enlarged Activity response is boxed in the Desk worker, and new Clippy
   findings in History and link rendering are fixed.

## Still unverified

Automatic approval review rejected the temporary Windows junction creation/removal check with
“blocked by policy” before execution. It was not retried through another mechanism. Actual junction
and symbolic-link fixtures remain unrun; the regular-directory boundary cases pass.

Linux, expanded UI interaction/draft persistence, installed answer-pipe checks, real phone ACLs and
offline transport, SSH reconnect/uncertain writes, live Claude/plugin evaluation and workflow rollout
remain open. The bounded scanner's behavior on large real repositories still needs measurement.
No new packages, licensing run, native/live-host check, CPU probe, reliability run or adoption period
was performed here. Signing remains deferred for 0.2. No publication occurred.

## Transport, platform and lifecycle follow-up

The next pass starts from `4efa083`. The earlier paragraph above describes the first pass only.

| Check | Result |
|---|---|
| Windows `cargo test --workspace --locked --quiet`, `AGENTBOLT_CHANNEL=expanded-workspace-1019`, `AGENTBOLT_INSTALL_TEST=0` | Suite passes. GUI/startup/live-toast tests remain ignored, and opt-in installer/scheduler cases were not activated. Core 90, CLI 37, daemon 39 and Desk 19 unit tests pass, together with default protocol/IPC/headless process/guard/packager checks. |
| WSL Ubuntu, Rust 1.98.1: `cargo test -p agentbolt-core -p agentbolt -p agentbolt-ipc --locked --quiet` | 145 tests pass before the added SSH-capacity test: core 90, guard two, CLI 34, MCP stdio six, IPC 13. |
| WSL `cargo test -p agentbolt-daemon --bin agentbolt-daemon --locked --quiet` | 32 daemon unit tests pass, including the five new phone HTTP mock checks and five Git scanner cases. |
| Windows and WSL `cargo test -p agentbolt --lib remote::tests --locked --quiet` | Five SSH boundary/capacity tests pass on each platform. No remote SSH account is contacted. |
| Windows `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass. |
| WSL `cargo clippy -p agentbolt-core -p agentbolt -p agentbolt-ipc -p agentbolt-daemon --all-targets --locked -- -D warnings` | Pass after compiling the Windows-only toast helper only on Windows or in tests. |
| Anode session 4: `cargo test -p agentbolt-desk --test gui lifecycle_handshake_preserves_offline_drafts_and_closes_warm_windows -- --ignored --nocapture --test-threads=1` | Pass in 2.15 s on debug binaries. Both ordinary and warm Desk preserve offline drafts, reject another transaction's close/cancel and close on their own lifecycle request. Channel `gui-upgrade-114996`; afterward no matching window or channel data directory remains. Lease released. This is not a startup performance measurement. |

Linux builds used `/tmp/agentbolt-expanded-target` and throwaway `expanded-linux-*` channels. HTTP mock
tests use ephemeral loopback listeners and synthetic credentials; private functions accept that fixture,
while normal saved phone settings still reject plaintext HTTP. No account or machine configuration changed.

The phone mocks verify anonymous and authenticated checks on both topics, no auth on anonymous probes,
separate daemon/action credentials, low-priority JSON publications, exact reply cursor use, redirect
refusal and redacted errors. Wrong topics/IDs, malformed or interrupted responses, reported truncation,
more than 500 events and bodies over two MiB return errors without a checkpoint. This validates the
HTTP contract against the [ntfy publishing](https://docs.ntfy.sh/publish/) and
[subscription](https://docs.ntfy.sh/subscribe/api/) interfaces, not a real service's ACLs or TLS.

The SSH review found that the four-entry idle pool did not bound concurrent child processes. The relay
now limits active calls to four, with at most three long answer waits. A fourth wait or fifth active call
is refused before connecting/submitting; releasing or cancelling a call frees capacity. This does not
replace actual cross-machine authentication, reconnection and uncertain-write validation.

Remaining work includes expanded native tab/paging interaction, final-candidate installer/upgrade and
performance checks, live Claude/plugin/phone/SSH validation, packaging and a new reliability run.
The previous junction fixture rejection remains unresolved. No new licensing run or adoption period
was started; no release was tagged or published.

## Native expanded-Desk pass

This pass starts from `ce533ed` and uses Anode session 4 at 1280×720, with a 900×620 Desk
client area and debug binaries. `crates/agentbolt-desk/tests/expanded_gui.rs` is an ignored,
self-cleaning fixture with 60 owners/agents, open questions, archived questions, handoffs, messages
and advisory agent holds; one archived question has 55 synthetic delivery records. It seeds the
store before starting the daemon, then gives the daemon sole ownership. No working copies,
phone service, chat wake permissions, installation registrations or real accounts are configured.

Run in Anode with `cargo test -p agentbolt-desk --test expanded_gui -- --ignored --nocapture --test-threads=1`.
Inspect the tabs and pages, enter `Keep across pages` on Inbox question 00, navigate away and back,
and send that reply to finish. The fixture verifies a Desk-originated answer through the MCP pipe,
then stops its processes and removes its fresh channel directory. It times out after 15 minutes.

Observed through the native window:

- Inbox page two shows questions 50–59; Previous restores the edited question-00 reply.
  The reply also survives visits to History, Board, Work holds, Activity and Agents.
- History reaches its second page and finds `Archived decision 00` with search. The delivery
  browser reaches all 55 records, with the final five on page two and Older disabled at the end.
- Board handoffs, messages and active agents reach their second pages. The closed filter reveals
  the failed handoff and its result. Separate section buttons keep messages and agent controls
  accessible without scrolling through the other lists.
- Work holds reaches page two. Configuration is a directly selectable view; its empty allowlist
  stays empty. Agents directory paging and the Activity totals (120 submitted, 60 answered) render.
- After the follow-up fix, the failed handoff displays `Claimed by fixture-59` even though that
  agent is outside the first 50 directory entries. The Activity period before the first stored
  event displays Unknown instead of zeros; the current synthetic period retains its counts.

Three successful fixture runs used `gui-expanded-109892` (132.10 s), `gui-expanded-75868`
(159.06 s), and `gui-expanded-117496` (69.25 s). The final run rebuilt both daemon and Desk to
verify the claimant and missing-history fixes. No matching windows or data directories remain,
and the desktop lease was released. These durations include manual interaction and are not
performance measurements. Expansion was checked with pointer input; Anode's accessibility
Expand action did not expand an egui collapsing header, so that accessibility path is not proven.

The pass led to these changes:

- Fixed Board navigation above independently scrolling Handoffs, Messages and Who is working;
  page/search changes use their own scroll state. The closed filter now explicitly names failed work.
- Direct Active holds / Configure working copies navigation, with an explicit scanning-off state.
- Delivery summary limited to the last three records, with the complete browser immediately
  accessible and its paging controls outside a bounded list.
- Expandable Activity methodology and owner breakdown, keeping the baseline and verdict reachable.
- Exact claimant cards for the visible handoffs, independent of the agent page or liveness filter.
  The card includes public identity and chat context, never the bearer handle; lookup remains bounded
  by the 50 visible requests. Older daemon snapshots remain readable with an empty card default.
- Periods wholly before the first event have no known history span. The Desk renders their measures
  as Unknown and keeps a brief explanation visible even when methodology is collapsed.

Focused verification after these changes: Windows core 92 unit tests plus two repository guards,
Desk 19 unit tests plus four headless checks pass. Opt-in GUI/startup cases remain ignored in the
ordinary command. Core/Desk all-target Clippy passes. WSL Ubuntu passes all 17 expanded service
tests, including the new off-page/inactive claimant and missing-history regressions. The native
fixture's successful end checks are in addition to those ordinary tests.

This covers synthetic native navigation, not every large-data mutation, keyboard/accessibility path,
installed upgrade or live integration. The final-candidate release, security/live-host, actual phone/SSH,
performance and reliability gates above remain open. No licensing run, adoption period or publication
was started by this pass.

## Per-host SSH configuration and encrypted loopback (2026-10-19)

`agentbolt mcp --ssh-config FILE` selects an existing absolute SSH configuration file for that MCP
host, including paths with spaces. It remains a separate local argument and is absent from the remote
command. Strict host verification and the existing noninteractive/forwarding restrictions still override
configuration values. Without this option, SSH keeps its normal configuration discovery.

The opt-in `crates/agentbolt/tests/ssh_loopback.rs` uses Windows OpenSSH 9.5p2 and an ephemeral
127.0.0.1 endpoint built with the already installed Paramiko 2.8.1. It launches the actual relay and
headless daemon against a fresh channel; only the fixture accepts SSH connections. Its exact command
allowlist launches the relay as separate arguments without a shell. No install, real account, shared
SSH setting or firewall is changed. The fixture's synthetic private keys have user-only permissions;
an initial attempt correctly failed because inherited Windows group permissions made them unacceptable
to OpenSSH. Paramiko emits an installed-library deprecation warning; it is not a product dependency.

Final run: `ssh-loop-104740`, 1.52 seconds (after compilation), passes:

- Wrong host and client keys refuse every operation. The test config requests permissive host checking,
  confirming Agentbolt's strict setting wins. A valid key then connects successfully.
- Registrations retain `remote:loopback-a` and `remote:loopback-b`. An explicitly shared owner recovers
  its offline-seeded historical answer and cancellation state through a fresh connection.
- A committed read with its response dropped reconnects once. A committed ask with its response dropped
  reports an uncertain outcome and is forwarded only once. An explicit dedupe retry returns the same ask.
- Three concurrent answer waits leave the fourth connection available for Board reads and cancellation.
  A fourth wait is refused before submission; all three waiting reads observe the cancellation.
- The actual MCP command accepts the configuration path with spaces, initializes over stdio, and
  registers through SSH. Its full JSON text and structured result match, including the durable answer.
- The server closes transports and reaps its relay children before exit. The daemon, keys, temporary
  configuration and channel data are removed. No human answer was submitted over MCP: the historical
  answer was seeded offline before starting the daemon.

Focused checks: Windows CLI 38 unit tests, Linux CLI 36 unit tests, Windows repository guards two;
CLI all-target Clippy on Windows and Linux. The opt-in fixture is ignored by ordinary test commands.
This proves a real encrypted local transport and process integration. It does not prove an external SSH
account, remote shell quoting, cross-machine operating-system combinations, a deployed phone service,
or the final-candidate reliability gates. No licensing, packaging or long-running check was started.

## Remote references keep their original computer (2026-10-19)

Source review found that remote chat controls were protected, but evidence paths and app chat links in
questions and Board messages still used local launch controls. The Desk now treats these as copyable
references when their origin is remote or unknown. HTTPS references retain their existing browser action.
Remote question headers and Board messages show the originating computer. Local references with known
origin retain their Explorer/chat controls. Older unknown-origin records explain the copy behavior.

Schema migration 14 adds nullable `board_messages.sender_machine`. New posts snapshot the registered
sender's machine in their transaction; retries return the existing post without replacing its source.
Both MCP reads and Desk pages include that snapshot, independent of a sender still being registered.
Legacy rows and older wire messages keep an unknown origin instead of inferring one from a shared owner.

Focused verification: 94 core tests pass on Windows and Linux, including legacy-message migration,
cross-machine dedupe and sender deletion. The Desk's 20 binary unit tests pass, including the evidence
classification and a filesystem-probe closure that must never run for remote/unknown references, even
when the cache previously held the same ask as local. Two repository guards and core/Desk Windows
all-target Clippy pass; Linux core all-target Clippy passes. These changes have not yet had a native UI
or installed-upgrade check. No external integration, main-channel change or release packaging occurred.

## Claude plugin recovery and coordination (2026-10-19)

The decision skill now describes exact-ID answer recovery, durable host identity, bounded waits and
pending-state handling. The coordination skill handles existing requests without duplicating them,
requires successful fenced completion, and preserves supplied result wording. The `board_done` tool's
schema also asks for verbatim supplied results, preserving limits and qualifiers; the saved mock catalog
was regenerated from that same source.

Claude Code 2.1.302 ran six cases with three plugin runs and three comparison runs per case. The fixtures
use the real saved tool schemas and fixed, case-specific MCP responses, following the
[official plugin evaluation format](https://code.claude.com/docs/en/plugin-evals). Only mocked tools are
available in each case; no real MCP server, Desk or shell/publishing tool is started. Model and semantic
grader calls use the normal Claude account. Reports stayed local.

| Scenario | Observed plugin behavior in three runs |
|---|---|
| Calculation | Returns 120 minutes, invokes neither skill and asks no question. |
| Publishing approval | Sends one approval request and keeps the action pending without a human answer. |
| Saved answer | Reads the supplied ask ID, uses its human Python choice and does not ask again. |
| Existing handoff | Claims and completes the supplied ID with fence 42; creates no replacement or human question. See the wording correction below. |
| Stale work scan | Checks `src/retry.rs` for PR opening and treats unknown scan state as no clearance. |
| Pending answer | Uses the same ask ID with a bounded positive wait; no answer remains pending without a duplicate question. |

The suite completed in 581 seconds. All 18 plugin runs passed the runner's checks, with mean score 1.0
and reported comparison delta +0.4333. The comparison has no plugin tools, so that difference does not
isolate the effect of skill wording. Reported list-price cost was $2.5129558, not an account billing claim.

Manual review nevertheless found one handoff result changing “three retries” to “three attempts.” The
automated semantic grader had accepted it. That result was not accepted as clean evidence. The skill and
tool description were tightened to preserve supplied facts verbatim, and the result grader now explicitly
distinguishes retries from total attempts. A focused three-plus-three rerun completed in 85 seconds;
all three plugin runs retained the supplied review facts exactly and used fence 42. Its reported cost
was $0.4110306. The original finding, actual tool inputs/final replies, source/report hashes and corrected
reruns remain in [the evidence record](PLUGIN-EVAL-2026-10-19.json).

An earlier single-run probe exposed an unnecessarily prescriptive grader requiring an extra read before
claiming a known ID. It was replaced with a check against duplicate handoff creation; the ownership,
fence, result-content and no-question requirements remain. This adjustment is recorded in the evidence.

Windows CLI's 38 unit tests and all-target Clippy pass, including the compact tool-catalog budget. Strict
plugin validation and the tracked-file repository guard are checked alongside the final documentation.
The wait fixtures return immediately: they prove call arguments and pending-state behavior, not a real
ten-minute host wait. Saved-answer mocks do not prove backend persistence. Live Claude navigation/waits,
dispatcher rollout and real PR workflow integration remain open. No licensing or release packaging ran.

Automatic approval review rejected cleanup of the 44 evaluation temporary folders as “blocked by policy”
before execution. No alternate cleanup was attempted. Their exact paths are retained in the evidence
record; raw reports and HTML reports also remain in ignored local output folders.

## SSH accounts using PowerShell (2026-10-19)

The remote command previously began with a quoted executable, which is valid in sh/Command Prompt but
is a string expression in PowerShell. `agentbolt mcp --remote-shell powershell` now selects the required
call operator. `sh` remains the default; `cmd` explicitly selects the same existing invocation syntax.
The setting describes the remote SSH account's shell, not the local client OS. Unsupported shell names
and shell settings without `--ssh` fail before connecting. All modes retain the restricted executable,
channel and machine grammar; shell selection cannot inject arbitrary commands. Connection errors now
include the shell setting among the configuration items to check.

Focused checks pass: Windows CLI 38 unit tests, Linux CLI 36 unit tests, and the new `remote_shells`
fixture on both platforms. That fixture copies the actual CLI into a temporary path containing spaces
and launches its relay through Windows Command Prompt and Windows PowerShell, or `/bin/sh` on WSL.
The relay sees empty input, exits successfully without stdout/stderr, and creates no daemon data.
CLI all-target Clippy passes on Windows and Linux; both repository guard checks pass on Windows.
The existing encrypted SSH fixture was updated for the new config
field; its prior transport evidence is unchanged, and it was not rerun for this local command change.

A separate local PowerShell protocol check used the real headless daemon on `shell-probe-122036`.
Registration and a Board directory read preserved `Shell round trip: café — 雪 🚀` through UTF-8 stdin
and stdout. Both launched processes exited, descendants were reaped, and channel data was removed.
This did not register a task, protocol handler or install, and did not contact an SSH account or main.
Actual cross-machine SSH shell/profile behavior remains a rollout check; local shell success does not
prove that external configuration. No licensing, packaging or long reliability run was started.

## Phone privacy checks after a clock change (2026-10-19)

Both the phone worker's one-minute refresh and the core publication gate accepted future-dated
protection checks after a backward clock adjustment. They now share a check requiring nonnegative,
bounded age; overflow also fails closed. A fresh successful topic check restores normal delivery.

Five focused phone service tests pass on Windows and Linux, including the new clock rollback and exact-expiry
case. It verifies that no phone token is minted while the previous privacy check is in the future,
that a fresh check restores notification preparation, and that the one-minute refresh/two-minute
publication boundaries expire exactly. Core/daemon all-target Clippy passes on Windows. These checks
use an in-memory store and a controlled clock; the computer's clock and phone configuration were not
changed, and no external service was contacted.

## Scanner metadata and executable configuration (2026-10-19)

The [expanded security review](SECURITY-EXPANDED-2026-10-19.md) confirmed two P2 issues: a configured
Git content filter could execute during background status collection, and `.git` file indirection could
read metadata in an excluded root. The scanner now validates `.git`/`commondir` destinations, metadata
entries and bounded local/worktree configuration before any repository Git command. Linked worktrees
remain supported when both their working folder and the main checkout owning their metadata are allowed.

Configuration parsing happens in an empty temporary directory with repository discovery isolated and
includes disabled. Filter definitions/includes, alternate object stores, grafts and partial clones
return unknown. Inherited Git path/config/trace overrides, global/system configuration, external
attributes/excludes, hooks, fsmonitor and fetching are disabled. A separate attribute read refuses
tracked files naming content filters, including filters whose definition would come from elsewhere.
No target repository configuration is rewritten. Unsupported scans cannot provide clearance.

Twelve focused scanner tests pass on Windows and thirteen on Linux, including Linux metadata symlinks.
The checks cover both reported reproductions and allowed linked-worktree behavior, unchanged indexes,
dirty/untracked/rename/unpushed paths, gitlinks, configuration includes/worktree filters, attribute-only
filters, inherited config/trace injection, excluded/unlisted metadata and alternate/partial objects.
Daemon all-target Clippy passes on both platforms. Tests use synthetic temporary repositories and a
throwaway channel; no installation, real working-copy configuration, private folder, phone or SSH
service was changed. Actual Windows junction/permission fixtures, same-user path replacement races and
installed/live integration checks are not established by this evidence.

## One-click adoption measurement (2026-10-19)

A fresh full-plan audit found that the 80% one-click criterion had no measurement in Activity. The Desk
now records whether an answer was submitted without note/reply/form edits, preserving that evidence in
saved drafts. Untouched prefilled answers can qualify; edits remain edits even if cleared. Reviewed
batches and automatic defaults do not count as one-click answers. Notification/phone answers count only
when the installed Desk commits them. The existing human-answer boundary and MCP tool catalog are unchanged.

Activity shows direct-answer counts and share against questions first shown during each period,
including older backlog, using only answers recorded before its end. This denominator is intentionally
different from the submitted-question cohorts used by the other measures. Schema migration 15 leaves
old answer evidence null; older drafts, old clients and saved baselines remain unmeasured rather than
being assigned an inferred rate. A percentage is withheld when relevant human answers lack evidence.
The [methodology](ADOPTION.md) describes this operational definition and its limits.

All 99 core tests and both repository guards pass on Windows and Linux. New checks exercise shown versus
unseen questions, direct/edited/batch/defaulted/notification/legacy responses, phone commitment, the
period-end boundary, older backlog, missing baseline fields, legacy wire shape and migration from schema
14. The Desk's 22 binary unit tests pass, including retained draft edits and distinct unknown/not-recorded
display states. Windows workspace all-target Clippy and Linux core/CLI all-target Clippy pass.

These are local service, migration and headless checks. The new Activity row and edit tracking have not
yet had a native interaction check, and schema 15 has not yet had an installed upgrade check. No real
baseline/adoption period was started, and no main-channel install, external account, licensing, package
or long reliability run was changed.
