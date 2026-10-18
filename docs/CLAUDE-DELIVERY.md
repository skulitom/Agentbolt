# Answers back to Claude

The Claude client setup and bundled Claude plugin now permit a ten-minute `desk_answers` wait.
Other clients keep the 20-second default unless their host passes `--max-wait-ms`. Tool discovery
advertises the effective cap, which never exceeds the daemon's ten-minute ceiling. The daemon polls
durable answers without holding the store lock during the wait; other calls use separate connections.

After asking, retain the ask ID and pass it to `desk_answers` with `wait_ms: 600000` when waiting is
appropriate. A host may background a long-running call. Continue independent work; do not infer an
answer from backgrounding, timeout, or silence. A later run with the same bound owner reads the same
answers at registration or from `desk_answers`. Reading does not consume them.

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

This repository supplies the adapter contract; it does not edit a live dispatcher's charter, append to
Compass's NOTES, send messages, or claim a live Claude turn acted on an answer. Those rollout and host
checks follow feature implementation. Any integration must preserve that dispatcher's maintenance-hour
and messaging rules. Agentbolt never creates an MCP human-answer operation.
