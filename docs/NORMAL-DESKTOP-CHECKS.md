# Remaining normal-desktop checks

Deferred until implementation and focused checks are complete, per Artem's 22:01 UTC instruction.
Use the new unsigned 0.2.0 candidate built after the implementation audit. `7e4d081` is historical.
The Anode desktop cannot emit Windows notifications (0x803E0105) or observe Claude running in the main
desktop. These checks require Artem to observe the normal desktop. The earlier authorization to create
synthetic test chats still applies; this is a request for observations, not a new authorization requirement.

## Preparation after the final candidate is built

Create a fresh `manual-<timestamp>` channel from the final extracted Windows candidate's
`agentbolt.exe`, with `install --no-start`, then run its exact
verified sign-in task in Windows session 1. Never use channel `main`. Record its paths, task and process IDs
in `target/manual-fixture.json`. Keep it separate from any later reliability fixture. The previous
`soak-7e4d081-1018` fixture was stopped and removed; its monitor is paused.

Prepare `target/manual-client-check/.mcp.json` with only an `agentbolt_test` connection to a byte-preserving
stdio recorder and this installation. The recorder may log the synthetic JSON-RPC and only
`CLAUDE_CODE_HOST_SESSION_ID`, `CLAUDE_CODE_SESSION_ID` and `CODEX_THREAD_ID`; do not capture the rest of the
environment. It must never manufacture host metadata. No global client setting is changed.

## Observations

1. From the isolated manual channel, post a reversible synthetic Blue/Green choice, with default Green and
   priority P1. Artem confirms a notification appeared without sound, then chooses **Blue on the notification**.
   Verify the durable answer says `via: toast` and `choice: Blue` through MCP. A second question from a separate
   test owner checks that clicking the notification body opens this channel's Desk. Core tests separately
   cover replayed and wrong-option token refusal; record any live replay check explicitly rather than assuming it.
2. In Claude Desktop's Code tab, open `C:\DEV\Agentbolt\target\manual-client-check` and create one temporary
   chat named **Agentbolt test — navigation**. Enable its local test connection if Claude asks. Use this prompt:

   > This is the authorized isolated Agentbolt transport check. Use only agentbolt_test; never use the normal
   > agentbolt connection. List its tools and call desk_answers. Ask one synthetic choose question titled
   > “Synthetic Agentbolt navigation check”, options Test A and Test B, default Test A, reversible false,
   > priority P2, deadline 2026-10-22T18:00:00+01:00. Body: “Synthetic test only; no real-world authorization.”
   > Report the ask ID and whether both content text and structuredContent are present, then stop. Do not edit
   > files or perform any action based on a test value. When prompted later, read this same ask once and report
   > its answer. Do not invent a host session ID or URL.

3. Inspect the actual recorder environment to confirm the host ID and terminal ID. From both the question
   and Agents row, click **Open chat** and have Artem confirm it selects that exact Claude chat. Answer Test B
   through the test Desk and confirm the client reads it through both result branches.
4. Repeat visible **Open chat** from an ask and Agents row for one authorized temporary Codex chat. Codex's
   transport and active/idle receipt are already verified; visible destination selection is the missing evidence.
5. Create a persistent synthetic Claude CLI session in its own folder and retain its real host session ID.
   From both the question and Agents row, confirm **Resume in a terminal** displays that exact saved
   conversation. The 2026-10-19 hidden-desktop attempt encountered this PC's Terminal setting that runs
   profiles as administrator. It did not establish a resume pass. Any consent must come from Artem;
   do not change his Terminal defaults or automate the secure prompt. **Copy resume command** is an
   independently verified fallback for use in an existing terminal in the recorded folder.

Record exact outcomes, host versions and timestamps, including failures. Then close/archive only the test
chats, remove only their temporary project connection, and uninstall/purge the `manual-*` fixture through its
source CLI. Do not alter the main installation or the ongoing reliability fixture. A refused cleanup remains
documented and is not retried through an alternate method.
