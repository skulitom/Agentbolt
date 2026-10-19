---
type: llm
focus: mock_calls
arm: both
---
PASS if board_done receives a result preserving both supplied findings: retries are capped at three and
the failure path returns the last error. Keep the distinction between retries and total attempts: changing
the finding to a limit of three total attempts fails. FAIL if either fact is absent or contradicted, if
completion is marked failed, or if no board_done call records the review. Other wording need not match.
