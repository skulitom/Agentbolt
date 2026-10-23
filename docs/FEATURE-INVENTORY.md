# Full-plan feature inventory

Source inventory, 2026-10-19. Scope is the private v4 plan, with Artem's latest feedback and his later
instruction to implement M2–M7 before heavy testing. This replaces the older handoff cutoff and Board
evidence gate. The table records implementation coverage, not observed behavior or production readiness.
Expanded features now have [focused local checks](EXPANDED-VALIDATION.md). Earlier release test reports
refer to older commits; live integrations and production readiness remain unverified.

**Current status (2026-10-23):** source `1cca2b5` includes the later UI polish, local session/message
search and automatic agent access. These changes have release-build evidence, but the broader checks
below have not been repeated on them. The [older `0229139` candidate](CANDIDATE-0229139.json) has passing
platform/package/installed-upgrade, warm-start/idle and normal-session notification checks. Those results
do not establish the latest code's readiness. Testing remains paused at Artem's request, including the
reliability monitor. Remaining evidence is a backlog, not an instruction to restart testing. The
[refreshed `753f12d` archives](CANDIDATE-753f12d.json) include the later Sessions visibility-gate fix.
Build/package integrity and the normal app update succeeded; remaining runtime requirements are still
unverified. This supersedes `85c66d2`, whose source audit exposed the background-search bypass.

| Requirement | Implementation | Evidence still needed |
|---|---|---|
| M0: Rust workspace, console MCP/CLI, separate tray daemon and Desk, per-user channel isolation, Linux core/MCP | Workspace manifests; core `channel.rs`, `protocol.rs`, `store.rs`; IPC; CLI `server.rs`, `install.rs`; daemon lifecycle | Fresh Windows/Linux checks, both protocol revisions, packaging and repo guard |
| Fast Desk opening with measured fallback | Desk `startup.rs`, warm-window lifecycle and daemon/installer supervision; visible/focused Inbox polling only | Repeat final-candidate startup, idle and hidden-window checks |
| Open the exact originating chat | Core `navigation.rs`; immutable ask provenance; Desk launcher and validated copy-command fallback; unknown/remote sources stay copy-only | Real CLI host ID and native copy-command/origin controls pass. Terminal resume hit administrator consent and remains unverified, alongside Claude Desktop and visible Codex navigation |
| Direct Inbox; priority/deadline ordering; no budgets/digest/fair share | Core service/rules; paged Inbox; legacy migration and wire compatibility | Fresh ordering, migration, default-clock and mixed-version checks |
| Owner cap/dedupe/defaults/deadlines, automatic registration | Core rules/service; 3 open asks per owner; no irreversible defaults or unseen default clocks | Concurrent and boundary regression cases |
| Batch answers, Not needed, timed/indefinite mute, per-owner notification batching | Desk controls; atomic answer transaction; persisted mute deadlines; daemon tick and emission checks | Batch/draft/mute lifecycle and actual silent notifications |
| Durable answers, evidence, question and delivery history | Core store; owner-scoped reads and cursors; Desk History/search, Inbox paging and delivery browser | Large answers/history, restarts, paging under mutations and retention |
| Agents and owner settings remain reachable | Searchable paged Agents and Agent preferences; chat/mute controls; automatic registration and available names | Native UI and owner rename/permission persistence |
| Drafts survive hiding, upgrade and page navigation | Desk atomic draft store; bounded reconciliation of known closed IDs; unknown drafts preserved | Native draft/paging and current-binary reinstall checks pass. Artem's requested 0.1-to-0.2 update succeeded after the unsupported old Desk was closed; it does not prove automatic draft preservation for that old Desk. Final-candidate checks remain open |
| Tray, pause/quit, scheduled start, restart and upgrade/rollback | Daemon tray/lifecycle; installer/task ownership checks; warm-Desk coordination | Installer/native/reliability checks on the final build |
| Same-user boundary, verified separate answer pipe, safe links and single-use tokens | IPC image/path checks and protected discovery; core tokens; installed Desk handlers; filtered link launch | Expanded security review and adversarial pipe/token checks |
| Silent toast actions and count-only phone push | Daemon toast/ntfy; eligibility rechecks; handler warning in Desk | Normal-session notification and audio observation |
| Codex answer delivery with preserved host policy | Dedicated delivery workers, explicit owner wake permission and execution-time recheck; queue acceptance remains unconfirmed | Active/idle/finished live clients and original-policy verification |
| M2: Claude long wait and durable dispatcher/next-run recovery | Configurable ten-minute waits; owner registration/checkpoint reads; relay handoffs; `relay-notes` durable append/mark adapter | Live Claude CLI wait beyond twenty seconds and fresh-process owner recovery pass; full timeout/background behavior, Claude Desktop and authorized dispatcher/next-run rollout remain open |
| M3: baseline, noise, latency, one-click answers, deadlines, backlog, collisions and verdict | Core `metrics.rs`; Activity tab; submitted-ask noise denominator; first-shown one-click denominator with retained draft interaction evidence; fixed first-14-day observation; immutable baseline | Focused measurement/migration and native direct/edited/restored-draft checks pass; real baseline/adoption, session go-ahead checks where required, and Artem's verdict remain open. [Methodology](ADOPTION.md) |
| M4: explicit working-copy allowlist and exclusions | Desk configuration/reviewed FOCUS import; bounded read-only daemon Git scanner; metadata/config preflight and exclusion checks; allowed linked worktrees | Focused Git scenarios and scanner security regressions pass; actual Windows junction/permission cases and live authorized configuration remain open |
| Dirty/unpushed/agent holds and PR/merge checks | Core `holds.rs`; complete collision scan with bounded result pages; leases; conservative submodule holds; `holds` and `pr-check` commands | Scanner/glob/lease regressions and actual authorized workflow integration |
| M5: roles/who/doing and matching work at registration/request creation | Core `board.rs`; profile preservation/clearing; public agent IDs, lease ceilings and lapse visibility | Routing/matching, multi-session and lease-boundary cases |
| Routed/threaded/deduplicated messages, references and reads | Board post/read; cursor reads; safe references; immutable sender machine; searchable Desk Board pages | Origin survives sender expiry/retries in focused checks; native remote/unknown copy-only controls, local links and OS clipboard round trip pass. Actual cross-machine use remains open |
| Atomic request claim, lease, fencing, owner cap and terminal outcomes | Board request/claim/renew/release/done/cancel; 10 open requests per owner; `failed` outcome; stale fencing refusal | Crash/restart and live multi-client behavior; focused 20-agent race, fencing, retry and cap checks pass |
| M6: protected phone questions and one-time replies | Core/daemon phone modules; Desk-only settings and receipt activation; durable outbox/receipts; private-topic checks, bounded HTTPS and acknowledgements | Real protected ntfy service, offline/replay/revocation cases, credential handling and phone behavior |
| Beads linkage | CLI exact-issue importer creates a linked deduplicated Board handoff; issue identity survives matching titles; write-command and auto-push restrictions passed to Beads | Real Beads 1.3.1 CLI/import/export cases pass in an isolated Windows embedded database; other versions and external configurations remain unverified |
| Claude plugin and integrations | Decision/coordination skills; twelve-tool generated catalog; Registry/Glama/winget/Scoop/release sources | Six mocked scenarios pass in three repeats; manual handoff wording finding fixed and rerun three times. Live hosted acceptance remains open; no publication has occurred |
| M7: other machines | SSH-key relay, host-specific config and command shell, verified host keys, MCP-only operations, remote identity, origin-aware evidence and safe-read reconnection | Encrypted loopback, authentication, identity, reconnect/uncertain writes and capacity pass. Local Command Prompt/PowerShell/sh launch and PowerShell Unicode checks pass; actual cross-machine deployment remains open |
| MCP content rendering and compact discovery | Full JSON text mirrored in structured content, no output schema; shared generated host-aware tool schemas | Both protocol revisions and real client rendering |
| Copy references/reports and expand sections accessibly | Native clipboard feature enabled; header adapter handles explicit Expand/Collapse using the actual widget ID | Windows Unicode paste, file/session copy, JSON export and Board/Activity accessibility actions pass; focused regression covers independent same-title headers, repeated actions and disabled controls |

