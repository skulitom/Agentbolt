# Native remake verification — 22 September 2026

These are observed results, not a claim that every release gate has passed.

## Automated checks

- Locked Release build: zero warnings/errors. WPF on .NET Framework 4.8; the official `ModelContextProtocol.Core` 1.4.1 is the single direct product dependency.
- `native/test.ps1`: **51 checks pass**, followed by real stdio suites for both generic hosts and Codex metadata routing. Both stdio suites also pass against the packaged executable.
- Three tools, **1,628 UTF-8 bytes** in the tested catalog. Creation returns within the one-second test bound without waiting for input. Concurrent wait/cancel, repeatable reads, timeouts, malformed input, matching text/structured output, clean stdout and EOF shutdown pass.
- Distinct `_meta.threadId` values on one connection produce distinct task destinations. Missing identity reports no automatic delivery; malformed identity is rejected.
- Persistence checks cover a fresh store view, terminal retries/conflicts, 12 answer/cancel races, traversal rejection and atomic records. A real subprocess round-trips quotes, trailing backslashes, Unicode, newlines and shell metacharacters without invoking a shell.
- Actual WPF controls cover choice submission, validation, omitted optional fields, active-control preservation, drafts restored in a replacement panel and Later remaining pending.
- Guided setup ran twice with the real Codex CLI against an isolated `CODEX_HOME` and install directory. An existing connection survived, a backup was written, and repeated setup reused the package. The developer's actual configuration was not changed.
- A subprocess stand-in tests delivery failure, bounded retry, two competing workers recovering an expired claim, per-task routing, large Unicode responses, and no resend after acknowledgment. No real task is messaged by these regression tests. A separate concurrent-read regression exercises 120 atomic replacements against 300 complete-snapshot reads.

## Packaged desktop

Tested in Anode's separate Windows desktop with two packaged MCP processes and `tests/desktop-harness.mjs`. All submitted test choices and notes are synthetic, not user preferences.

1. Exactly one panel appeared above an existing application without becoming the foreground window. All simple-choice actions were visible at 860 × 620.
2. A second agent's question arrived while a note was being edited. The note and focused edit control were preserved.
3. A choice saved the answer and comment; the other process received the exact terminal answer through `feedback_read`.
4. Blank required input displayed “Minutes is required.” and kept the request pending.
5. After entering `15`, the panel owner was terminated. The other process restored exactly one panel, the selected question and its draft, without taking focus.
6. Submission after takeover saved `minutes: 15`, omitting the untouched optional boolean.
7. Both stdio inputs closed cleanly; the harness exited 0 with no AgentUI windows left. Other desktop applications were left alone.
8. A socket ownership snapshot found **no TCP connections or listeners** belonging to either AgentUI process. This is not a sustained network-denied test.

Working-set snapshots were 78.9–86.2 MiB for the panel owner and 43.4–48.0 MiB for the second MCP process. Initial private memory was 51.8 and 36.3 MiB respectively. These are observed snapshots, not guarantees; the harness also read twice per second. Formal idle CPU and latency measurements remain open.

## Host delivery

An explicitly marked synthetic reply went through `CodexDelivery` to this originating development task using Codex CLI **0.155.1**. The CLI accepted it on attempt 1; the durable delivery state became `queued` with no error. This probe used the launching environment's task identity; per-call metadata routing was tested separately over stdio.

**Receipt confirmed:** after the previous development turn ended, feedback `1b03065e30aa4209b1f185394734ce2e` arrived as a new message and automatically resumed this same task. The model received the full synthetic answer and continued implementation without another manual message. This verifies a reply queued during an active turn being processed in the next turn. It does not establish mid-turn interruption or delivery first submitted to an already-idle task. The panel retains **Queued in Codex**, because individual model receipts are not reported back to AgentUI.

Failure/retry and competing-worker recovery pass with the subprocess stand-in; actual disconnected-Codex recovery remains a live-host check. A second automatic host adapter is not implemented. Large answers now queue a short ID notification and remain fully available through `feedback_read`, preventing permanent failure at the Windows process argument boundary.

References: [Codex thread metadata](https://github.com/openai/codex/pull/18093), [Codex queue implementation](https://github.com/openai/codex/blob/main/codex-rs/cli/src/queue_cmd.rs), [SDK stdio transports](https://csharp.sdk.modelcontextprotocol.io/v1/concepts/transports/transports.html), [Windows runtime requirements](https://learn.microsoft.com/en-us/dotnet/framework/get-started/system-requirements).

## Package and remaining gates

Deployable files total approximately **3.84 MiB**; the zip is **1.39 MiB**, including dependencies, documentation and notices. The Windows runtime is not bundled. A 145.2 MiB self-contained .NET experiment was rejected as the shipping design. Its ignored `artifacts/win-x64` folder remains because automatic approval review blocked cleanup with a general policy rejection; it is excluded from the new zip.

Before legacy cutover: verify replies first submitted to an already-idle task; add a second host; test with networking denied; validate clean Windows installation and signing; check high contrast, screen readers, DPI and keyboard usability; quantify latency/CPU; surface presentation failures to agents; and bound retained history. Legacy worktree changes were not overwritten or removed.
