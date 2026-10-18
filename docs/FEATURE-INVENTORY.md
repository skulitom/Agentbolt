# Full-plan feature inventory

Source inventory, 2026-10-19. Scope is the private v4 plan, with Artem's latest feedback and his later
instruction to implement M2–M7 before heavy testing. This replaces the older handoff cutoff and Board
evidence gate. The table records implementation coverage, not observed behavior or production readiness.
Expanded features have formatting/compilation feedback only. Earlier test reports refer to older commits.

| Requirement | Implementation | Evidence still needed |
|---|---|---|
| M0: Rust workspace, console MCP/CLI, separate tray daemon and Desk, per-user channel isolation, Linux core/MCP | Workspace manifests; core `channel.rs`, `protocol.rs`, `store.rs`; IPC; CLI `server.rs`, `install.rs`; daemon lifecycle | Fresh Windows/Linux checks, both protocol revisions, packaging and repo guard |
| Fast Desk opening with measured fallback | Desk `startup.rs`, warm-window lifecycle and daemon/installer supervision; visible/focused Inbox polling only | Repeat final-candidate startup, idle and hidden-window checks |
| Open the exact originating chat | Core `navigation.rs`; immutable ask client/session/cwd/machine provenance; Desk chat controls and OS launcher | Live Claude desktop/CLI and Codex host metadata/navigation |
| Direct Inbox; priority/deadline ordering; no budgets/digest/fair share | Core service/rules; paged Inbox; legacy migration and wire compatibility | Fresh ordering, migration, default-clock and mixed-version checks |
| Owner cap/dedupe/defaults/deadlines, provisional P0 handling | Core rules/service; 3 open asks per owner; no irreversible defaults or unseen default clocks | Concurrent and boundary regression cases |
| Batch answers, Not needed, timed/indefinite mute, per-owner notification batching | Desk controls; atomic answer transaction; persisted mute deadlines; daemon tick and emission checks | Batch/draft/mute lifecycle and actual silent notifications |
| Durable answers, evidence, question and delivery history | Core store; owner-scoped reads and cursors; Desk History/search, Inbox paging and delivery browser | Large answers/history, restarts, paging under mutations and retention |
| Agents and owner settings remain reachable | Searchable paged Agents and Owners; chat/mute controls; Desk-approved owner proposals | Native UI and owner rename/permission persistence |
| Drafts survive hiding, upgrade and page navigation | Desk atomic draft store; bounded reconciliation of known closed IDs; unknown drafts preserved | Persistence/crash/upgrade and cross-page regressions |
| Tray, pause/quit, scheduled start, restart and upgrade/rollback | Daemon tray/lifecycle; installer/task ownership checks; warm-Desk coordination | Installer/native/reliability checks on the final build |
| Same-user boundary, verified separate answer pipe, safe links and single-use tokens | IPC image/path checks and protected discovery; core tokens; installed Desk handlers; filtered link launch | Expanded security review and adversarial pipe/token checks |
| Silent toast actions and count-only phone push | Daemon toast/ntfy; eligibility rechecks; handler warning in Desk | Normal-session notification and audio observation |
| Codex answer delivery with preserved host policy | Dedicated delivery workers, explicit owner wake permission and execution-time recheck; queue acceptance remains unconfirmed | Active/idle/finished live clients and original-policy verification |
| M2: Claude long wait and durable dispatcher/next-run recovery | Configurable ten-minute waits; owner registration/checkpoint reads; relay handoffs; `relay-notes` durable append/mark adapter | Live long-wait behavior, authorized dispatcher rollout and next-run consumption |
| M3: baseline, noise, latency, deadlines, backlog, collisions and verdict | Core `metrics.rs`; Activity tab; submitted-ask denominator; fixed first-14-day observation; history-span reporting; immutable baseline | Real baseline/adoption, session go-ahead checks where required, and Artem's verdict |
| M4: explicit working-copy allowlist and exclusions | Desk configuration/reviewed FOCUS import; bounded read-only daemon Git scanner; exclusion overlap checks before scans | Isolated Git scenarios, path/permission boundaries and live authorized configuration |
| Dirty/unpushed/agent holds and PR/merge checks | Core `holds.rs`; complete collision scan with bounded result pages; leases; conservative submodule holds; `holds` and `pr-check` commands | Scanner/glob/lease regressions and actual authorized workflow integration |
| M5: roles/who/doing and matching work at registration/request creation | Core `board.rs`; profile preservation/clearing; public agent IDs, lease ceilings and lapse visibility | Routing/matching, multi-session and lease-boundary cases |
| Routed/threaded/deduplicated messages, references and reads | Board post/read; cursor reads; safe references; searchable Desk Board pages | Message routing, retry/dedupe and paging scenarios |
| Atomic request claim, lease, fencing, owner cap and terminal outcomes | Board request/claim/renew/release/done/cancel; 10 open requests per owner; `failed` outcome; stale fencing refusal | 20-agent race, stale completion, retries and cap enforcement |
| M6: protected phone questions and one-time replies | Core/daemon phone modules; Desk-only settings and receipt activation; durable outbox/receipts; private-topic checks, bounded HTTPS and acknowledgements | Real protected ntfy service, offline/replay/revocation cases, credential handling and phone behavior |
| Beads linkage | CLI exact-issue importer creates a linked deduplicated Board handoff; leaves the source issue unchanged | Isolated real-format adapter and CLI cases |
| Claude plugin and integrations | Decision/coordination skills; twelve-tool generated catalog; Registry/Glama/winget/Scoop/release sources | Expanded behavioral evaluation and hosted acceptance; no publication has occurred |
| M7: other machines | SSH-key relay, verified host keys, MCP-only operations, explicit remote machine identity and safe-read reconnection | Actual Linux/Windows SSH sessions, reconnect/uncertain-mutation and identity cases |
| MCP content rendering and compact discovery | Full JSON text mirrored in structured content, no output schema; shared generated host-aware tool schemas | Both protocol revisions and real client rendering |

## What implementation does not establish

- No real phone credentials, SSH connection, monitored working copy, dispatcher schedule, baseline or
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

The named product features now have implementations. Next, validate the expanded code with focused
regression/security/platform checks and repair findings before rebuilding packages, live-host checks,
idle measurement or a fresh uninterrupted reliability run. Signing remains explicitly deferred for 0.2.
The production goal stays active until its required evidence exists; nothing has been tagged or published.
