# Lightweight AgentUI remake

Planning date: 21 September 2026. Revised 22 September after the user selected a shared desktop panel and prioritised effortless, frequent interaction. This document proposes the replacement; it does not implement it.

## Outcome and constraints

Build a small local MCP server whose job is to ask for feedback, retain the answer, and let an agent collect it while working asynchronously.

Confirmed requirements: **no HTTP listener**, including localhost; offline operation; and **one small desktop panel that works across agent apps**. The user should not have to open links, edit response files, refresh a page, or tell the agent to check for an answer. The host starts the MCP executable automatically after installation. Human input happens in its native desktop UI.

The earlier plan equated "no extra server" with a host-rendered or file-only UI. A native desktop window can be owned by an MCP process without running a web server. This revision adopts that architecture. Each connected host still needs its own stdio MCP process; one of those processes owns the shared panel. There is no separately managed UI service.

Proposed first-release scope: questions, choices, and simple forms. Custom interactive previews have not been confirmed as a requirement and are deferred. Offline means the feedback system needs no Internet access after installation; the host's model and its own online requirements are outside this project's control.

The core contract is:

1. The agent creates a feedback request and receives an ID immediately, without waiting for the human.
2. The agent continues independent work.
3. The human submits an answer or dismisses the request later.
4. The answer is saved locally and can be retrieved repeatedly, including after a process restart.
5. The agent receives the answer through automatic integration in supported hosts. The human does not need to send a second message saying they answered.

## Product experience

The panel is part of the minimum product. Manual file submission is a developer recovery mechanism, not a supported end-user workflow.

| Moment | Required behavior |
| --- | --- |
| First use | Install one package and connect selected agent apps through a short guided setup. After the host's required trust step, show one real sample question. No Node installation, terminal startup, port entry, or manual config editing in the normal path. |
| A new question arrives | Present a compact card automatically without taking keyboard focus. Identify the agent, project, question, and whether work is blocked. |
| Several questions arrive | Add them to the existing panel, with a pending count and clear grouping. Preserve the current card, focus, and draft. Do not create more windows or reorder the item being answered. |
| The user answers | Offer clear choices, text, or a small form, with an optional explanation. A simple choice can submit in one click; forms use an explicit Send action. |
| The user cannot answer yet | Provide Later and Dismiss. Later affects presentation only; it is not an answer. Dismiss may carry an explanation. |
| After submission | Show Saved immediately after durable acceptance, then Delivered when the agent integration acknowledges receipt. Show Applied only if the agent explicitly reports using the answer. |
| A follow-up arrives | Update the same activity group with the next card. Keep the previous decision available in a compact history. |
| The panel is hidden | Keep a taskbar/tray entry with a pending count, an optional global shortcut, and a restrained notification on a new actionable request. Reopening reaches the existing panel directly. |
| The connection fails | Keep the draft, explain that delivery is pending, and retry automatically. Never show an unacknowledged answer as delivered. |

Automatically surfacing a question must not become repeated interruption. Open the first card without activation; while the panel is open, update it quietly. Respect an explicit hide, Later, Do Not Disturb, and notification settings. Batch bursts. Announce new requests accessibly without moving keyboard focus. The user should react to a visible request, not remember to go looking for one.

The UI remains reachable while at least one local MCP process is alive. If all hosts stop their MCP processes, no independent daemon remains. State survives and the next launch restores pending work automatically. An always-running panel after every agent exits would be a separate lifecycle requirement, not something stdio alone provides.

## What the repository tells us

The current application already uses one Node process, with stdio MCP plus HTTP and WebSocket services. Its web implementation also creates a second HTTP server for previews. The remake must remove those listeners, not merely combine processes.

The inspected source contains three packages, nine registered tools, React/Zustand UI state, Express/ws transport, a preview runtime, a widget library, and multi-client installation logic. A count of `.ts`, `.tsx`, `.js`, and `.css` files under `packages/server/src`, `packages/shared/src`, `packages/ui/src`, and `packages/ui/public/agentui` found 93 files and 13,948 lines. This count includes declarations and checked-in copies, and excludes tests, examples, and dependencies.

Relevant evidence:

