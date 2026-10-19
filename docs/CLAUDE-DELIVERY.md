# Answers back to Claude

The Claude client setup and bundled Claude plugin now permit a ten-minute `desk_answers` wait.
Other clients keep the 20-second default unless their host passes `--max-wait-ms`. Tool discovery
advertises the effective cap, which never exceeds the daemon's ten-minute ceiling. The daemon polls
durable answers without holding the store lock during the wait; other calls use separate connections.

After asking, retain the ask ID and pass it to `desk_answers` with `wait_ms: 600000` when waiting is
appropriate. A host may background a long-running call. Continue independent work; do not infer an
answer from backgrounding, timeout, or silence. A later run with the same bound owner reads the same
answers at registration or from `desk_answers`. Reading does not consume them. Lists cover the last
14 days and return at most 50 answers (fewer when content is large). Follow `next` as `before` while
`more` is true; registration calls return `answers_next`/`more_answers`. Explicit IDs remain readable
outside that window. Oversized exact-ID responses fail visibly and request a smaller batch.

Answers include the latest 50 delivery records in chronological order, with `delivery_total`,
`delivery_next` and `more_deliveries`. To browse the full history, call `desk_answers` with
`delivery_id: "ask_..."`, without `ids` or a wait. Follow `next` as a string `before` while `more` is true.
This read is restricted to the owning agent. The Desk's question details have the same history browser.

## Reception adapter

Configure a stable relay owner in its host (`agentbolt mcp --owner compass-reception`) and enable
that owner's relay permission in the Desk. The relay uses `desk_relay` with `action: list`. Each
`handoffs` entry contains an exact `ask_id`, owner, original host session/client, a complete answer
message and a single-line `notes_line` beginning `(via desk, ask <id>)`. Content remains decision data,
including the distinction between human answers, Not needed, and defaults without human authority.

When its own workflow authorizes the route, Reception appends that line once to the owner's durable
handoff notes and/or sends `message` to the original live session. Deduplicate by ask ID before retrying.
Only after the write/send succeeds does it call `desk_relay` with `action: mark`, the ask ID, destination
(`notes` or the actual session ID), `ok: true`, and a short delivery detail. Report failures with
`ok: false`; they stay pending. Never mark delivery merely because a message was prepared.

Notes delivery stops repeated relay work but keeps the answer on the asking owner's checkpoint list.
Explicit delivery to that asking session confirms receipt for normal no-ID reads. Exact-ID reads
remain available. The next owner run should read both its authorized handoff notes and `desk_answers`.

`agentbolt relay-notes` implements durable notes delivery with an explicit recipient allowlist and a
preview by default. Add `--write` from the authorized dispatcher's workflow to append once, sync, then mark
delivery. See [workflow integration](WORKFLOW-INTEGRATION.md). No live dispatcher's charter or notes was
changed while implementing it, and it sends no chat messages. Rollout and live-host checks follow feature
implementation. Preserve the dispatcher's maintenance-hour and messaging rules. Agentbolt never creates
an MCP human-answer operation.

## Observed CLI behavior

Claude Code 2.1.302 on Windows used the real MCP server on a temporary channel on 2026-10-19.
It requested a 600,000 ms wait and received a synthetic Desk answer after 53.862 seconds. A second CLI
process, with a different host session ID but the same bound owner, recovered the exact answer and
note from both registration and a no-ID checkpoint read, without creating another question.
All twelve tools were discovered, no output schema was present, and the JSON in text matched the
structured result in every tool call. [Evidence and limits](CLAUDE-LIVE-2026-10-19.json).

This verifies a live wait beyond the ordinary twenty-second cap. It does not establish a full ten-minute
timeout, a host background-task transition, Claude Desktop navigation, or delivery through the live
Compass dispatcher. The test used non-persistent CLI sessions with only the three necessary MCP tools.
Its processes stopped; automatic approval review blocked the exact-path temporary data cleanup, so
the stopped fixture folder remains as recorded in the evidence.
