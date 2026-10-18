# 0.2 implementation audit

Artem's instruction at 22:01 UTC on 2026-10-18 is to implement all changes before heavy testing.
This audit uses the v4 feedback at the top of Compass's `PLAN.md`, `CODEX-HANDOFF.md`, and `NEXT.md`.
This is the historical M0/M1 audit. Artem subsequently expanded the task to the full plan, including
M2–M7, and reiterated that all features must precede testing and licensing. The active implementation
scope is tracked in [FULL-PLAN.md](FULL-PLAN.md); this audit does not establish full-plan completion.

The long CPU measurement and candidate soak were cancelled. The hourly monitor is paused. The two
current fixtures were uninstalled and purged successfully. The manual desktop checks are deferred too;
Artem's earlier authorization and readiness answer are retained. No test used the main installation.

## Implementation coverage

| Requirement | Implementation | Remaining work |
|---|---|---|
| Measure startup and choose a fast opening path | `agentbolt-desk/startup.rs`, `resident.rs`, `os.rs`; daemon `warm.rs`. Previous measurements selected a hidden warm Desk. | Final-build timing/CPU validation. |
| Hidden Desk must not mark questions shown | Desk polls Status unless visible, focused and not minimized; `app.rs`/`view.rs`. | Final-build regression validation. |
| Open chat on asks and Agents | `app.rs`, `os.rs`, core `navigation.rs`; host metadata stored with each ask. | Normal-session visible navigation and Claude Desktop environment validation. |
| Prefer Claude desktop host ID; validated terminal fallback | MCP `host_session_ref_from`; `chat.rs` validates IDs and canonical local folders, passes separate arguments. | Live Claude host check. |
| Codex per-call thread identity | MCP derives identity from host metadata; validated `codex://threads/<id>` link. Actual Codex metadata was observed previously. | Visible destination check. |
| Direct Inbox; no daily budget, digest tab or fair share | Core admission is direct, legacy rows migrate without being marked seen; Settings has no daily budget. | Full regression validation. Legacy wire fields remain for open older clients only. |
| Cap, dedupe, priority/deadline ordering, provisional P0 demotion | Core `rules.rs` and `service_impl.rs`, Desk ordering includes muted owners last. | Full regression validation. |
| Default only reversible, shown questions; silence never approval | Core default and answer rules, MCP full text and structured results, plugin instructions. | Full regression validation. |
| Not needed and persistent owner mute | Core and Desk operations, count derived from durable answers. | Full regression validation. |
| One popup per owner per ten minutes, P2 due within two hours | Durable owner cooldown and replacement toast, timer handles approaching deadlines. | Normal-session silent appearance/activation validation. |
| Muting also applies to pending notifications | Audit fix: exclude muted questions/deadlines from phone counts; recheck mute, pause and answer state before emitting a queued toast. | Final validation. |
| Silent toasts, single-use option tokens, handler-key warning | `toast.rs`, core `tokens.rs`, `service_impl.rs`, daemon emission-time registration check. | Normal-session sound/button/body checks. |
| Count-only phone push | `ntfy.rs`, Desk topic setting, no ask text in push, no configured default destination. Removed obsolete Digest notification wording. | Final validation; no real phone topic used for tests. |
| Durable answers and at-least-once reads | Store and bounded checkpoint queries. Audit fix: queue acceptance no longer suppresses checkpoint/relay results; explicit relay confirmations remain distinct. | Migration and checkpoint regressions, then final validation. |
| Codex delivery without taking request workers | Two bounded delivery workers; execution-time wake permission, validated host identity, no policy override flags. | Final client validation. Unloaded/archived wake limits remain documented. |
| Batch answers and approved owner proposals | Atomic core transaction, edited Desk review; owner renaming preserves state and refuses name collisions. | Final regression validation. |
| Native scenario parity and drafts | `NATIVE-SCENARIO-AUDIT.md`; typed optional boolean, atomic per-session draft persistence, focus-safe refresh, one Desk per session. | Final validation of current binary. |
| Always-on sign-in and failure recovery | Verified per-account/channel task, `PT0S`, supervisor restart/backoff, owned child lifetime. | Fresh uninterrupted 72-hour observation after code is complete. |
| Desk-only answer path and local IPC isolation | Separate pipes, path identity checks, user-only random endpoint discovery and private locks. No MCP answer operation. | Final security regressions; prior independent review evidence retained. |
| Upgrade while MCP/Desk is open | Lifecycle preparation, persisted drafts, original-session reopen, startup fence, task/file rollback before new daemon startup. After a possible DB migration, installer leaves new files in place for repair rather than reverting to incompatible binaries. | Final packaged installer/upgrade/uninstall validation. |
| Version, packaging and distribution | Workspace 0.2.0, clean-source locked-build packager, notices, checksums, release notes, Installed Apps, winget/Scoop/MCP manifests, draft-only workflow. | One final packaging pass and release validation; no tag/publication. Signing explicitly deferred. |
| Claude plugin | Plugin, instructions, schema validation and fixed-mock behavioral eval implemented. | Final archive/source consistency check. |
| Linux core/MCP, repository guard, licenses | OS boundaries and pinned CI in place; no new dependency in these audit fixes. | Final Windows/Linux checks. |

## Findings closed in code

1. **Queue acceptance was mistaken for delivery.** The live unloaded-chat observation was previously only
   documented. Source review found successful queue records also hid answers from the normal no-ID read
   and next registration. Delivery records now distinguish `ok` (transport acceptance) from `confirmed`
   (explicit relay delivery). Queued answers remain discoverable; repeated reads keep stable IDs. Migration
   recovers old unconfirmed records while preserving relay confirmations recorded in the audit chain.
2. **Mute stopped toast creation but not every notification path.** Phone counts included muted owners,
   and already queued toasts were not rechecked after a mute, pause or answer. Counts and emission checks
   now use current state. Tray/Inbox totals still include every open question.
3. **Obsolete Digest behavior remained in notification types and text.** Those internal fields and text
   are removed. Only historical database migration and older-client wire compatibility retain Digest.

No additional missing implementation was identified within this handoff's scope. This is an implementation
inventory, not a production-readiness claim. The final validation remains outstanding in `NEXT.md`.
Existing candidate archives describe their pinned historical commits and must be rebuilt after these fixes.

Focused Windows checks pass for queue acceptance/checkpoint recovery, notification eligibility and counts,
released-database migration (including relay preservation and intact audit chain), ntfy formatting/rate limit,
tray count text, and the repository guard. Core/daemon all-target clippy and formatting checks also pass.
These checks use throwaway/in-memory fixtures; no notification, host chat or soak was started for them.

## Validation order

Finish code and focused regressions; then run the Windows/Linux checks once, build one final candidate,
and run package/installer/native/client checks against it. Start the long idle/reliability measurements
only after that candidate is stable. Hosted distribution acceptance and publishing remain outside local
verification. Never count a cancelled soak or an earlier binary's measurement as a final-build pass.
