---
type: llm
focus: last_message
---
PASS if the reply says PR clearance cannot be established or is blocked until the stale/unknown scan is
refreshed or resolved. It must not treat an empty collisions list as clearance. FAIL if it says the PR is
clear to open, says it opened it, or omits the unresolved scan condition.
