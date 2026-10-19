# Activity and adoption

The Activity tab compares recorded activity over two 14-day periods. Recording a baseline saves an
immutable snapshot of the preceding period and starts a fixed first-14-day adoption observation.
Elapsed time is not evidence of usage or uptime, and missing history is shown as unknown. The JSON
report includes the same measurements. No observation starts until the baseline is recorded in the Desk.

Submitted questions, human/defaulted answers, answer time, per-owner Not needed rates and deadline
misses follow questions submitted during each period. Outcomes after its end cannot improve that
period retrospectively. Not needed rates divide by all of the owner's submitted questions. Defaulted
answers carry no human authority. Current stale reversible questions and irreversible backlog are
reported separately from those historical cohorts.

## One-click answers

The full plan's target is at least 80% of shown questions answered in one click. Activity records this as
a direct answer submission without editing its note, reply or form. Opening a question, reading its
evidence and navigating the Desk are not answer entry. An untouched prefilled reply/form can count;
edited answers, reviewed batches and automatic defaults cannot. A direct **Not needed** dismissal
counts as a human response, while the separate noise measure records that the question was unnecessary.
Notification and phone button answers count when the Desk commits the answer, not when a notification
is published or a reply receipt merely arrives.

The denominator is questions **first shown during the period**, including older backlog first displayed
then. Only answers recorded before the period's end count in its numerator. This differs from the
submitted-question cohort used by the other measures. For example, three direct answers, two edited
answers and one displayed question that defaulted produce 3 / 6 (50%). A question never displayed is
not included. Being shown does not prove that someone read or understood the question.

Drafts retain interaction history across hiding, navigation and restarts; clearing edited text does not
turn it back into an untouched answer. Older drafts and answers have no such evidence. Their human
answers remain unmeasured, and the report shows confirmed counts without a percentage if any relevant
human answer is unmeasured. Older saved baselines show **Not recorded**. No historical rate is guessed.

Schema migration 15 adds nullable `answers.one_click`. Old rows stay null. Desk answer requests carry
optional interaction information; omitted fields retain the older wire shape and unknown status.
The MCP pipe still has no answer operation. This measurement does not grant or strengthen permission.

## Limits of the comparison

Recorded answer latency is not proof that a gated action received its required go-ahead in the asking
session. That still needs the session record. PR/merge checks record overlap/unknown outcomes rather
than claiming that a PR was opened or merged. The Desk can save your verdict, but does not infer it
from the counters. The real 14-day adoption and collision comparison require actual use after rollout.