- [MCP entry point](../packages/server/src/mcp.ts) and [runtime context](../packages/server/src/mcp/context.ts): process and transport lifecycle.
- [HTTP implementation](../packages/server/src/web/http.ts): shell and preview listeners.
- [tool registration](../packages/server/src/mcp/tools/index.ts): current public surface.
- [waiters](../packages/server/src/core/waiters.ts), [event log](../packages/server/src/core/event-log.ts), and [storage](../packages/server/src/core/storage.ts): asynchronous delivery and persistence lessons.
- [existing MCP Apps proposal](plan-claude-desktop-widgets.md): useful host-integration research; its broad widget scope is not a requirement for this remake.

Preserve the lessons about stdout, validation, durable answers, explicit user intent, and consistent structured/text results. Rewrite the implementation around the smaller contract. The current README describes `ui_ask` as blocking, while its implementation returns immediately by default; new documentation and contract tests must describe the same behavior.

## Architecture

```text
Agent host A --stdio--> AgentUI executable A --+
                                              +--> local request/answer files
Agent host B --stdio--> AgentUI executable B --+             |
                                                            v
                                        One shared native feedback panel
                                        owned by one live MCP process

Supported host integration <-- accepted answer <-- owning MCP process
```

There are no listening sockets, background daemons, WebSockets, preview origins, browser tabs launched by the server, or network asset downloads. The shared files are internal transport/storage; users never handle them.

Ship one product with one executable per supported OS/architecture. The executable hosts stdio MCP on a worker and the native UI on the required UI thread. For concurrent hosts, elect the UI owner with a per-user OS mutex; other MCP processes publish their own requests and accepted results into isolated namespaces. The UI writes response candidates back to the originating namespace. Each namespace's owner validates and accepts its own answers. File change signals trigger a rescan; a bounded reconciliation timer handles missed signals while requests are pending.

If the UI owner exits while another MCP process remains, another process acquires ownership and reconstructs the same panel. Persist drafts and presentation state independently of a particular process. Test startup races, owner crashes, and shutdown handoff before relying on this design. No new coordinator service is introduced.

The implementation language is now a spike decision: the old TypeScript-only choice was tied to a host-rendered HTML form. A concrete Windows-first candidate is .NET/WPF with the official C# MCP SDK and stdio transport. Package its runtime so users install one product; measure the bundle and per-process memory because self-contained .NET includes runtime weight. Windows-first is a proposed delivery scope, not a confirmed cross-platform requirement. Keep the domain/storage boundary independent of the native UI toolkit. See [WPF](https://learn.microsoft.com/en-us/dotnet/desktop/wpf/overview/), [MCP C# transports](https://csharp.sdk.modelcontextprotocol.io/v1/concepts/transports/transports.html), and [.NET deployment](https://learn.microsoft.com/en-us/dotnet/core/deploying/single-file/overview).

MCP Apps can remain an optional host adapter later. It cannot guarantee this panel's presence across applications: the host controls its lifetime and available display modes. Even a picture-in-picture request may resolve to another mode. See the [official display-mode API](https://apps.extensions.modelcontextprotocol.io/api/classes/app.App.html#requestDisplayMode).

## Minimal contract

Three tools visible to the model:

| Tool | Inputs | Result and behavior |
| --- | --- | --- |
| `feedback_request` | question, optional context, optional flat fields | Persist a new request; surface it in the desktop panel; return ID and pending status immediately. |
| `feedback_read` | explicit request IDs, optional `waitMs` | Return status and any terminal answer for each ID. Default `waitMs: 0`; bounded waiting only when explicitly requested. |
| `feedback_cancel` | request ID, optional reason | Cancel a pending request idempotently. Never overwrite an answer. |

The native UI submits through an internal function or a response candidate file with a request ID and idempotency key. No model-facing submission tool is necessary. Files are private to the OS user; this is not authentication against other processes running as that same user.

Keep the input schema small and fully described. Avoid a schema-discovery tool and a generic interface language. Proposed field types are text, number, boolean, and single choice; a yes/no question is a choice. Plain text context can display a short code or diff excerpt without a separate rendering subsystem. Limit field counts, text lengths, and request/answer sizes.

Example use:

```text
feedback_request({
  question: "How should the import handle duplicate records?",
  fields: [{
    id: "duplicates",
    type: "choice",
    options: ["Skip duplicates", "Replace existing records", "Stop and report"]
  }]
})
  -> { requestId: "...", status: "pending", presentation: "desktop" }

... agent continues independent work ...

feedback_read({ requestIds: ["..."] })
  -> { requests: [{ requestId: "...", status: "answered", answer: { ... } }] }
```

