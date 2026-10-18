# Prior art

Agentbolt's design came from Artem's own AgentUI and from long-standing ideas. Where another project informed it, it's
credited here. Read on 2026-10-18.

## What it builds on
- **AgentUI** (this repo's history; the `legacy` branch and tags). The native remake's contract carries over: an ask
  returns an id at once, answers are durable and never consumed by reading, silence is never agreement, and Codex
  replies are queued into the asking thread with `codex queue`.
- **Advisory file locks** (`flock`, `lockf`) and **Git LFS file locking**: the idea that a holder declares what it is
  changing, and others check before they start. Agentbolt's "holds" (a later milestone) are this idea, keyed on a
  GitHub repo and path.
- **Leases with fencing tokens**, as in distributed lock services (Chubby; the fencing-token pattern for
  lease-based locks): a claim expires unless renewed, and a late writer with a stale token is refused.
- **[Gas Town](https://github.com/gastownhall/gastown)** (MIT): mailboxes between coding agents, and escalation routed by
  severity. That informed Agentbolt's ask priorities (P0 to P2). Gas Town launches and owns its agents; Agentbolt works
  with agents however they were started.
- **[Beads](https://github.com/gastownhall/beads)** (MIT): an issue tracker for coding agents with a "ready" queue. A
  later adapter may link Board requests to a repo's Beads issues.
- **[LangChain agent-inbox](https://github.com/langchain-ai/agent-inbox)** (MIT, archived): an inbox page for
  human-in-the-loop interrupts. A UX reference only.
- **[ntfy](https://github.com/binwiederhier/ntfy)** (Apache-2.0 / GPL-2.0): the optional count-only phone push is
  sent to an ntfy topic with a plain HTTP request. No ntfy code is included.
- **[rmcp](https://github.com/modelcontextprotocol/rust-sdk)**: the official Rust MCP SDK, used as a dependency.

## Excluded
- **MCP Agent Mail** (Dicklesworthstone/mcp_agent_mail) is excluded. Its licence is MIT plus a rider that grants no
  rights to OpenAI, Anthropic, or anyone acting for or on behalf of them, and defines use to include executing,
  analyzing and incorporating it. Agentbolt is built with those companies' models, so nothing from it is used. On
  2026-10-18 the planning agent read only its licence file and search-engine summaries of its README, and opened no
  code. Every overlapping idea above has the earlier, general source named.
