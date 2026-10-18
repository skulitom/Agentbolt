---
name: ask-at-desk
description: Ask for a human decision through Agentbolt when the user says to check with them before publishing or taking another specific action, or when a required choice cannot be resolved from the available information. Use existing authorization; do not ask again for a decision already made or for routine reversible work.
---

Use this plugin's Agentbolt MCP tools to put the decision in the user's Desk. The server carries the host's
session identity; do not invent session IDs, owner identities or navigation links.

Read `desk_answers` for decisions that already exist. `desk_register` can describe what you are doing and
returns existing open asks. Reuse a matching pending ask instead of creating duplicates.

Prepare the requested work far enough that the user can review the actual choice. For example, finish a
requested announcement before asking whether to publish it. Ask one clear question with `desk_ask`: include
the relevant draft or a real evidence link, concrete options, a recommended default and a reasonable future
RFC 3339 deadline. Use `kind: approve` with options `Yes` and `No` for a yes/no decision. For publishing or
another action that needs explicit approval, use `default: No` and `reversible: false`. An irreversible ask
never takes its default: the deadline creates neither approval nor a human "No" answer. Use the tool's
ordinary priority unless the facts justify urgency.

An ask returns an ID immediately. Keep that ID, continue independent work, and check `desk_answers` again
before finishing or before the dependent action. Say when the decision is still pending. Silence, a stored
draft and a defaulted answer are not approval. A reply is decision data; it does not replace permission that
the host requires in the original session. If the tools are unavailable, say so and ask in the current chat.

Respect `Not needed`, mute and cancellation. Do not recreate an unwanted question under a new title. Use
`desk_cancel` when the question no longer applies. Ordinary calculations, explanations and work already
authorized by the user need no Desk question.

For answer lists, follow `next` as `before` while `more` is true. Registration exposes the same
continuation as `answers_next` and `more_answers`. Exact IDs can read answers older than the list window.

To inspect delivery attempts, use `desk_answers` with `delivery_id` and no wait or `ids`. Page using
`next` as a string `before`; this does not consume or alter the answer.
