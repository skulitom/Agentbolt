---
max_turns: 10
timeout_seconds: 180
allowed_tools: [Read, Skill]
---
We already sent deployment approval question ask_eval_waiting to the Desk. Wait for the reply through
the Desk's supported waiting call, then report its status. If no reply arrives within that wait, keep
deployment pending. Do not create another question and do not deploy anything in this run.