Return compact text and matching structured results on every branch. Unknown IDs, invalid answers, and storage failures must be explicit errors. Pending requests and wait timeouts are normal outcomes.

## Asynchronous behavior and its boundary

Creating a request must never hold a tool call open for human input. Reading without `waitMs` must also return immediately. Support several independent requests and answers arriving in any order.

An optional wait is capped initially at 20 seconds and respects MCP cancellation/disconnect. It does not claim to guarantee compatibility with every host timeout. It checks existing answers before parking, rechecks after registration, and releases listeners/timers when done. Waiting is a convenience when the agent has no other useful work; periodic checkpoint reads are the baseline.

Two independent integrations must work: showing the human a request, and delivering the response into the agent's working context. The native panel solves the first. It does not grant the MCP server permission or a protocol mechanism to interrupt arbitrary hosts.

For each fully supported host, provide an installed integration that automatically collects responses at execution checkpoints and uses a documented host mechanism to resume a paused/ended turn where available. That integration runs through the host's own extension/hook facilities; it must not require the human to poll, copy text, or type "continue". A prompt/skill asking the model to remember to poll is useful guidance but is not a delivery guarantee. Do not automate keystrokes into a host window to simulate support.

Separate three statuses: saved locally, delivered into agent context, and acknowledged/applied by the agent. Pending notification delivery must not invalidate a saved answer. Retry by stable ID; host integrations deduplicate. Verify receipt while the agent is working, waiting, and after its turn ends. If a host cannot resume automatically, label it as limited compatibility and retain the saved answer for the next turn. It does not meet the full effortless-interaction claim.

