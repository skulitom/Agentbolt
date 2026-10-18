# Workflow adapters

These commands implement the Reception and PR-check handoff points. Run them from the installed CLI
when the corresponding workflow is authorized. They do not change scheduled tasks, charters, owner
permissions or working-copy monitoring configuration. In Compass, the ledger stays authoritative,
Reception owns its notes, and existing messaging and maintenance-hour restrictions still apply.

## Reception and the next run

Enable relay permission for the dispatcher's stable owner in the Desk. Choose its authorized notes
file and list the recipient owners explicitly. Start with a preview:

```powershell
agentbolt relay-notes --owner compass-reception --notes C:\DEV\Compass\NOTES.md --recipient-owner compass-builder --recipient-owner compass-followup
```

Add `--write` within Reception's authorized workflow to append and record delivery. The adapter:

- Reads only answers the relay is authorized to list and selects only the named recipient owners.
- Writes one JSON-escaped physical line per answer, marked `(via desk, ask <id>)`, with the owner and
  complete answer text. Dates on appended lines are UTC; the answer itself retains its exact timestamp.
- Locks the selected UTF-8 file, checks for the identical existing line, appends without rewriting
  other notes, and syncs the write before marking delivery to `notes`.
- Can retry after interruption or a lost mark response without adding another complete copy.
- Reports individual failures and exits nonzero if any selected handoff could not be completed.

Only cooperating writers honor the file lock; the owning workflow must prevent concurrent rewrites of
the notes file. The adapter reads at most 4 MiB and refuses larger notes. It handles the current relay
page (up to 100 recent answers); run again to drain further pages after successful marks. If other
owners occupy the relay page, use `desk_relay` to route them through their authorized workflows.

At each owner run's start and finish, read `desk_answers` and the authorized notes file. Notes delivery
does not confirm receipt by a live session and does not consume exact-ID reads. Deduplicate actions by
ask ID. Defaults and Not needed retain their meaning; a default is never permission from Artem.
Reception may send the prepared message into a live session only under its own messaging authority.
This CLI does not send chat messages or restart agents.

## PR creation and merge

Refresh the workflow's isolated clone through its normal process. Before opening a PR:

```powershell
agentbolt pr-check --owner compass-builder --repo owner/project --cwd C:\scratch\project --base origin/main --head HEAD --stage pr_open
if ($LASTEXITCODE -ne 0) { throw 'Resolve work holds before opening the PR' }
```

Repeat immediately before merge, using the fetched current PR head and base commits:

```powershell
agentbolt pr-check --owner compass-merge --repo owner/project --cwd C:\scratch\project --base <base-commit> --head <pr-head-commit> --stage merge
if ($LASTEXITCODE -ne 0) { throw 'Resolve work holds before merging' }
```

The command verifies the repository root and origin, resolves immutable commit IDs, computes the merge
base and reads the complete NUL-separated changed-path list without external diff/text conversion,
hooks, fetches or working-tree changes. Rename source and destination both count. More than 100 paths
or filenames that cannot safely be represented become a conservative whole-repository hold check.
No truncated list can produce clearance. Output includes the exact base/head/merge base and indicates
whether named refs moved during the check. Missing/stale/failed scans and overlaps produce nonzero exit.

Use only that returned head for the subsequent action. For a GitHub merge, the workflow should supply
`gh pr merge --match-head-commit <checked-head>` alongside its existing review and merge conditions.
Recheck if the base, PR head or holds change. This is advisory collision detection, not an atomic lock
on somebody else's checkout and not approval to open or merge a PR. It never edits the monitored folder.

For non-Git workflows or already computed path lists, `agentbolt holds --owner ... --repo ... --path ...
--stage pr_open|merge` and the `work_holds` MCP tool provide the same underlying check.

No live Compass notes or routine prompts were changed by implementing these adapters. Installing the
candidate and enabling the chosen workflows is rollout, after feature implementation and verification.
