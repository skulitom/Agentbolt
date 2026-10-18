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

The ten-minute CPU sample measured **0.323% of one core combined** (daemon 0.065%, Desk 0.258%,
600.18 seconds). CPU passed, but the following reopen probe failed to refresh the Inbox; investigation is
still open. This longer test is not recorded as an overall pass.

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
Core regression tests and all-target lint pass; the new controls still need live GUI verification.
Per-owner toast batching is the remaining implementation part of the third feedback item.