For a future MCP Apps adapter, `updateModelContext` supplies future context without triggering a response. `sendMessage` is a distinct host-supported action. Neither API is available to an arbitrary native stdio server simply because it has a desktop UI. See the official [App API](https://apps.extensions.modelcontextprotocol.io/api/classes/app.App.html#updateModelContext) and [host capabilities](https://apps.extensions.modelcontextprotocol.io/api/interfaces/app.McpUiHostCapabilities.html).

Do not use progress messages, resource notifications, or sampling as a substitute for a supported resume mechanism. Do not make MCP Tasks an MVP requirement. Native elicitation may later be an adapter, but its request-coupled interaction does not establish that a host will let the original tool return and the agent continue. See the [elicitation specification](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation).

When the originating MCP process stops but another remains, the shared panel can preserve or save a response candidate and mark it as waiting for that agent to reconnect. Once every MCP process exits there is no UI owner or watcher. Pending work and drafts are restored on the next launch.

## State and persistence

Keep one immutable request and at most one accepted terminal result per ID:

```text
pending --> answered
        --> dismissed  (human, optional explanation)
        --> cancelled  (agent, optional explanation)
```

Frequent interaction initially means many small discrete requests and follow-ups, not sending every keystroke or slider movement to the model. A follow-up question is a new request linked to the same activity. Reading is non-destructive, so retrying a read cannot lose an answer. Keep delivery acknowledgments and small UI drafts separate from immutable accepted answers.

Use a versioned local directory, separate from legacy `.agent-ui` data:

```text
<data-dir>/<request-id>/
  request.json       immutable question and field definitions
  response.json      internal UI response candidate
  result.json        server-validated terminal result
```

The response includes the request ID, schema version, answers, optional comment, and submission ID. Only an explicit Send/choice/Dismiss action creates a response candidate. Drafts are separate. Parse and validate complete files; malformed or partial writes remain pending with a diagnostic. After acceptance, later edits cannot silently replace the terminal result. File formats can support testing and recovery, but do not appear in normal user instructions.

In-process UI submission and cross-process file import call the same finalization function. Serialize operations; the first successfully persisted terminal transition wins. Duplicate submissions return the accepted result, while conflicting submissions report that the request has already ended. A cancellation race follows the same rule.

Use temporary files plus atomic replacement and handle transient Windows file locks. Acknowledge submission only after persistence succeeds; document and test the chosen durability boundary rather than promising protection from every filesystem or power failure. Do not persist drafts on every keystroke.

Give each connected agent session an isolated store namespace and encode it in the request handle. Persist a manifest containing host, project, session label, and process ownership so the UI can discover requests without mixing their destinations. Acquire exclusive writer ownership for terminal results in each namespace. The UI owner only writes response candidates and UI state for other namespaces. Restore mappings automatically on reconnect; require no manual request ID entry. Provide `--data-dir` for deliberate storage placement. The global inbox is a presentation over these files, not a shared server.

Owners import newly submitted candidates while connected, independently of the model calling `feedback_read`, so the UI receives acceptance promptly. `feedback_read` also reconciles before returning. Coalesce change signals and rescan pending requests periodically while needed; idle sessions should not busy-poll. Resolve waiters only for accepted terminal transitions, never draft edits.

Treat suggested values as suggestions. Start fields unanswered; the UI can display recommendations without pre-submitting them. Represent omitted optional values explicitly as unanswered. This preserves the legacy distinction between agent defaults and human decisions without a live `touched` tracking subsystem.

## Human experience

The panel has three areas: a compact pending list, the active question, and a small status/history area. Start with native text, number, boolean, and choice controls plus a comment field. Use OS theme, visible focus, accessible labels, readable errors, and keyboard navigation. Preserve drafts when switching questions. Show enough context to decide without opening the conversation or repository.

Bundle all assets or use system facilities. Render supplied text as text. No arbitrary executable previews or remotely loaded UI. For the first release, rich comparison and code excerpts are compact read-only context, not another application inside the panel.

Installation handles only the explicitly selected host adapters and preserves their existing configuration. Package the native runtime and use the installed absolute executable path, so normal startup never depends on downloading via `npx`. Any unavoidable host approval happens during setup. Starting an ordinary agent task should then be enough to make feedback reachable. A tray icon alone is insufficient discoverability; automatic presentation must pass a usability test.

## Scope removed from the replacement

- HTTP, WebSockets, LAN access, port selection, access tokens for browser routes, and browser-opening logic.
- React, Zustand, Express, Three.js, preview modules, animation helpers, and copied preview vocabularies.
- Saved widgets, presets, library search, permanent widget URLs, and automatic previews. Keep only the new small pending-request inbox and answer history.
- Generic content/control registries, live value synchronization, and agent-authored executable UI.
- Broad configuration auto-detection and cloud services. Keep guided setup for the selected supported clients and lightweight pending-request observation inside live MCP processes.

Defer attachments, custom previews, reusable forms, continuous feedback streams, and additional host adapters until a concrete use case requires them. Revisit storage only if real workloads outgrow small request files; do not add a database preemptively.

## Delivery plan

| Phase | Work | Exit condition |
| --- | --- | --- |
| 0. Prove the whole interaction | Spike a native MCP executable, automatic non-activating panel, and one supported host integration. Prototype panel ownership with two stdio clients. Measure runtime costs before fixing the stack. | A person answers an automatically shown question and the agent receives it without another human message. Test working, waiting, and ended turns. One panel serves two clients without listeners; owner exit restores it. Record any unsupported host behavior. |
| 1. Build the small core | Implement schema, request IDs, lifecycle, isolated storage, manifests, and request/read/cancel tools. | Real stdio tests create two questions, answer out of order, and read repeatedly without loss. Creation never waits for input. |
| 2. Build the panel | Implement pending queue, active card, drafts, Later/Dismiss, small history, tray/taskbar access, and notification policy. | A user completes several successive interactions without opening links, files, or extra windows. New requests never steal typing focus or overwrite drafts. |
| 3. Make delivery reliable | Implement finalization, owner handoff, reconnection, automatic host receipt, acknowledgment, and retry semantics. | Accepted answers survive restart; each reaches the correct agent once logically despite retries. Failures show Saved/Pending accurately. Test host disconnect and UI-owner crash. |
| 4. Package and constrain | Build the installer, guided connection flow, selected host adapters, concise agent guidance, accessibility checks, and footprint/offline reports. | On a clean machine, install and answer a real request without a terminal or runtime setup. Subsequent startup needs no downloads. Stdio stays clean; all processes exit when their host lifecycles end. |
| 5. Replace the legacy application | Checkpoint existing work, move the validated single package to the repository root, replace docs and scripts, and remove the legacy package tree from the active version. | Fresh build, typecheck, focused integration tests, packaged-artifact smoke test, and real host check pass. The legacy revision remains available for rollback. |

Phase 0 is a decision gate for the complete experience. Start with one fully supported host and add a second before claiming seamless cross-agent operation. Other stdio hosts can have limited compatibility, clearly labelled. If no intended host supports automatic delivery/resume, resolve that limitation before investing in polish or declaring the UX complete. Do not substitute manual file editing or a localhost server.

Logical boundaries, with filenames and language fixed after the spike:

```text
core/               request contract, lifecycle, persistence
mcp/                stdio lifecycle and three tools
desktop/            native panel, tray, drafts, UI ownership
integrations/       small adapters for tested agent hosts
packaging/          installer and platform builds
tests/              stdio, persistence, native interaction, host delivery
skills/agentui/SKILL.md
```

## Acceptance criteria and complexity budgets

Hard requirements:

- One MCP process per host connection, one shared panel owned by a live MCP process, zero listening sockets, zero runtime downloads, and no independent service.
- Three model-facing tools; native UI submission is internal.
- Request creation returns before any human response. Failed presentation is visible; do not silently leave the human with an invisible request.
- Requests surface automatically and are answerable without links, files, terminal commands, refreshing, or a follow-up chat message.
- Supported host integrations automatically deliver answers during work and handle the host's pause/end behavior explicitly.
- New requests never take keyboard focus from active typing; bursts reuse the existing panel and preserve drafts.
- Multiple requests, dismissal, cancellation, duplicate delivery, and restart recovery work.
- No default, unfinished response file, timeout, or disconnected UI is interpreted as agreement.
- Acknowledged answers remain readable; errors appear in both machine-readable and text results.
- Tests exercise the actual built stdio binary, not only handler functions.
- Network-denied tests cover the packaged server and native UI. Check socket ownership and process lifecycle during an end-to-end run. Feedback transport stays local; a cloud-model host may independently require Internet access.

Initial review budgets, to confirm after the spike:

- Keep direct product dependencies to the MCP SDK, native UI facilities, and necessary packaging/integration support. Replacing a framework with custom platform plumbing does not automatically make the product lighter.
- Keep core logic small and readable. Set separate budgets for the core, native UI, and host adapters after measuring the spike; the earlier 3,000-line total was based on the superseded embedded-form design.
- All model tool descriptions and schemas together below 8 KiB of serialized UTF-8 JSON.
- Bundle everything required for offline startup. Report the installer, installed size, and bundled runtime separately; do not call a native implementation small without measuring it.
- Record cold start, idle CPU, memory with one and two hosts, and request-to-visible latency on Windows. Initial UX target: a new card appears within one second when running; save acknowledgement appears within 300 ms on healthy local storage. These are targets to verify, not benchmark results.

## Migration and validation notes

The current worktree contains substantial existing modifications and untracked files. Implementation must first preserve that working state. Use an isolated checkout or a clearly separate prototype directory; a worktree alone does not include uncommitted changes. Do not overwrite the current `PLAN.md` or rewrite legacy files during the spike.

Create a clean versioned replacement with its own state format. Old widget libraries and sessions remain readable by the legacy version; automatic conversion is outside the first release. Retain only behavior tests relevant to the new contract and add targeted regression cases for persistence, asynchronous ordering, and host integration. The legacy suite's total test count is not a feature-parity requirement.

The handoff artifact should include the installable product, agent workflow instructions, a tested host compatibility table, measured footprint, usability results, and a rollback reference. Host rows must distinguish panel presentation, automatic in-flight delivery, end-of-turn resume, setup steps, and disconnection recovery. Manual polling/file operation does not qualify as full support.

## Implementation checkpoint — 22 September 2026

A Windows replacement candidate now lives in `native/`; see its README and verification report. It implements the three stdio tools, durable records, one native panel across MCP processes, draft recovery, final-answer delivery through Codex's queue command, and guided setup. The packaged build is approximately 1.39 MiB zipped and 3.84 MiB installed. It uses WPF and the Windows-provided .NET Framework runtime, with the official C# MCP SDK.

51 checks and two real-stdio suites pass. The packaged desktop was exercised with two clients, preserved typing during new requests, and restored its draft after the owner process was terminated. No AgentUI TCP listeners were observed. A synthetic feedback reply queued during work automatically resumed this same development task after its previous turn ended; the model received it and continued work. Failure/retry, competing workers and large responses also have subprocess regression coverage.

The active-turn-to-next-turn delivery path is verified. The full Phase 0 gate still needs the already-idle and waiting cases. Second-host automatic delivery, network-denied validation, accessibility/clean-install checks and the legacy cutover remain. Legacy files and the developer's actual host configuration have not been changed by the implementation.
