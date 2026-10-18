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
