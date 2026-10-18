# Agentbolt 0.2 release verification

Work started from `main` at `bcb60f2`, in `C:\DEV\Agentbolt`. The original scope covered v4 feedback,
M1 and release preparation. Artem subsequently expanded implementation to the full M2–M7 plan, with
features first. The results below are historical M0/M1 evidence. Current expanded checks are in
[EXPANDED-VALIDATION.md](EXPANDED-VALIDATION.md), and the current scope is in `NEXT.md`.
This is an evidence log, not a statement that the release is ready. No tag or public release has been made.

## Baseline (2026-10-18)

- Windows: `AGENTBOLT_CHANNEL=codex-02-unit cargo test --workspace --locked` passed; the real installer
  remained opt-in and the live toast test was ignored. This does not prove those manual checks.
- Formatting was isolated in commit `729fc82`, before implementation changes.
- Linux/WSL Ubuntu, Rust 1.98.1: the full `cargo test -p agentbolt-core -p agentbolt --locked` exposed a
  Windows-only install-folder assertion in the MCP library tests. After limiting those folder assertions
  to Windows, all 95 Linux tests passed (22 CLI/MCP library, 6 stdio protocol, 65 core, 2 guard).
  Previous CI ran only the MCP stdio tests, so it had missed this failure.

## Desk startup

Measured optimized binaries in Anode's hidden Windows session on this PC. The first launch after building
showed the window in **720.5 ms** and received the Inbox in **744.3 ms**, measured from before process
creation. The following four new-process launches showed in **235.0–256.2 ms** and received the Inbox in
**254.7–287.3 ms**. These are fresh-process measurements, not a claim that the OS disk cache was flushed.

The first launch spent 549.0 ms between entering the native framework and renderer readiness. UI creation
took 1.2 ms and painting followed 6.7 ms later; process creation/loading took about 158 ms before `main`.
The renderer interval includes graphics initialization and accessibility setup; these were not individually
profiled. The 300 ms window target failed on that first launch, motivating the plan's warm-process fallback.

The diagnostic is opt-in: set `AGENTBOLT_STARTUP_LOG` to a writable JSON-lines file. It records process id,
wall timestamp and elapsed times for main entry, native startup, renderer readiness, first UI, first painted
frame, actual window visibility and Inbox responses. It records no ask text or answers. Without that variable,
it creates no trace file or visibility-observer thread.

Repeatable GUI probes (run inside Anode after building all release binaries):

```powershell
$env:AGENTBOLT_CHANNEL = 'startup-probe'
cargo build --workspace --release --locked
cargo test --workspace --release --test startup -- --ignored --nocapture --test-threads=1
```

`cold_startup_measurement` records five fresh-process launches in `target/release/startup-results.json`.
Set `AGENTBOLT_REQUIRE_COLD_TARGET=1` to enforce the cold targets; normal installations use the preloaded
Desk. `warm_desk_reopens_fast_without_showing_unseen_asks` enforces the 300 ms / 1 s targets, checks five
show/hide cycles, checks that hidden asks remain unseen, and checks that killing the daemon closes its Desk.
Each probe creates and removes its own throwaway channel. It does not install a task or protocol handler.

