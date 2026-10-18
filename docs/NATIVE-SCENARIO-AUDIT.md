# Native-remake scenario audit

Source: the repository's own `native/verification.md` on tag `legacy-native-2026-10-18`.
The old WPF implementation's passing results are not evidence that the Rust implementation passed.
This maps the applicable scenarios to current checks, including the ones still outstanding.

| Legacy scenario | Rust evidence / status |
|---|---|
| Small MCP catalog; protocol negotiation; clean JSON stdout; malformed requests; EOF | `agentbolt/tests/mcp_stdio.rs`, IPC malformed-line/oversize/disconnect tests, and the daemon process suite pass on the applicable platforms. |
| Full text and structured result; real host metadata | Real Codex client trace summaries in `CODEX-LIVE-2026-10-18.json`; Claude Desktop remains open. |
| Ask returns immediately; bounded waits; answer wakes a wait; cancellation; repeated reads | Service and daemon tests pass. Real Codex ask calls took 2–17 ms in the recorded host runs. |
| Distinct threads sharing one MCP connection; absent or malformed identity | Server registration/session tests and core owner-isolation tests pass; real Codex supplies per-call thread IDs. Unknown identity grants no wake permission. |
| Durable results after restart; duplicate terminal writes; twelve answer/cancel races | File-store restart tests pass. New `twelve_answer_cancel_races_have_one_durable_terminal_result` runs actual competing threads, checks one winner, repeat reads, reopen durability and an intact event chain on Windows and Linux. |
| No shell interpretation of quotes, Unicode, newlines or metacharacters | Native-child argument test, direct Codex argument tests and validated navigation tests pass. No shell builds an answer command. |
| Reply/choice/form validation; blank required field cannot submit | Core and view tests pass; the real Desk batch fixture passed edited choice/reply/required-number validation, disabled Send and atomic submission. |
| Optional fields omitted rather than invented | Audit found untouched optional booleans became false. Fixed with explicit Not answered/Yes/No state; omission, both explicit values, old draft compatibility and saved new drafts now pass tests. Fresh single-answer GUI verification remains required. |
| Selection and draft survive panel termination/restart | Draft tests and actual installed open-Desk upgrade/restart checks pass. The warm Desk now belongs to the daemon rather than to one MCP process. |
| A new ask does not steal focus while editing a note | Still requires a fresh GUI observation on the candidate. |
| Usable panel at 860 × 620; one panel; no unwanted activation | Repeated warm-window tests cover one resident window and hidden/unseen safety. The exact small-window layout check remains open; a live Agents-row width defect was found and fixed. |
| Guided setup twice, preserving other client configuration | Replaced by `clients`, which prints explicit setup instructions. The installer does not rewrite client settings; upgrade/rollback tests preserve the installation and open clients. |
| Concurrent writes and atomic file replacement under readers | The architecture now has one SQLite writer, transactions and WAL. Mutex/exclusive-store, event-chain, race and concurrent IPC tests cover that design; the old JSON-file replacement stress count is not claimed as rerun. |
| Delivery failure, bounded work, durable retry/read, no cross-thread answer | Dedicated bounded delivery-worker, owner, relay and process tests pass. Live active and loaded-idle Codex receipt passes; unloaded automatic wake remains a documented host limitation. |
| No TCP listener | Transport is local named pipes/Unix sockets. A candidate process-level listener snapshot is still required. |
| Silent notifications and activation | XML/token/registration checks pass. Anode's notification platform is unavailable; normal-session banner, sound, button and body checks remain open. |

The headless suites were rerun on 2026-10-18. GUI and host-specific rows are not implied by that run.
Do not mark this audit complete until the explicitly open rows have evidence or an explicit maintainer scope decision.
