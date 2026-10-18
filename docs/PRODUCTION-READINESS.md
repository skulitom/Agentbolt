# Agentbolt 0.2 release verification

Work started from `main` at `bcb60f2`, in `C:\DEV\Agentbolt`. The active release scope is the v4
feedback, remaining M1 checks and release preparation in `NEXT.md`; M2 and later are outside this release.
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