The warm implementation must handle both upstream issues explicitly:
[hidden-window reopening #5229](https://github.com/emilk/egui/issues/5229) and
[initial root visibility #8459](https://github.com/emilk/egui/issues/8459). The initial test caught that blocking
`SWP_SHOWWINDOW` alone was insufficient: winit subsequently writes `WS_VISIBLE` directly. Both native paths
are gated during preload. The Desk uses actual Windows visibility in addition to focus before listing asks.

The warm-process probe passed five consecutive show/hide cycles: **10.8–13.1 ms** to show and
**27.5–58.0 ms** to receive the Inbox. Hidden asks remained unseen on every cycle, and terminating the
daemon removed the warm window. The native visibility checks run throughout preload, not just at its end.
Set `AGENTBOLT_MEASURE_IDLE=1` for the warm probe to additionally measure both processes for ten minutes;
the combined CPU limit is 0.5% of one core (stricter than Task Manager's all-core percentage).

The first ten-minute CPU sample measured **0.323% of one core combined** (daemon 0.065%, Desk 0.258%,
600.18 seconds). CPU passed, but the following reopen probe failed to refresh the Inbox. The launcher now
passes its foreground permission to the warm Desk with `AllowSetForegroundWindow`, allowing the user's
click to focus the resident process without bypassing Windows' focus rules.

The full repeat passed: **0.276% combined** (daemon 0.042%, Desk 0.234%, 600.18 seconds), then all five
reopen cycles passed at **10.1–13.7 ms window / 22.7–61.3 ms Inbox**, including hidden-ask checks and crash
cleanup. This sample includes direct Inbox and the focus fix; later noise-control and delivery changes
will need the final release-candidate performance check too.

## Chat navigation

The Desk now offers validated chat navigation on asks and Agents rows. Claude Desktop links are generated
from `CLAUDE_CODE_HOST_SESSION_ID`, taking precedence over its different CLI session id; Codex uses per-call
thread metadata. Claude CLI resumes with separate Windows Terminal arguments. Invalid ids are refused;
unsafe, missing or nonlocal folders fall back to the user's existing local home directory.
The installed Codex package manifest registers `codex`, and its bundled app source uses
`codex://threads/<threadId>`. CLI fallback is available if opening that protocol fails.

Ask records retain the original client and working directory alongside the session id, even after agent
expiry or a later registration. Schema migration preserves existing asks and answers without guessing
missing historical client context. Both session URL schemes use the same character filter.

Windows workspace tests and all-target lint checks pass. Tests cover malformed identifiers, terminal
argument separation, folder fallback, host-id precedence, durable chat context and a released-schema
upgrade. **Live inheritance of Claude Desktop's host variable, actual Codex `_meta.threadId` calls and
navigation into those real clients remain unverified.**

Sign-in reliability, installed upgrade behavior, live-client navigation/delivery, signing, packaging and
remaining `NEXT.md` items are still pending.

## Direct Inbox

Daily budget, fair-share allocation, daily P0 bypass and the Digest UI are removed. Every accepted ask goes
straight to the Inbox, ordered by priority then deadline. The three-open-ask owner cap, dedupe, deadline
defaults and provisional-owner P0-to-P1 downgrade remain. The obsolete UTC day-boundary code is also removed;
there is no longer a daily limit requiring a DST-aware boundary.

Opening a 0.1 database moves held Digest asks to the Inbox in an audited transaction without marking them
shown. Existing notification settings and answers survive. Legacy wire fields remain for compatibility with
already running 0.1 clients; new asks never enter Digest. Windows workspace tests and all-target lint pass,
including 36 simultaneous Inbox asks, repeated bound-owner P0s, ordering and an upgrade with unseen asks.
Not needed and persistent owner mute are implemented on asks, Agents and owner settings. Not needed
works for every ask kind, approves no option, is counted per owner and revokes outstanding answer tokens.
Mute survives re-registration, suppresses new toasts and orders that owner's asks below unmuted asks.
Core regression tests and all-target lint pass. A live Anode check on `codex02-gui-1018` verified mute on
both asks from one owner, unmute from Settings, note retention when selecting another ask and returning,
Not needed with that note, no option approved, matching MCP result branches and a dismissal count of one.
The test used supplied host metadata; it does not prove real-client metadata or chat navigation.
The owned Desk and headless daemon were stopped. Automatic approval review rejected cleanup of the test
data folder; `C:\Users\artem\AppData\Local\Agentbolt\codex02-gui-1018` remains. No task or handler was registered.
Toast batching is implemented with a persistent ten-minute owner cooldown and one opaque owner tag.
Additional eligible asks replace the Notification Center entry with “N more waiting” and `SuppressPopup`;
the daemon serializes these replacements. P2 asks become eligible at two hours before their deadline, even
without another MCP call. Tests cover the exact cooldown boundary, separate owners, daemon restart,
no repeated tick alerts and the existing notification's buttons answering only its displayed ask.
Windows workspace tests and all-target lint pass. Live toast rendering/activation remains unverified.

## Notification registration health

The daemon now rechecks the complete protocol command before each toast, including the executable,
channel and URL argument. A changed registration receives no fresh button tokens. Inbox and status
responses carry the current warning to the Desk, which explains how to repair notification actions;
repair clears it on the next refresh. Regression tests reject wrong programs, channels, placeholders and
extra arguments. Live registration-change/repair verification remains pending.

## Delivery isolation

Codex queue work now has two dedicated workers and a bounded 32-job waiting queue, independent of the
daemon's request/store workers. Saturation records an unsuccessful delivery; the durable answer remains
available through `desk_answers`. A blocked-worker test verifies the concurrency limit, queue bound and
continued availability of request workers.

Delivery also requires a validated Codex host thread, rather than any session belonging to an owner with
Codex wake enabled. Permission is checked again immediately before execution, so revoking it on the Desk
also affects waiting deliveries. Regression tests cover revocation, wrong client type and malformed ids.
Actual active/idle/finished-thread delivery and sandbox-policy preservation remain unverified.

## Batch answering

The Inbox supports selecting up to 32 asks and reviewing editable choices, replies, forms and notes together.
The final Send answers action is explicit. The Desk pipe commits the batch atomically; an invalid answer,
duplicate id or already-closed ask rolls the entire batch back. MCP has no batch-answer operation. Tests
cover stale batches, duplicate/oversized/empty inputs, rollback and durable per-ask Desk authority. Windows
workspace tests and lint pass. The release `tests/gui.rs` fixture passed through Anode: select three asks,
change the choice to Yes, edit a reply and change a numeric form from 5 to 7. Clearing the required quantity
disabled Send. All three edits arrived atomically with Desk authority. The fixture's processes and data
were removed; the missing-handler warning was also visible on the Desk.

After changing child containment, the warm reopen probe passed five cycles at **10.5–13.1 ms window /
27.0–57.9 ms Inbox**, including hidden-ask safety and crash cleanup. One earlier probe was cancelled after
an optional test switch incorrectly treated `0` as enabled; both measurement switches now require `1`.
Automatic approval review blocked deletion of that interrupted probe's data folder,
`C:\Users\artem\AppData\Local\Agentbolt\warm-95688`. Its processes are stopped. This and the earlier
`codex02-gui-1018` folder are the two known manual-cleanup leftovers.

## Installed crash probe and licenses

An actual release install was started by `schtasks /Run` from Anode session 4. Windows selected the user's
console session 1 for `InteractiveToken`; pipe status worked. Terminating only the verified test daemon
with exit code 1 did **not** cause a restart within 95 seconds. This confirms the reported restart gap on
this PC despite `RestartOnFailure` in the stored task. The test cleanup removed the installation, task,
registrations, shortcut and data.

The sign-in task now runs `--supervise`: a small parent restarts failed daemon workers with exponential
backoff (1–30 seconds), resetting after a minute of stable operation. A normal Quit stops it. Both daemon
workers and preloaded Desks are created suspended, assigned to a kill-on-close job, then resumed, closing
the previous spawn/containment race. Stopping the installed task also stops its worker and warm Desk.

The release installer probe now passes: **2.0 seconds to recover from the killed worker without any client
start**, then upgrade with an MCP session open, installed status and complete uninstall/cleanup. Both
installer tests passed in 7.94 seconds. This does not yet cover an open Desk during upgrade or the 72-hour
soak. Workspace tests and all-target lint also pass.

`cargo-deny 0.20.2 check licenses` passes (three harmless unused-license-allowance warnings).

## Owner-name proposals

`desk_register` accepts `requested_owner`. The daemon keeps the session provisional until the Desk approves
the exact current proposal. The Desk lists it in Settings and Agents and shows a pending-name count on the
Settings tab. Reject clears the proposal. Approval atomically renames the owner, its agents and its asks,
preserving answers, dedupe, caps, mute and toast cooldown. A durable binding lets the same host session
re-register under its approved name after a daemon restart or handle expiry.

Proposals cannot rename a configured/approved owner or join an existing one; intentionally sharing an
existing owner requires the host's `--owner` configuration. Another session cannot claim an approved name.
Stale approvals and collisions fail without changing anything, and approval has no MCP-pipe operation.
No relay or wake permission is added. Without a host session id, approval applies to that MCP process's
identity; a new process needs configuration or its own approval.

Four store regression tests cover authority, isolation, stale/conflicting proposals, preserved state and
restart/expiry. Windows workspace tests (72 core tests) and all-target clippy pass. Linux core/MCP tests
and clippy pass. The release `owner_name_review_through_mcp_and_the_real_desk` fixture passed in Anode:
reject the first proposal, approve the second, then verify re-registration, the retained ask and dedupe.
Every MCP result in this test had matching complete text and structured branches. Its processes and data
were removed. This exercises the real MCP executable with a test client; live Claude/Codex host checks
remain pending.

## Durable unsent drafts

Choices, replies (including deliberately cleared replies), form fields, notes, selected asks and batch
selection are saved locally when they change, using a synced temporary file and atomic replacement.
Loading them never sends an answer. Closed asks are removed from saved drafts on the next Inbox refresh.
Unreadable or future-version files are retained and a visible warning explains that saving is blocked.
Drafts are bounded to 4 MiB. Each Windows session uses its own file, so two sessions do not overwrite each
other; a new session can restore the latest saved state.

Both drafts and egui window preferences now live inside the channel's data folder. New windows are
centered; the live 1280x720 test now fits wholly on screen. Previous builds used egui's default Roaming
folder, so some earlier fixtures left window preferences there despite removing their channel data.
Those legacy preference folders need separate cleanup; never remove the real `Agentbolt` folder or
another task's fixture.

The release batch GUI fixture passed with `AGENTBOLT_DRAFT_RESTART_TEST=1`: edit Yes, Edited reply,
quantity 7 and note Survives restart; forcibly kill the Desk; reopen and visually confirm all edits and
three selected asks; explicitly send. All three answers arrived atomically. Its channel data disappeared
on cleanup and no Roaming preference folder was created. Desk unit tests and workspace clippy pass.
This is draft/crash recovery evidence, **not yet an open-Desk installer-upgrade pass**.

## Running 72-hour scheduler probe

The isolated channel `codex-02-soak-1018` started at **2026-10-18 19:53:02 UTC**, from Anode. Its scheduled
task uses `PT0S` and `--supervise`; Windows selected console session 1. It holds no asks. The binary
snapshot from `b96ddc3`, process ids, hashes and observations are recorded in
[SOAK-2026-10-18.json](SOAK-2026-10-18.json). Do not upgrade or restart this installation during measurement.
An hourly thread heartbeat checks it quietly and will record the result and remove the test installation
after at least 72 hours, no earlier than **2026-10-21 19:54 UTC**. This measures the pinned scheduler build;
it does not replace final release-candidate validation. A restart or reboot invalidates uninterrupted
uptime evidence and must be reported, never silently reset.

## Independent security review and Windows isolation

A fresh reviewer inspected pipes, peer identity, tokens, dispatch, installer replacement and supervision at
`65369ad`. Thirty targeted checks passed, with no token-forgery or MCP answer-writing finding. Two medium
findings were confirmed: bare `schtasks.exe` could resolve beside the downloaded executable, and globally
predictable mutex/pipe names allowed another Windows account to prevent startup.

System commands now use the actual Windows system-directory API (`schtasks`, Explorer); terminal resume
uses the explicit per-user Windows Apps alias. Daemon, supervisor and per-session Desk coordination use
exclusive, user-only profile files, with access-denied reported as a fault. Pipe addresses include the user
SID and a fresh 128-bit nonce on every listener start. A protected, write-locked, bounded discovery file
publishes the nonce; clients still verify the serving executable and the Desk pipe still verifies its client.
No current client falls back to the historical predictable address. Delete-on-close removes coordination
files without a close/delete race. The core crate remains free of Windows dependencies.

A best-effort MCP-only legacy listener lets already-open 0.1 front ends reconnect. If another account or
process occupies that old name, current service still starts and logs a compatibility warning; old sessions
must then be reopened. The legacy listener uses the same user-only DACL and MCP dispatcher, with no answer
operations. Real-daemon tests prove both restart/reconnect on the old wire protocol and current startup
with the legacy address deliberately occupied.

Windows workspace tests and all-target clippy pass. The IPC suite includes 17 tests, including old-address
squatting, fresh addresses after restart, discovery DACL/read bounds and refusal of replacement while live.
Linux core/MCP tests and clippy pass. These fixes have not yet been independently re-reviewed. Scheduler
task names still need per-user qualification and safe migration of the earlier task; open-Desk upgrade
coordination and a full mixed-version installer/MCP exchange remain open.

References: [Rust command resolution](https://doc.rust-lang.org/std/process/struct.Command.html#platform-specific-behavior)
and [Microsoft's single-instance mutex guidance](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexw).

## Desk lifecycle preparation for upgrades

The Desk now exposes a separate per-process lifecycle listener, with only Prepare, Cancel and Close.
It uses private discovery and the same user-only transport; the installer must verify the Desk image
before connecting and keep that connection across executable renaming. No lifecycle operation accepts
an answer, reads the Inbox or weakens the daemon's sibling-Desk identity check.

Preparation acquires the UI's frame lock, pauses editing/polling, refuses unsaved-draft errors, then waits
behind every queued worker operation and closes the old daemon connection. The acknowledgement includes
the exact pid, Windows session, visibility and warm state. Cancellation belongs to its transaction;
abandoned pauses expire after two minutes. A committed Close cannot be cancelled or expire back into
editing, and posts a native close even when a warm Desk is hidden. Loading/reopening still sends nothing.

The release `lifecycle_handshake_preserves_offline_drafts_and_closes_warm_windows` fixture passed in Anode
session 4 in **1.58 seconds**, channel `gui-upgrade-111624`: visible and warm windows, refusal of premature
and wrong-transaction Close, wrong/right cancellation, acknowledged graceful exit, retained choice, note,
reply, numeric form and batch selection. This fixture deliberately has no daemon, proving an offline
Desk can still preserve drafts for an upgrade. Its data folder and legacy Roaming preference folder are
absent after cleanup. Eighteen Desk tests, eighteen Windows IPC tests, workspace all-target clippy and
format checks pass; thirteen Linux IPC tests and core/MCP clippy pass.

**Installer integration is still pending.** Before touching files it must enumerate only verified Desk
processes in this channel's installation, prepare all of them and retain their connections and process
handles. An older Desk with no handshake must cause a clear pre-mutation refusal; it cannot be assumed
to have no drafts merely because it is hidden. Copy failures must cancel preparation. Stopping must wait
for the actual old processes, including legacy pipe users, and fail instead of "carrying on" after a
timeout. Previously visible windows must reopen in their original sessions. The existing installer
fixture must exchange MCP requests after replacement, not merely keep an idle stdio process open.
The full installer/rollback and mixed-version tests, per-user scheduler names with verified legacy-task
migration, and a security review of the completed lifecycle integration remain release gates.

## Installer integration: visible Desk and live MCP continuity

The installer now retains verified process handles and lifecycle connections before touching binaries,
serializes maintenance per user/channel, renews saved pauses after copying, and waits for the actual old
daemon processes after stopping the task. It refuses an older Desk without the handshake before mutation.
A file-replacement journal restores earlier programs on pre-start failures. Once the new daemon can write
the database, failures leave the complete new installation repairable instead of rolling old programs back
under a migrated database. Fault-path coverage is still required.

An acknowledged standalone Desk can launch its replacement in its own Windows session after giving up its
singleton. A visible daemon-owned warm Desk has a short-lived visibility marker consumed by the replacement.
Task Scheduler RunEx restores the daemon in its original session. The installer checks the new visible Desk
through its verified lifecycle endpoint; uninstall closes standalone Desks as well as daemon-owned ones.

Two release integration runs passed on fresh channels, with no registration/data/install-folder leftovers:

- `insttest-1572813f95034`: **9.49 s**, warm Desk and daemon in Anode session 4. Retained choice/note drafts,
  visible replacement, post-upgrade MCP registration, retained ask, answer through the new sibling Desk,
  answer retrieval through the held-open MCP process, and uninstall while both clients remained open.
- `insttest-19d5c1f8ab690`: **10.09 s**, standalone visible Desk in session 4 and daemon in session 1.
  Same assertions, with a real pre-isolation CLI copied from the pinned `b96ddc3` source for the MCP process.
  Only the fresh fixture channel ran; the ongoing 72-hour installation was untouched.

MCP assertions parse the complete JSON text from `content` and compare it with `structuredContent` before
using the result. Transient reconnect errors are bounded and retried. Windows all-target clippy, IPC/Desk
unit checks and the release workspace build passed before these runs. Scheduler isolation, fault injection
and a final security review remain. Microsoft's [RunEx session contract](https://learn.microsoft.com/en-us/windows/win32/api/taskschd/nf-taskschd-iregisteredtask-runex)
informed the session-preserving launch.

Artem answered signing ask `ask_c4f58d78a5f04db5` through the Desk at **2026-10-18T20:33:49Z**:
“Signing can be deferred beyond 0.2.” The candidate will be explicitly unsigned; signing is no longer a
0.2 release gate, and no signed-build validation is claimed.

## Per-user scheduler tasks and reviewed rollback boundaries

Task names now include the actual process-token SID. COM inspection verifies the principal's resolved SID,
interactive least-privilege logon and exactly one execution action with the installed daemon, expected
working folder and channel arguments. Start, stop and deletion use the verified definition; mutations
recheck its XML. Historical names are migrated only when verified; unrelated or inaccessible historical
tasks are ignored. A collision on the new scoped name is refused before file replacement. New task XML
uses the token SID instead of environment-provided account names. CLI and Desk share the same verifier.

The independent follow-up review found two upgrade rollback races, now fixed:

1. An automatic task start could open the new database while the installer still intended to roll files
   back. A separate private startup gate is held from preflight through replacement/rollback. Every new
   supervisor, worker and Desk checks it before initialization; workers retain it until store/pipe startup
   completes. The installer releases it only after restoring old files or committing the new installation.
   A refused worker exits with a transient failure, preserving supervision recovery.
2. Rollback could overwrite a task changed since preflight. The installer now journals only its successful
   task mutations. It restores a replacement only while that exact XML remains unchanged, restores a
   removed legacy task only while its name is still absent, and skips untouched preflight snapshots.

The reviewer rechecked these fixes and the surrounding lifecycle/pipe/token/installer code and reported
**no remaining actionable security issue**. That pass was read-only; the following live tests supply the
execution evidence:

- Final release installer suite in Anode: **3 passed in 27.14 s**. Channel `insttest-1bcac26283b30` used a
  visible standalone Desk in session 4, daemon in session 1 and the real older `b96ddc3` MCP binary held open.
  A locked third source forced a copy failure after earlier replacements: every original byte was restored,
  all existing processes stayed alive, the subsequent upgrade reopened the Desk and retained its draft,
  and the held MCP process read a new Desk answer before uninstall cleaned the fixture.
- Channel `insttest-1bcac1d50630c` verified legacy migration, preservation of wrong-image/argument/folder
  historical tasks, and refusal of scoped decoys on install/start/uninstall. During a deliberately delayed
  copy, direct new worker and supervisor launches exited 1 without creating a database. Changing the task
  concurrently caused refusal and rollback preserved the changed XML and restored all program bytes.
- `oldrefuse-1792356768310`: a real pre-lifecycle warm Desk from `b96ddc3` caused pre-mutation refusal;
  all three SHA256 hashes and task XML remained identical. Verified task stop, uninstall/purge and removal
  of that fixture's old Roaming preferences completed. The first harness attempt (`oldrefuse-1792356737782`)
  stopped on PowerShell's stderr handling, before assertions, and also cleaned up; the corrected run passed.
- Four Desk headless tests pass, including refusal during maintenance before any channel data is created.
  The negative main-channel answer check refuses before acquiring a startup gate.

Windows workspace tests, all-target clippy, formatting and the release build pass. Linux core/MCP/IPC tests
and all-target clippy pass. `cargo-deny 0.20.2 check licenses` passes (three pre-existing unused allow-list
warnings). The pinned 72-hour installation and the main installation were untouched.

The same final code also passed a warm Desk/daemon session-4 reinstall and held-MCP exchange in **8.27 s**,
channel `insttest-1502819b34c00`, followed by verified uninstall/purge.

## Reliability run interrupted before 72 hours

Read-only inspection at 2026-10-18 20:54 UTC found the original soak task Ready, last result 0, with no
recorded processes alive. Its daemon log records `quit from the tray menu` at **20:45:27.7271105Z**;
the supervisor records normal exit at **20:45:27.8352749Z**. The source of that tray action is unknown.
The observation is interrupted, not a scheduler failure or a 72-hour pass. No restart was attempted;
the final candidate needs a fresh uninterrupted run. The obsolete hourly monitor is paused.

Automatic approval review rejected the cleanup command for this stopped fixture with **“blocked by policy”**.
Its installation, registrations, data and original logs remain in place; no alternate deletion was attempted.
The two earlier blocked cleanup folders (`codex02-gui-1018` and `warm-95688`) also remain unresolved.

## 0.2.0 release preparation (2026-10-18 21:09 UTC)

The workspace version is now 0.2.0. Installation writes a per-user, per-channel Installed Apps entry after
the rollback boundary, with quoted uninstall commands that retain user data. Uninstall removes that entry.
The release installer fixture passed in Anode session 4 in **8.80 s**, channel `insttest-ad003a9707f4`:
version/commands were checked in the registry, a visible warm Desk retained its draft across reinstall,
the held-open MCP client exchanged an ask and a new Desk answer, and uninstall/purge removed the entry,
task, shortcut, install folder and data. No main-channel change was made.

Windows workspace tests, all-target clippy and the release build pass. Linux core/IPC/MCP and packager tests
and clippy pass. The license policy passes, with the same three unused allow-list warnings; cargo-about
0.9.2 generated the Windows notices successfully. These checks used Rust 1.98.1, now pinned in the release
workflow. GUI suites remain explicitly separate from the headless workspace run.

The new `xtask package` builds all selected executables using Cargo with locked dependencies and an explicit
x64 target, reads their actual paths from Cargo, and checks that the checkout stayed clean at the same revision.
It writes stable-order ZIPs with notices, license and per-binary hashes, and derives winget/Scoop manifests
from the ZIP's real checksum. The MCP Registry entry uses the documented custom-installation guide format.
The packager refuses an existing output directory. Unit tests cover ZIP round trips, executable modes,
repeatability, path traversal refusal and matching manifest checksums.

An independent security pass found one P2 release-integrity issue in the first packager: it could label stale
prebuilt programs with current HEAD. The mandatory locked build and explicit target fix that. Follow-up review
found no unresolved security issue in the installer entry, release workflow or packaging changes.

Local archive/schema validation, package-manager execution and Linux archive generation are still pending
at this checkpoint. The hosted workflow and attestations have not run; no tag, release or distribution
submission has been created. Signing remains explicitly deferred by Artem. Readiness still requires the
live host/toast checks, final idle probe and a new uninterrupted 72-hour run.

### Packaged release validation

Candidate source **`e7e1ff0bfc55a4aa9d4e355af90ac7cf29ce1bbf`** was packaged on both native targets with
locked dependencies. Review files are under `target/candidate-current/` (ignored, not published):

| Archive | Bytes | SHA-256 |
|---|---:|---|
| `windows-x64/agentbolt-0.2.0-windows-x64.zip` | 8,019,716 | `9faaf0728ec4be3600e23a3325ecc52b15bf4dcf5db91ffd4800785623953af8` |
| `linux-x64/agentbolt-0.2.0-linux-x64.zip` | 1,184,949 | `36303a87f7d2a8958e9c331bcd8a779eafc1658889e09c0162a6f377ebd0753c` |

An independent archive reader checked every checksum, the ZIP CRCs, flat filenames, per-binary hashes,
PE/ELF x64 architecture, Linux executable permissions, version/revision metadata and nonempty notices.
Python jsonschema and PowerShell Test-Json both accepted all three generated manifests against their official
schemas. Winget 1.29.380 reports **Manifest validation succeeded**, with no warnings after adding its schema
header. Actionlint 1.7.12 accepts the release workflow. Both targets' cargo-about notices generated with
`--locked --fail`; Linux emits expected warnings for unused Windows clarification entries.

The actual extracted Windows installer passed **3 live tests in 26.91 s** in Anode session 4. Channel
`insttest-9db826da15a8` preserved an open old MCP client and visible warm Desk drafts, restored every original
binary after a forced third-file copy failure, then upgraded and delivered a new Desk answer. Channel
`insttest-9db830856940` covered scheduler migration, foreign-task preservation, scoped decoys, the startup fence
and changed-task rollback. Both fixtures uninstalled/purged successfully, including Installed Apps entries.

The extracted Linux binary reports 0.2.0, initializes MCP and lists all five tools without `outputSchema`.
This check starts no daemon and makes no ask. Its temporary extraction was removed.

The generated Scoop hooks were executed verbatim against the extracted Windows ZIP in an isolated harness,
channel `scoophook-1792358142697`, **5.88 s**. The install hook completed, update's pre-uninstall hook left the
daemon running, and uninstall removed the programs/registrations while retaining a data sentinel. Explicit
test cleanup then purged that data. Scoop itself is not installed; this is hook execution evidence, not a
claim that a live public bucket/download was exercised. All these new fixtures were cleaned successfully.

### Claude plugin behavior

The 0.2.0 plugin carries a configurable installed-binary path and channel, the five real MCP tool schemas,
and a narrowly triggered decision skill. Strict Claude validation and the skill validator pass. The release
packager includes only tracked plugin sources, plus the license, in a separate version-matched ZIP.

Claude Code **2.1.302** ran the final mocked evaluation at **2026-10-18 21:30:11 UTC**, taking **144 s**:
two cases, three repetitions each, with and without the plugin (12 runs total). All six with-plugin runs
passed. Each publishing run made exactly one irreversible `desk_ask` and left approval pending; all three
calculation runs returned 120 minutes with no question. The publishing score was 1.0 versus 0.5 without the
plugin; calculation scored 1.0 in both arms. The comparison measures the whole plugin, including tool access.
The CLI reported $0.7815243 in list-price cost, not a statement of the account's actual bill.

An earlier run exposed an inaccurate explanation that an irreversible ask defaults to No at its deadline.
The skill now states that irreversible asks never default, and the semantic grader checks this distinction.
Fixed mocks report no human answer without inventing a pending-ID list. The final traces confirm the correction.
No real MCP server or publishing tool was granted; reports were kept local with `--no-publish`.
`docs/PLUGIN-EVAL-2026-10-18.json` records evaluated source hashes, actual tool inputs, grader results and trace
hashes. This does not substitute for the remaining live Desktop-host checks.

Automatic approval review rejected removal of the previous evaluation's 12 temporary roots with “blocked by
policy”; that command did not execute. Those `claude-eval-*` roots listed in `target/plugin-eval-reviewed.json`
remain. No alternate deletion was attempted. The final run's retained traces are also kept for review.

### Live notification registration and session limitation

The extracted Windows candidate ran on installed channel `toast-1792359272773` in Anode session 4.
A real MCP ask reached the Inbox, but Windows rejected toast emission with **0x803E0105, notification
platform unavailable**. Anode also reported no isolated audio endpoint. Therefore toast appearance,
audible silence and actual notification-button/body activation remain unverified in a normal session.
The Windows XML parser and core single-use/wrong-option token tests remain separate evidence.

Changing this fixture's actual handler command in the daemon's Windows session made the Desk display
“Notification actions need repair”; restoring the saved command cleared the warning on the next refresh.
The shell host and Anode initially observed different values for that HKCU path, so the effective mutation
and repair were both performed and observed inside Anode. Logs are retained in `target/toast-fixture.log`.
The normal installer removed the fixture's programs, data, task, shortcut and registrations in Anode.
Automatic approval review rejected removal of the remaining test-only registration in the shell host's
registry view with “blocked by policy”; `HKCU\Software\Classes\agentbolt-toast-1792359272773` remains in
that view with its original command restored. No alternate removal was attempted.

The independent static security pass over the plugin, archive path validation and startup-test changes
found no actionable issue. It did not execute live tests.

### Final-candidate idle and real Codex observations

The extracted `e7e1ff0` Desk, daemon and supervisor remained alive throughout **600.182 s** with the Desk
hidden. Combined CPU was **0.317612% of one core** (Desk 0.257734%, daemon 0.059878%, supervisor 0%).
Five subsequent reopen cycles took **13.020–16.047 ms** for the visible window and **30.189–58.925 ms**
for the Inbox. Unseen-ask/default-clock and crash-cleanup assertions passed. The test now verifies actual
candidate process identities and measures all three retained process handles, rather than silently treating
an exited process as zero CPU. Its first invocation exposed a test-path mismatch, fixed before this passing run.

Real Codex Desktop 0.163.0-alpha.10.2 supplies `_meta.threadId`, lists all five tools without `outputSchema`,
and exposes both result branches. Native CLI 0.160.0 invoked by the daemon successfully delivered to an idle
loaded chat and queued the next turn of an active chat. Original and resumed turn records preserve the same
sandbox and approval policy. Completed unloaded and archived cases have limits described in
`LIVE-CLIENT-CHECKS.md`; neither is counted as an automatic-wake pass. The test chats were archived, the local
MCP override removed and the test installation successfully purged.

The GUI inspection also found long provisional session names wrapping into ten-line rows on Agents.
Giving that column a useful minimum width fixes the cause while retaining horizontal scrolling for small
windows. Because this changes the Desk binary, the final archive needs its own repeated idle/reopen check.

`NATIVE-SCENARIO-AUDIT.md` maps the old verification scenarios to current evidence and remaining GUI checks.
The added twelve answer/cancel races pass on Windows and Linux, including durable reopen and repeat reads.

The native audit exposed a form regression: an untouched optional boolean was serialized as false.
The Desk now represents it as an optional value and offers Not answered / Yes / No. Tests check omission,
explicit false and true, defaults, required-field validation, legacy Bool draft compatibility and persistence
of unanswered/explicit-false drafts. All 19 Desk unit tests and all-target Desk clippy pass. The subsequent
`7e4d081` GUI fixture also passed omission, draft restart, focus retention and the 860 × 620 layout checks;
see `CANDIDATE-7e4d081.json` and `NATIVE-SCENARIO-AUDIT.md`.

### Implementation-first correction

At 22:01 UTC Artem requested all implementation before heavy testing. The current ten-minute probe was
cancelled after 488 seconds without a completed result. The new 72-hour fixture was stopped and its monitor
paused; both fresh fixtures uninstalled and purged successfully. Neither is counted as a pass. The earlier
cleanup rejections recorded above remain unchanged. Manual desktop testing is deferred despite the earlier
ready answer. Existing ZIPs remain historical snapshots, not the final build after these changes.

The handoff-to-code audit is in `IMPLEMENTATION-AUDIT.md`. It found a lost-checkpoint bug: queue exit 0
suppressed an answer from normal reads even when the host did not wake. Queue acceptance is now distinct
from confirmed relay delivery, with migration retaining genuine relay confirmations. It also found muted
owners included in phone counts and queued toasts bypassing later mute/pause/answer changes; both paths
now consult current state. Digest fields/text were removed from internal notification code.

Focused Windows regressions pass for checkpoint recovery after queue success, migration of released data
without changing its event chain, notification mute/pause/answer rechecks, ntfy counts/rate limiting and tray
text. Repository guard, formatting and core/daemon all-target clippy pass. The first new notification test
used the wrong option for its synthetic question; correcting that fixture to Blue made it pass. Full-suite,
Linux and packaged validation are deferred to the final implementation as requested.
