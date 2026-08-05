# AgentUI — Async User-Feedback Interfaces for Claude Code

> Implementation plan. Self-contained: everything needed to build this is in this file.
> Follow the milestones in order (P0 → P5); each one is independently verifiable.

## Context

Claude Code has no good way to get rich, interactive feedback from the user mid-task. This project builds a suite of MCP tools that let Claude **dynamically create small web-app interfaces** (sliders, pickers, live previews — e.g. a three.js render of an in-game rifle whose animation parameters the sliders drive). The user adjusts asynchronously while Claude keeps working; Claude receives interim feedback (committed value changes, comments) and a finalized, standardized structured-data output when the user is done.

Decisions already made with the user (do not revisit):
- **Stack:** TypeScript/Node end-to-end (`@modelcontextprotocol/sdk`, Express + ws, Vite + React).
- **Feedback model:** hybrid — blocking `ui_wait` long-poll + non-blocking `ui_get_state`.
- **V1 includes custom Claude-authored preview panes** (sandboxed iframe, three.js vendored).
- **UI delivery:** local web server, opened in the default browser (no Electron/Tauri).
- Platform: Windows 11 primary, keep cross-platform.

## System overview

One Node process, launched by Claude Code over stdio, acting as both:
1. **MCP server** (stdio transport) exposing `ui_*` tools.
2. **HTTP + WebSocket server** (lazily started on first `ui_create`, bound to `127.0.0.1`, port 4517 with scan fallback 4518–4536) serving a prebuilt generic React shell, per-session APIs, and Claude-authored preview modules.

Claude authors a JSON **Interface Spec** (zod-validated), optionally plus a single-file ESM **preview module**. The server writes a per-session directory, opens `http://localhost:<port>/session/<id>` in the browser, and mediates events both ways. All durable state is plain JSON/NDJSON files, so Claude can also just `Read` the output.

```
Claude Code ──stdio/MCP──> [agent-ui process] ──HTTP/WS──> Browser (React shell)
                                  │                             │
                                  └── .agent-ui/sessions/<id>/  └── sandboxed iframe (preview.mjs)
```

## Monorepo layout

pnpm workspaces; `tsup` builds the Node packages, Vite builds the UI; one strict `tsconfig.base.json` (`"module": "NodeNext"` for Node packages, bundler resolution for UI). Root `pnpm build` builds shared → ui → server; the server serves the UI's `dist/` statically (path resolved from `import.meta.url`, never `process.cwd()`).

```
C:\DEV\AgentUI\
├─ package.json / pnpm-workspace.yaml / tsconfig.base.json
├─ .mcp.json                     # registers the server for this repo (dogfooding)
├─ CLAUDE.md                     # two-line pointer to the skill
├─ skills/agent-ui/SKILL.md      # DX doc teaching Claude the workflow
├─ packages/
│  ├─ shared/    # @agentui/shared — zod schemas, types, protocol constants
│  │  └─ src/{index,spec,state,events,ws-protocol,preview-protocol}.ts
│  ├─ server/    # @agentui/server — MCP + web server
│  │  └─ src/{cli.ts, mcp/{tools,heartbeat}.ts, web/{http,ws}.ts,
│  │          core/{session-manager,session,event-log,storage,ports,waiters}.ts, log.ts}
│  └─ ui/        # @agentui/ui — Vite + React shell
│     ├─ public/vendor/three/    # pinned three.module.js + addons (offline, copied at build)
│     ├─ public/preview.html     # sandbox host page (import map + bootstrap)
│     └─ src/{main.tsx, App.tsx, ws.ts, store.ts,
│              components/{ControlRenderer,GroupSection,PreviewFrame,CommentBox,ActionBar,StatusBar}.tsx,
│              controls/{Slider,NumberInput,Select,Toggle,ColorPicker,TextInput,VectorInput}.tsx}
```