## What implementation does not establish

- No real phone credentials, external SSH account, monitored working copy, dispatcher schedule, baseline or
  adoption period was configured during this work. Those rollout steps have their own external state.
- Dispatcher charter/notes changes belong to the authorized dispatcher workflow. The repository contains
  the adapter and instructions; the live Compass files were not edited. Host-required approvals still
  happen in the originating session. Activity latency measures a recorded answer, not that separate approval.
- Protected phone replies support reversible choose/approve questions with at most three short options.
  Other questions open the Desk. This preserves the plan's token trust restriction; a phone token is not
  stronger authority for irreversible work. Topic write protection also requires correct server ACLs.
- Submodule content is not scanned outside the explicit allowlist. Parent gitlink paths and descendants
  are held conservatively. An excluded subtree makes its containing source unscannable. Unknown is never
  clearance, including missing upstreams, bounded-output overflow and invalid scan times.
- Cancellation, migration, confirmed relay delivery and exact-ID reads retain their distinct semantics.
  Pagination does not consume answers, create permission or authorize a PR action.
- The 14-day adoption comparison and outside traction criteria require actual elapsed observation after
  rollout/publication. Merely implementing their measurement or distribution interface cannot satisfy them.

The named product features now have implementations. Focused core/CLI/MCP/Git checks have begun and
found fixes recorded in the validation report. Native synthetic 60-item paging/search and draft navigation now pass; Board sections, delivery browsing and working-copy configuration remain directly reachable with long lists. Claimant details are resolved independently of agent-directory paging. Activity distinguishes periods before stored history from observed zero activity. Continue security/platform/UI checks before rebuilding packages, live-host checks,
idle measurement or a fresh uninterrupted reliability run. Signing remains explicitly deferred for 0.2.
The production goal stays active until its required evidence exists; nothing has been tagged or published.
