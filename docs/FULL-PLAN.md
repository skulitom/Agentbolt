# Full-plan implementation

Artem explicitly expanded this task to M2–M7 on 2026-10-18. The private v4 plan's latest feedback still
wins over older text. Implement every feature first; heavy validation, licensing, packages and live-client
checks follow after the implementation is complete. Existing checks are historical evidence only.
Do not tag or publish. Do not change the real main-channel installation as part of development.

## Feature work

- M1 completion: selectable answered/cancelled questions with the original evidence, Open chat, answer,
  note and delivery history, including when no questions remain open.
- M2: configurable Claude long waits and durable relay handoff records/instructions for Reception and
  the next owner run. Retain at-least-once checkpoint reads and never treat a queued message as receipt.
- M3: baseline snapshots and comparison reporting for ask volume, answer latency, overdue/stale asks,
  irreversible backlog, Not needed by owner, collisions and Artem's verdict. The 14-day observation is
  a later adoption activity, not a reason to defer feature implementation.
- M4: explicit working-copy allowlist, lock-free dirty/unpushed Git path holds, freshness/errors visible,
  agent holds and PR/merge collision checking. Never scan outside the configured allowlist.
- M5: Board registration/roles, who/doing, routed messages and reads, deduplicated requests, atomic claims,
  handle-based renewable leases, fencing tokens, owner limits and ten-request cap, completion and
  cancellation, matching requests/claims/holds on registration and request creation, and a Desk view.
- M6: protected ntfy phone choices with single-use per-option tokens routed through the installed Desk;
  Beads links/adaptation; extend the existing Claude plugin; keep registry/Glama/winget artifacts in sync.
- M7: authenticated relay for agents on other machines, explicit machine identity, reconnection and
  durable reads. Remote agents have no human-answer operation.

## Implementation progress

The first expanded implementation pass adds the following code (compile checks only; no feature or
release test run has been claimed):

- Desk history: answered and withdrawn asks remain selectable even with an empty Inbox; their evidence,
  Open chat, human/default distinction, notes and delivery records remain available without answer controls.
- M2: Claude registration/plugin permit ten-minute waits and discovery advertises the actual cap. Relay
  lists now include exact destination metadata, full messages and single-line `(via desk, ask <id>)`
  notes records. [CLAUDE-DELIVERY.md](CLAUDE-DELIVERY.md) describes the dispatcher adapter.
- M3: Activity tab, fixed baseline, current/prior 14-day cohorts, median human-answer time, deadline misses,
  owner Not needed rates, stale reversible/irreversible backlog, PR/merge collision check counts and a
  Desk-authored verdict. Reports can be copied as JSON. No baseline or adoption run was started.
- M4: Desk-only working-copy allowlist/exclusions and reviewed FOCUS import; bounded background Git scans;
  dirty and unpushed paths; unknown/error/stale status; renewable agent holds and overlap matching.
  `work_holds` and `agentbolt holds --stage pr_open|merge` expose the guard. Nothing is configured or
  scanned in the real installation. `pr-check` now implements the PR/merge adapter; live workflow rollout is separate.
- M5: durable routed/threaded messages and cursor reads; role/doing registration and matches; deduplicated
  requests, ten-request owner cap, atomic claims, inactivity leases, fencing, explicit renew/release/done,
  cancellation and Desk Board view. Lease ceilings can be changed in the Desk. Tool-call activity renews
  only that handle's live claims. Public agent IDs do not expose bearer handles.
- Plugin: added scoped coordination instructions and long-wait configuration. Its previous evaluation
  applies only to the old plugin; a new evaluation belongs after all feature work.

The next expanded pass adds these features (compilation only):

- M6: optional protected ntfy delivery with Desk settings, credentials omitted from status/logs, anonymous
  read-protection checks, bounded HTTPS requests, durable publication/receipt state and acknowledgements.
  Purpose-bound single-use buttons go through the installed Desk. Simple reversible choices support up
  to three options; irreversible/long/form/text questions stay in the Desk under the existing token trust
  restriction. [Phone replies](PHONE-REPLIES.md) records the setup and retention limits.
- M6: `agentbolt beads` imports an exact existing issue into a linked, deduplicated handoff. It does not
  claim, change or close the issue. [Beads adapter](BEADS.md).
- M7: `agentbolt mcp --ssh ... --machine ...` and the MCP-only SSH relay; verified host keys, no password
  prompts, bounded connections, safe-read reconnection and explicit remote-machine provenance on asks.
  A remote chat is not opened or queued as if it were local. [Remote machines](REMOTE-MACHINES.md).
- Workflow adapters: `relay-notes` filters an explicit owner allowlist, durably appends once to the chosen
  notes file, then records delivery; `pr-check` computes complete paths from immutable Git commits and
  checks holds at PR open/merge. Both fail visibly on incomplete work. [Integration](WORKFLOW-INTEGRATION.md).
  These are implemented commands; live Compass charters, notes and schedules have not been changed.

- The feature audit also added one-hour/four-hour/day/indefinite mute choices. Deadlines persist and expire
  in the daemon, including after restart. Board profile updates now preserve omitted roles/repository;
  explicit empty roles and `clear_repo` remove those fields without disturbing the other one.

The navigation pass adds searchable History and Board pages, all-status Board requests, inactive agent
pages, Inbox and hold pages, and owner-filtered relay/answer continuation cursors. Only the returned
Inbox page starts shown/default clocks; browsing other tabs polls counts without showing asks. Drafts
survive Inbox page changes. Question pages also stop at a byte budget instead of exceeding the pipe
limit. Hold checks inspect every matching active hold while returning at most 50 collision details;
`total_collisions` and `more_collisions` disclose the remainder. The glob matcher uses bounded stack
and memory independent of path length, with directory-boundary handling for `**/`.

Board messages now carry up to eight safe references; fenced completion supports `failed: true` with
the same stale-claim and retry rules as success. Durable phone receipts reach the installed Desk before
new network checks, so a poll outage does not hold up a previously received reply. The plugin's full
twelve-tool catalog is generated from the server's schema source, including its host wait cap. All of
these changes have compilation feedback only; no feature, native, behavioral or release checks ran.

The final navigation/reporting pass adds searchable, complete Agents and Owners directories; known-closed
draft reconciliation in bounded batches (unknown IDs and drafts on other pages are retained); and a
paged delivery-history browser in both Desk and owner-only answer reads. Each answer carries only its
latest 50 delivery records plus continuation metadata. Not needed rates now divide by submitted asks,
and adoption reporting uses the fixed first 14 days after baseline recording, with stored-history span
shown separately. These are implementation changes with compilation feedback only.

Scanner review also closed two false-clearance paths: an allowlisted source may not overlap an excluded
root in either direction, and submodule paths are conservatively held using parent-index metadata only.
Nested checkouts are not scanned. Future-dated scan timestamps are unknown.

The [full feature inventory](FEATURE-INVENTORY.md) now maps each v4/M0–M7 product requirement to its
implementation and distinguishes source coverage from validation and rollout. No remaining named product
feature has been intentionally deferred. Fresh regression/security/platform/host checks and rollout
requirements remain unproven; implementing the interfaces does not establish production readiness.

## Later verification and rollout

Run the full feature/security/compatibility and platform checks once implementation is complete, then
release preparation. Real adoption/baseline periods, live host behavior, phone service credentials,
publication and any external workflow rollout must be recorded honestly; implementing a feature is not
evidence that those activities occurred. The production goal remains active.
