---
name: agentui-native
description: Ask the human through AgentUI's shared local desktop panel while continuing independent work.
---

Use the installed AgentUI feedback tools when a human decision is needed.

1. Call `feedback_request` with a concise question, enough context to decide, and either 2–8 choices or a short form. Fields have `id`, `label`, `type` (`text`, `number`, `boolean`, `choice`), optional `required`, numeric `min`/`max`, and choice `options`. Group related decisions; avoid flooding the inbox.
2. Retain the request ID. The call returns immediately. Continue work that does not depend on the answer.
3. Read outstanding requests with `feedback_read` at natural checkpoints and before concluding. Use its bounded `waitMs` only when no independent work remains. Do not tight-loop while the person is thinking.
4. Apply answered values as human feedback. Omitted optional values express no opinion. Dismissal, cancellation, timeout, drafts and silence are not consent.
5. Cancel obsolete questions with `feedback_cancel` and a short reason.

Codex may queue replies back to the originating task automatically. Check `automaticDelivery` in the creation result. Delivery can repeat after a crash: the same request ID denotes the same decision. Never claim an answer was processed before receiving it. Other stdio hosts need agent-side reads; the human should not have to edit files or send an extra chat message to expose the answer.

Use plain text and native controls. Do not send executable previews or start an HTTP server. Never fabricate the host's task identity.