Deps (keep boring): `@modelcontextprotocol/sdk`, `zod@^3.23` (v3 — must match the SDK's peer range), `express`, `ws`, `open`, `nanoid`, `react`, `react-dom`, `zustand`, `three` (vendored only — the shell itself never imports it).

## Interface Spec schema (`packages/shared/src/spec.ts`)

Zod is the single source of truth; TS types via `z.infer`. Discriminated union on `type`, 7 control types. Flat `controls` array with optional `group` label (no nesting — easiest for a model to author correctly); the renderer buckets by `group` in first-appearance order.

```ts
const ControlBase = z.object({
  id: z.string().regex(/^[a-z][a-z0-9_]*$/),   // snake_case; uniqueness via superRefine
  label: z.string(),
  description: z.string().optional(),
  group: z.string().optional(),
});

const Control = z.discriminatedUnion("type", [
  ControlBase.extend({ type: z.literal("slider"), min: z.number(), max: z.number(),
                       step: z.number().positive().default(0.01), default: z.number(),
                       unit: z.string().optional() }),
  ControlBase.extend({ type: z.literal("number"), min: z.number().optional(),
                       max: z.number().optional(), step: z.number().optional(), default: z.number() }),
  ControlBase.extend({ type: z.literal("select"),
                       options: z.array(z.object({ value: z.string(), label: z.string().optional() })).min(1),
                       default: z.string() }),
  ControlBase.extend({ type: z.literal("toggle"), default: z.boolean() }),
  ControlBase.extend({ type: z.literal("color"), default: z.string().regex(/^#[0-9a-fA-F]{6}$/) }),
  ControlBase.extend({ type: z.literal("text"), default: z.string().default(""),
                       multiline: z.boolean().default(false), placeholder: z.string().optional() }),
  ControlBase.extend({ type: z.literal("vector"), size: z.union([z.literal(2), z.literal(3)]),
                       default: z.array(z.number()), min: z.number().optional(),
                       max: z.number().optional(), step: z.number().optional() }),
]);

export const InterfaceSpec = z.object({
  schemaVersion: z.literal(1),
  title: z.string(),
  description: z.string().optional(),
  controls: z.array(Control).min(1),
  preview: z.object({ height: z.number().int().min(120).max(1200).default(400) }).optional(),
  layout: z.object({
    columns: z.union([z.literal(1), z.literal(2)]).default(1),
    previewPosition: z.enum(["top", "right"]).default("top"),
  }).optional(),
});
```

Shared helpers: `Values = Record<string, string | number | boolean | number[]>`, `defaultsFromSpec(spec): Values`, `validateValuesAgainstSpec(spec, values)`.

**Validation errors are a DX feature:** format zod issues per-path — `controls[3].default: expected number, got string` — plus a one-line hint pointing at the skill doc. This is what lets the calling model self-correct in one retry. Curve editors / file pickers / 2D pads are explicitly deferred (additive growth within schemaVersion 1).

## On-disk state

Data dir: `AGENT_UI_DATA_DIR` env, default `<cwd>/.agent-ui` (cwd = project root since Claude Code launches the server there). Add `.agent-ui/` to `.gitignore`.

```
.agent-ui/sessions/<sessionId>/
├─ spec.json        # current spec (rewritten on ui_update)
├─ state.json       # authoritative live state
├─ events.ndjson    # append-only event log
├─ preview.mjs      # optional, Claude-authored
└─ output.json      # written once on finalize — the deliverable
```

- `state.json`: `{ schemaVersion: 1, sessionId, title, status: "active"|"finalized"|"closed", values, revision, lastEventSeq, createdAt, updatedAt, finalizedAt? }`
- `events.ndjson`: one JSON object per line, `{ seq, ts, type, payload }`. Types: `session_created, spec_updated, values_interim, values_committed, comment, preview_event, preview_error, client_connected, client_disconnected, finalized, closed`.
  - `values_committed` fires on slider release / input blur / "Send to Claude".
  - `values_interim` is throttled drag noise, kept in the log but **never** wakes `ui_wait`.
- `output.json`: `{ schemaVersion: 1, sessionId, title, finalizedAt, values, comments: [{ts, text}], eventCount }`

**Atomic writes (Windows-aware):** write `state.json.tmp` then `fs.rename` (replace-on-rename works on NTFS), wrapped in a 3-attempt retry with 50ms backoff for `EPERM`/`EBUSY` (antivirus scanners hold handles). Events use `fs.appendFile` of single `\n`-terminated lines. Debounce: flush `state.json` at most every 250ms, plus immediately on every committed/comment/finalize event.

## MCP tools (`server/src/mcp/tools.ts`)

All via `server.registerTool` with zod input schemas, `outputSchema` + `structuredContent` results, `readOnlyHint: true` annotations on the read tools.

**Critical stdio rule: nothing may ever write to stdout except the MCP transport.** The logger (`log.ts`) writes to stderr only. Add an ESLint rule banning `console.log` in the server package.

| Tool | Input | Behavior |
|---|---|---|
| `ui_create` | `{ spec, previewModule?, open? = true }` | Validate spec; create session dir + files; lazily start HTTP server; open browser via `open` pkg. Returns `{ sessionId, url, values }`. Invalid spec → `isError` with formatted zod issues. |
| `ui_update` | `{ sessionId, spec?, previewModule?, values? }` | Hot-update mid-session; bumps `revision`; broadcasts `spec_updated` over WS (shell re-renders preserving unchanged control values; preview iframe reloads if module changed). Values pushed by Claude are validated against the spec. |
| `ui_get_state` | `{ sessionId, sinceSeq? }` | Non-blocking. Returns `{ status, values, revision, lastEventSeq, connectedClients, url, events }` — `events` = committed/comment/preview events after `sinceSeq`, capped at 50. |
| `ui_wait` | `{ sessionId?, sinceSeq?, timeoutMs = 240000, wakeOn? = ["values","comment","finalize"] }` | Long-poll. Omitted `sessionId` ⇒ wake on any active session. Returns `{ reason: "event"|"timeout"|"closed", sessionId, events, state }`. `"values"` means `values_committed` only. **Timeout is a normal non-error result.** |
| `ui_read_output` | `{ sessionId }` | Returns `output.json` content + absolute path. Helpful error if not finalized yet. |
| `ui_list` | `{}` | All sessions: id, title, status, url, lastEventSeq, connectedClients. |
| `ui_close` | `{ sessionId, deleteFiles? = false }` | Marks closed, notifies WS clients (UI shows "session ended"), releases waiters with `reason: "closed"`. |

### `ui_wait` vs MCP client timeouts (`mcp/heartbeat.ts`) — the trickiest part

MCP clients time out tool calls (~60s default), but reset the timer when they receive `notifications/progress` for the call (Claude Code supports this; also configurable via `MCP_TOOL_TIMEOUT` env on the client side).

Implementation contract:
- If the incoming request carries `_meta.progressToken`: send a progress notification every 10s for the life of the wait, allowing the full `timeoutMs` (default 240s). In the SDK, the tool handler's second argument (`extra`) exposes `sendNotification` and the request meta — sketch:

```ts
async function uiWait(args, extra) {
  const token = extra._meta?.progressToken;
  let hb: NodeJS.Timeout | undefined;
  if (token !== undefined) {
    let n = 0;
    hb = setInterval(() => extra.sendNotification({
      method: "notifications/progress",
      params: { progressToken: token, progress: ++n },
    }).catch(() => {}), 10_000);
  }
  const effective = token !== undefined ? args.timeoutMs : Math.min(args.timeoutMs, 50_000);
  try { return await waiters.wait(args, effective); }
  finally { if (hb) clearInterval(hb); }
}
```

- If **no** progressToken: clamp the wait to 50s and return `reason: "timeout"`. The skill teaches Claude to just call `ui_wait` again in a loop — `sinceSeq` makes the loop lossless (no events missed between calls).

### Waiters (`core/waiters.ts`)

Each `ui_wait` registers `{ sessionId?, sinceSeq, wakeOn, resolve }` in a list. `EventLog.append()` scans the list after each append and resolves matching waiters with **all** qualifying events since their `sinceSeq` (not just the triggering one). On resolve or timeout, remove from the list. Multiple concurrent waiters are allowed. On session close, resolve its waiters with `reason: "closed"`.

### Web server lifecycle (`core/ports.ts`, `web/http.ts`)

One persistent Express server per MCP process, started on first `ui_create`, never stopped (dies with the process — no child processes anywhere, so no orphan cleanup). Port: try `AGENT_UI_PORT` or 4517, scan 4518–4536 on `EADDRINUSE`. Two Claude Code projects = two MCP processes = two ports, no conflict.

Routes:
- `GET /session/:id` → shell `index.html`
- `GET /assets/*`, `/vendor/*`, `/preview.html` → static from ui dist/public
- `GET /api/session/:id` → `{ spec, state }`
- `GET /sessions/:id/preview.mjs` → the module, `Content-Type: text/javascript`, `Cache-Control: no-store`
- `WS /ws`

Bind `127.0.0.1` only.

## Frontend shell (`packages/ui`)

`App.tsx` fetches `/api/session/:id` (id from `location.pathname`), keeps `{spec, values, status, revision, connection}` in a zustand store, renders `GroupSection`s of `ControlRenderer` (a switch over the 7 types), `PreviewFrame` (only if the session has a preview module), `CommentBox`, `ActionBar`, `StatusBar`.

**WS protocol** (`shared/src/ws-protocol.ts`), every message `{ type, ... }`:
- client→server: `hello {sessionId, lastSeq?}`, `values {values, kind: "interim"|"committed"}`, `comment {text}`, `finalize {}`, `preview_event {name, data}`, `preview_error {message, stack}`
- server→client: `init {spec, values, status, revision, lastEventSeq}`, `spec_updated {spec, values, revision}`, `values_pushed {values}` (Claude changed values via `ui_update`), `finalized {}`, `closed {}`

**Three-tier debounce** (the key UX/noise tradeoff):
1. Local React state + postMessage to the preview iframe **every frame** — the user sees a live 60fps preview.
2. `values` with `kind: "interim"` over WS, throttled to 100ms trailing — keeps the log/`ui_get_state` fresh without spam.
3. `values` with `kind: "committed"` on pointerup/blur — the only value event that wakes `ui_wait`.

`ActionBar`: **"Send to Claude"** sends a `comment` (text optional — empty means "take a look") for a deliberate ping; **"Finalize"** opens a confirm dialog → server writes `output.json` → UI locks read-only with a banner.

`ws.ts`: reconnecting WebSocket, exponential backoff capped at 5s; on reconnect send `hello {lastSeq}` and receive an authoritative `init` (server state always wins). `StatusBar`: connected / reconnecting / session ended.

Dev loop: `vite dev` proxying `/api`, `/ws`, `/sessions`, `/vendor` to `localhost:4517`.

## Preview sandbox contract

Claude writes **exactly one ESM file** (passed as the `previewModule` string), stored as `preview.mjs`. Required shape:

```js
export default {
  setup({ container, spec, values, emit }) { /* build scene into container */ },
  update(values) { /* called on every change, ~60fps during drags */ },
  dispose() { /* optional */ },
};
```

Loading: `PreviewFrame` renders `<iframe src="/preview.html?session=<id>" sandbox="allow-scripts">` — **no** `allow-same-origin`, so the module runs in an opaque origin: no cookies/localStorage, cannot call the server API as the page origin. `preview.html` is served with CSP:
`default-src 'none'; script-src 'self'; img-src 'self' blob: data:; style-src 'unsafe-inline'; worker-src blob:; connect-src 'none'`
— module code can only import same-origin scripts and cannot exfiltrate.

`preview.html` contains the import map and a bootstrap script that: waits for `init` postMessage, dynamically `import()`s `/sessions/<id>/preview.mjs`, calls `setup`, forwards `values` messages to `update`, and wraps everything in try/catch → posts `preview_error {message, stack}` to the parent. The parent surfaces it as a UI banner **and** forwards it over WS into the event log — so Claude sees its own module's stack trace via `ui_wait`/`ui_get_state` and can fix it with `ui_update` (iframe reloads, no page reload).

postMessage protocol (`shared/src/preview-protocol.ts`): parent→iframe `{type:"init", spec, values}`, `{type:"values", values}`; iframe→parent `{type:"ready"}`, `{type:"preview_event", name, data}`, `{type:"preview_error", message, stack}`. Opaque origin forces `postMessage(msg, "*")`; the parent validates every inbound message shape with zod before trusting it.

**Libraries — vendored, no CDN** (must work offline): a build script copies pinned `three.module.js` + `addons/` (OrbitControls etc.) from `node_modules/three` into `ui/public/vendor/three/`. Import map in `preview.html`:
`{"imports": {"three": "/vendor/three/three.module.js", "three/addons/": "/vendor/three/addons/"}}`
so Claude writes ordinary `import * as THREE from "three"`. The skill lists exactly which bare specifiers exist. Adding a library later = drop an ESM build in vendor + one import-map line.

## Lifecycle, concurrency, persistence

- States: `active → finalized → closed`. Create is immediately active; finalized stays viewable read-only until closed. Every transition appends an event and releases matching waiters.
- `SessionManager` holds `Map<string, Session>`; unlimited concurrent sessions on one port; multiple tabs per session fine (server authoritative).
- On MCP restart: `SessionManager.hydrate()` scans the sessions dir, loads every `state.json`, resumes non-closed sessions (event `seq` continues from `lastEventSeq`). Port may differ after restart; URLs are always regenerated from the live port and returned in tool results (`ui_list` re-shares them). Open tabs show the reconnecting banner and recover.
- Leftovers from earlier runs: hydration restores everything, but a session idle longer than `AGENT_UI_STALE_AFTER_HOURS` (default 4, `0` disables) is reported **stale** — derived from `updatedAt`, never persisted, so it un-stales the moment anyone touches it. Stale sessions drop out of `pending()` (the badge and `/api/ping`), fold under "from earlier runs" in the dashboard, and are flagged `stale: true` in `ui_get_state`'s list. They stay fully answerable; only the user's "Clear all" (`POST /api/sessions/stale/close`) or `ui_close` actually closes them. See `core/staleness.ts` for why nothing is auto-closed or skipped at boot.

## Claude Code integration (DX)

`.mcp.json` (project root, for dogfooding):

```json
{ "mcpServers": { "agent-ui": {
    "command": "node",
    "args": ["packages/server/dist/cli.js"],
    "env": { "AGENT_UI_DATA_DIR": ".agent-ui" } } } }
```

### Global installation — auto-load in every project (required, not optional)

The end goal is that these tools are available in **any** Claude Code session on any unrelated project with zero per-project setup. Build for this from the start:

1. **Bin entry:** `packages/server/package.json` declares `"bin": { "agent-ui-mcp": "dist/cli.js" }` and `cli.js` starts with a `#!/usr/bin/env node` shebang.
2. **User-scope registration** (once, after `pnpm build`):

```bash
claude mcp add --scope user agent-ui -- node C:/DEV/AgentUI/packages/server/dist/cli.js
```

   `--scope user` stores the server in `~/.claude.json`, so it auto-loads in **every** session of every project. Use the absolute path to the built `cli.js` (works without publishing to npm; `pnpm link --global` + `agent-ui-mcp` as the command is an equivalent alternative). Do NOT set `AGENT_UI_DATA_DIR` in the user-scope config — the cwd-based default is the feature (next point).
3. **Per-project data isolation is automatic:** Claude Code launches MCP servers with cwd = the project directory, and the data dir defaults to `<cwd>/.agent-ui`, so each project gets its own sessions folder with no config. To keep foreign repos clean, the server writes `.agent-ui/.gitignore` containing `*` when it first creates the data dir (self-ignoring — the user never edits another project's `.gitignore`).
4. **Global skill:** install the skill to `~/.claude/skills/agent-ui/SKILL.md` (personal skills load in all projects). A project-local copy stays in this repo for development. Add a root script `pnpm install:global` that (a) builds, (b) runs the `claude mcp add --scope user` command, (c) copies the skill to `~/.claude/skills/agent-ui/`.
5. **Self-sufficient tool descriptions:** the skill may not always be loaded, so each tool's MCP description must carry the essential contract on its own — the wait-loop workflow, `sinceSeq` cursor usage, and "timeout from `ui_wait` is normal, call it again". The skill is the deep-dive (worked examples, preview authoring rules); the descriptions are the survival kit.

Cost of always-on: negligible — the web server only starts on first `ui_create`; an unused session just has an idle stdio process and seven tool schemas.

`skills/agent-ui/SKILL.md` — the piece that makes model spec-authoring reliable:
1. Workflow narrative: *create → tell the user the URL → keep working → loop `ui_wait` between work chunks → react to comments/committed values (maybe `ui_update`) → on finalize, `ui_read_output` and apply the values.*
2. A **complete worked example**: a "muzzle flash tuning" spec (sliders/color/select) + matching three.js `preview.mjs` + a wait-loop transcript.
3. Authoring checklist: snake_case ids, every control needs `default`, slider needs min/max, preview module must default-export `{setup, update}`, only `three` importable.
4. The timeout contract: "a `timeout` reason from `ui_wait` is normal — just call it again with the same `sinceSeq`."

`CLAUDE.md` gets a two-line pointer to the skill.

## Milestones (build in this order; verify each before moving on)

- **P0 — Scaffold (½ day):** workspace, 3 packages, tsconfigs, builds pass. ✔ `pnpm build` green.
- **P1 — Schema + MCP skeleton (1–2 d):** shared spec/state/events complete with vitest unit tests (defaults extraction, invalid-spec error messages); server with `ui_create/ui_get_state/ui_list/ui_close`, storage layer, HTTP serving a placeholder page. ✔ Via `npx @modelcontextprotocol/inspector`: create a session, see files on disk, get_state echoes values, browser opens placeholder.
- **P2 — Shell end-to-end (2–3 d):** all control renderers, WS sync, comment box, finalize → `output.json`, reconnect banner. ✔ Create a 6-control spec through the inspector, tweak everything in the browser, finalize, verify `output.json` and `events.ndjson` contents.
- **P3 — `ui_wait` + live Claude Code loop (1–2 d):** waiters, heartbeat/clamp logic, `ui_update`, `sinceSeq` semantics. ✔ Register in `.mcp.json`, run a real session: Claude creates an interface, does unrelated work, `ui_wait` wakes on a comment, finalize ends the loop. Test both with and without progress-token support.
- **P4 — Preview sandbox (2–3 d):** `preview.html` + bootstrap + CSP, vendor copy script, error surfacing, `preview_event` round-trip. ✔ The rifle demo: sway/recoil/tint sliders + a three.js box-gun `preview.mjs`; 60fps local updates; `ui_wait` silent during drags, wakes on release; deliberately break the module → Claude-visible `preview_error` → fix via `ui_update` without reload.
- **P5 — Hardening + DX (1–2 d):** restart hydration, port-scan tests, Windows rename retries, SKILL.md refined against real model authoring mistakes observed in P3/P4; `install:global` script + user-scope registration. ✔ Open Claude Code in a brand-new unrelated folder, ask for a tuning UI with no setup: tools auto-load, `.agent-ui/` appears with its self-ignoring `.gitignore`, full create→wait→finalize loop works.

## Risks & gotchas checklist

- **stdout corrupts MCP stdio** → stderr-only logger; lint ban on `console.log` in server; Express/ws must never log to stdout.
- **MCP client timeouts on long waits** → progress heartbeats + 50s clamp fallback + "loop again" contract. Document `MCP_TOOL_TIMEOUT` for users wanting longer single waits.
- **Windows** → `fs.rename` EPERM/EBUSY retries (antivirus); `open` package for browser launch (no `start` quoting hell); no child processes; locate ui dist via `import.meta.url`, never cwd.
- **Port conflicts** → fixed default + scan; port lives only in memory; URL always returned in tool results so a changed port self-heals.
- **Iframe/security** → `sandbox="allow-scripts"` without same-origin + strict CSP + `connect-src 'none'`. Express needs an explicit MIME entry for `.mjs` (`text/javascript`). WebGL works in sandboxed iframes; audio needs a user gesture (documented, not solved).
- **Model-authored spec errors** → per-path zod messages, worked examples in the skill, flat discriminated union keeps error locality good.
- **Event spam** → three-tier debounce; `values_interim` excluded from wake conditions.
- **zod/SDK version drift** → pin zod v3 range matching the SDK peer dependency; single zod instance via the workspace root.

## Extra implementation notes (read before coding)

These are the mistakes most likely to sink the build — check each one:

1. **Never mix up event granularity.** `values_interim` goes in the log only. If it wakes `ui_wait`, Claude gets spammed on every slider pixel and the whole async model collapses. Test this explicitly in P3.
2. **`sinceSeq` is inclusive-exclusive:** "return events with `seq > sinceSeq`". Both `ui_get_state` and `ui_wait` use the same semantics. Every response includes `lastEventSeq` so the caller always has the next cursor.
3. **The SDK tool handler signature** is `(args, extra) => result`; `extra` carries `_meta` (progressToken) and `sendNotification`. If you can't find `sendNotification` on `extra`, you're on an old SDK version — upgrade rather than working around it.
4. **`structuredContent` requires `outputSchema`** on the tool registration. If you return `structuredContent` without declaring `outputSchema`, some clients error. Also always include a human-readable `content: [{type:"text", ...}]` fallback alongside.
5. **The zod `default` field name collides with nothing in zod, but be careful:** `default` is a legit property key in the schema objects (`z.object({ default: z.number() })` is fine) — don't rename it to `defaultValue` in the spec; the short name is friendlier for model authoring.
6. **Session IDs in URLs:** use `nanoid(10)` — long enough to not collide, short enough to read aloud.
7. **`open` on Windows:** just `await open(url)`. Don't shell out to `start`.
8. **Vite base path:** the shell is served from `/`, session routing is client-side off `location.pathname` — configure Express to serve `index.html` for `GET /session/:id` (not a wildcard `*` that would swallow `/sessions/:id/preview.mjs`).
9. **Iframe reload on `ui_update` with a new module:** change the iframe `key`/`src` query param (`?session=<id>&rev=<n>`) — `Cache-Control: no-store` on the module route handles the rest.
10. **Test without a browser first:** every milestone through P3 is verifiable with `npx @modelcontextprotocol/inspector` + manually opening the URL. Don't block on browser automation.
11. **Hydration must tolerate junk:** a sessions dir with a corrupt `state.json` should log a warning to stderr and skip, never crash the MCP server at boot.
12. **Finalize is idempotent:** a second `finalize` (double-click, reconnect race) must be a no-op that returns the existing `output.json`, not an error or a rewrite.

## Critical files

- `packages/shared/src/spec.ts` — the zod Interface Spec everything derives from
- `packages/server/src/mcp/tools.ts` — the seven `ui_*` tools incl. heartbeat logic
- `packages/server/src/core/session-manager.ts` — lifecycle, storage, waiters, hydration
- `packages/ui/src/App.tsx` — generic renderer + WS sync + debounce tiers
- `packages/ui/public/preview.html` — sandbox host: import map, CSP, postMessage bootstrap
