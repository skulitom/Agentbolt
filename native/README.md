# AgentUI native remake

A small Windows desktop panel for asynchronous feedback to coding agents. It is a working replacement candidate; the legacy application has not been removed.

## Try it

Open `artifacts/windows/AgentUI.exe`. Choose **Try a sample question** to explore without connecting anything. **Connect → Install and connect** copies the package into your Windows account and registers `agentui-native` with the installed Codex CLI. Restart Codex once to load the connection. After that, agent questions appear automatically.

The zip contains the complete application. Extract the whole folder before opening the executable. It needs Windows with .NET Framework 4.8 or later (included with Windows 11); there is no Node.js runtime, browser, HTTP listener, account or runtime download. An unsigned development build may trigger Windows' download warning. Code signing and a clean-machine installer check are release work.

The panel is shared by all connected local agent apps. New questions do not replace the question you are answering. Choice buttons submit immediately; forms have a Send button. Optional controls start unanswered. Later keeps a question pending, Dismiss records a refusal, and closing the window hides it. Reopen from the tray; Pin keeps it visible. Standalone mode has **Quit AgentUI** in the tray menu.

## What works, and what remains experimental

| Host | Question and answer UI | Reply delivery |
| --- | --- | --- |
| Codex with `queue` support | Native shared panel; guided connection | Captures the per-call `_meta.threadId`, saves the answer, then invokes `codex queue` for that task. Verified with CLI 0.155.1: a reply queued during work automatically starts the next turn and reaches the model. Mid-turn interruption and replies first submitted to an already-idle task remain unverified. |
| Other local stdio MCP hosts | Same shared panel and durable answers | The agent uses `feedback_read` at work checkpoints. Automatic task resumption requires a host adapter and is not claimed. |

Codex connection metadata takes precedence over `CODEX_THREAD_ID`. The latter is only a fallback for a launcher that provides it. If there is no known task identity, the tool reports `automaticDelivery: false`; it does not guess a task. The setup currently locates `codex.exe` on PATH or in the standard npm installation. Nonstandard installs can supply the absolute executable through `AGENTUI_CODEX_EXE`.

**Queued in Codex** means Codex accepted the reply, not that the model acted on it. A failed delivery stays saved and retries. Delivery is at least once across a crash at the acknowledgment boundary, with a stable feedback ID for deduplication. A live Codex-connected MCP process is needed for retries; there is no separate background service. The feedback transport is local. A cloud-model agent can still need Internet access for its own work.

Large responses queue a short notification with the request ID. The agent reads the full saved answer with `feedback_read` before acting, avoiding the Windows process argument limit without truncating feedback.

## Agent contract

Only three tools are exposed (1,628 bytes combined in the tested catalog):

- `feedback_request`: ask a short question with optional context, choices, or text/number/boolean/choice fields. Returns an ID immediately. Continue independent work.
- `feedback_read`: read one or more IDs repeatedly, optionally waiting at most 20 seconds when no independent work remains. Reads never consume an answer.
- `feedback_cancel`: retire a question that is no longer relevant. An already accepted answer wins.

Never interpret an absent answer, timeout, draft or suggested value as agreement. On hosts without automatic replies, check outstanding IDs at natural checkpoints and before concluding work. Repeated delivery of the same feedback ID is the same decision.

```json
{
  "question": "How should duplicate records be handled?",
  "context": "Replacing records overwrites existing values. Skipping leaves them as they are.",
  "options": ["Skip duplicates", "Replace existing records", "Stop and report"]
}
```

Generic stdio registration points to the installed absolute `AgentUI.exe` with `args: ["--stdio"]`. Codex adds `--codex`. Do not use HTTP configuration. Host sandbox settings must allow a local desktop window and the application's local data directory.

## Build and verify

Development needs the .NET 8 SDK (C# 12 compiler), Windows, and Node.js for the stdio test driver. The generated application targets the built-in .NET Framework runtime. NuGet dependencies are locked; first-time development restore needs network access or a populated cache.

```powershell
./native/test.ps1
./native/build.ps1 -Publish
```

Publishing creates `native/artifacts/AgentUI-windows.zip`, a checksum, and the extracted `windows` folder. The tests use the real MCP executable and real WPF controls. Setup tests use an isolated Codex home and install directory; they do not change the developer's actual configuration. See [verification.md](verification.md) for recorded evidence and remaining release gates.

## Implementation

WPF supplies the native window and controls. The official C# MCP SDK handles stdio. Each host launches an MCP process; a named Windows mutex chooses exactly one panel owner. File change notifications plus a recovery timer refresh the shared inbox. If the owner exits, another live process takes over. No socket, HTTP server, WebSocket, extra UI process or resident broker is created by AgentUI.

Requests, terminal answers, drafts and panel state live under `%LOCALAPPDATA%\AgentUI\v2`. Per-request file locks serialize submissions and cancellation across processes. Complete records are flushed and atomically replaced. Submitted answers are immutable. Drafts remain separate from accepted answers. Only final answers and dismissals are delivered. Text is displayed literally; there are no executable previews or remote assets.

This directory inherits the Windows user's permissions. It is not encrypted or a security boundary against other programs running as the same user. Agent processes sharing the directory can read its records. No telemetry is implemented.

## Rollback and remaining scope

Setup preserves existing connections, backs up `config.toml` under the data directory, and installs each build in its own `%LOCALAPPDATA%\Programs\AgentUI\0.2.0-<hash>` folder. To disconnect, remove the `agentui-native` entry in Codex's MCP settings (or `codex mcp remove agentui-native`). After its processes stop, its installation folders can be removed; saved feedback is retained separately.

Before replacing the repository's legacy entry points: verify replies submitted to an already-idle task, validate a second host adapter, test with networking denied, measure CPU/latency, validate high contrast and screen readers, and exercise a signed package on a clean Windows machine. macOS/Linux, rich previews, attachments, continuous sliders, history retention and the legacy data migration are outside this first implementation.
