# Expanded feature security review — 2026-10-19

A read-only subagent reviewed the expanded runtime at `65f40b5`, including phone replies, SSH access,
Board/workflow operations and working-copy scans. The subsequent `b9879f8` changed only documentation
and archive preparation. The reviewer established these two issues with source inspection and isolated
Git commands; neither reproduction exercised a running daemon or real private folder.

| Severity | Location (file:line) | Finding |
|---|---|---|
| P2 | `crates/agentbolt-daemon/src/holds.rs:41` (reviewed revision) | Disabling hooks and fsmonitor left clean/process filters executable during status. A synthetic clean filter printed `FILTER_EXECUTED` with the scanner's exact arguments. |
| P2 | `crates/agentbolt-daemon/src/holds.rs:207` (reviewed revision) | An ordinary `.git` file could point at excluded metadata. A separate-Git-directory fixture passed the root check while config/index reads accessed its excluded target. |

Both findings are addressed in `crates/agentbolt-daemon/src/holds/git.rs` and its calling scanner:

- Metadata paths are normalized and checked against the allowlist/exclusions before filesystem access.
  Every path component and bounded metadata-tree walk refuses links, reparse points and special files.
  `.git` and `commondir` pointers may share metadata only within allowed folders for the same repository.
- Git parses bounded local/worktree config in a fresh empty directory, with an explicit empty `GIT_DIR`
  and `--no-includes`. It cannot discover the target checkout during this preflight. Includes, filter
  definitions and additional read/processing indirection are refused before repository Git runs.
- Git children discard inherited `GIT_*` overrides and disable global/system config, external
  attributes/excludes, optional locks, hooks, fsmonitor, replacement objects and network protocols.
  Alternates, grafts and partial-clone objects are refused; lazy fetching is also disabled.
- A no-execution attribute read rejects tracked paths using filters even when the filter would be
  defined elsewhere. Unsupported configurations return an unknown scan, never PR/merge clearance.

The Git behavior underlying this design is documented in
[Git configuration](https://git-scm.com/docs/git-config),
[repository layout](https://git-scm.com/docs/gitrepository-layout) and
[Git environment controls](https://git-scm.com/docs/git). These controls deliberately exclude personal
Git configuration; the scanner can report additional holds or unknown for repositories that interactive
Git handles normally. See [working-copy setup and limitations](BOARD-AND-HOLDS.md).

Focused checks cover dirty/untracked/rename/unpushed files, unchanged indexes, gitlinks, allowlisted
linked worktrees, excluded/unlisted separate metadata, clean/process filters, filter attributes,
local/conditional includes and worktree configuration, alternates/partial clones, inherited config/trace
overrides and isolated config parsing. Linux additionally exercises a metadata symlink. All fixtures
use synthetic temporary repositories; no real scan configuration, main channel or external service changes.
Twelve scanner tests pass on Windows and thirteen on Linux; daemon all-target Clippy passes on both.

The reviewer established no additional concrete vulnerability in the inspected phone, SSH, Board or
workflow paths. The proposed current-directory `ssh.exe` hijack was not confirmed: Rust 1.98.1 does not
implicitly search that directory ([official implementation](https://raw.githubusercontent.com/rust-lang/rust/1.98.1/library/std/src/sys/process/windows.rs)).
That is not a claim that those features are exhaustively verified. The remediation has focused regression
coverage, not a second independent review. Same-user directory replacement races are outside this scanner
boundary; actual Windows junction/permission fixtures and final installed/live-host checks remain open.

The reviewer's synthetic reproduction folder remains at
`C:\DEV\Agentbolt\target\security-review-git-boundary-cf15c6b2f53a486baf0b2cd363b095f3`.
After verifying its exact location and that it contained no reparse points, cleanup was rejected by
automatic approval review as “blocked by policy,” before execution. No alternate cleanup was attempted.
