# Beads handoff adapter

Link one existing Beads issue to a Board handoff:

```text
agentbolt beads --owner project-builder --repo example/project --issue bd-a1b2 --cwd C:/DEV/scratch/project --to-role reviewer --path src/**
```

The adapter calls `bd show --json --id <id>` in the chosen working directory, accepts only an exact ID
match, and creates a deduplicated Board request with the title, description, acceptance criteria, design
and notes. A stable key includes repository and issue ID. Repeating the command returns an existing open
handoff in the same role scope. Closed issues, ambiguous output and oversized content are refused.

To use an already prepared export instead of running Beads, pass `--json-file <file>`. The input may be
one issue object, an array from `bd show --json`, or an object containing an `issue` field. It is bounded
to 1 MiB. The original ID is retained as `beads_issue` and shown in the Desk's Board tab.

Beads remains the source of truth for the issue. Agentbolt does not claim, close, synchronize or modify
it. Completing the Board handoff records a coordination result; it does not silently complete the issue.
The caller remains responsible for its authorized Beads workflow. The CLI interaction is based on
[Beads' show command](https://github.com/gastownhall/beads/blob/main/cmd/bd/show.go).
