# One Desk, agents on other machines

The MCP front end can use an SSH relay to the computer that hosts the Desk. It needs no Agentbolt
HTTP listener or public answer endpoint. Install Agentbolt beside its daemon on the Desk computer and
use an SSH account belonging to that installation's user. Configure key authentication and verify its
host key through your normal SSH setup before enabling the relay. Agentbolt does not install SSH,
enroll keys, change a firewall, or accept a new host key automatically.

On the agent's computer, configure the MCP command as:

```text
agentbolt mcp --ssh desk-pc --machine laptop --owner my-project --remote-exe C:/Users/you/AppData/Local/Programs/Agentbolt/agentbolt.exe
```

`desk-pc` may be a configured SSH alias or `[user@]hostname`; configure ports, keys and jump hosts in
SSH's host configuration. `--channel` chooses the Desk's channel. `--machine` is a stable label for the
agent's originating computer, taken from the host configuration, never from tool arguments. Use
`--max-wait-ms 600000` for a Claude client that should permit long waits. The remote executable defaults
to `agentbolt` if it is already on the SSH account's PATH; otherwise supply its literal installed path.

The relay runs `agentbolt relay --machine <label>` over encrypted stdio. It accepts only the typed MCP
protocol and forwards it to the local user-only MCP pipe. There is no human-answer operation. Session
metadata still comes from the originating host. Unbound owner identities include the machine label;
explicitly configured stable owners may intentionally share their durable answers across machines.

Connections use noninteractive authentication, strict host-key verification, no agent/X11/port forwarding,
bounded frames and timeouts. Safe reads may reconnect and retry. Failed writes can have an unknown outcome:
read their durable state or reuse their dedupe key before retrying. The pool permits other calls while a
long answer read waits. Closing the MCP process closes its SSH children; it does not own the remote daemon.
These choices follow [OpenSSH's client documentation](https://man.openbsd.org/ssh_config.5).

The Desk displays the originating machine. A remote session's folder or chat is never opened on the Desk
computer, and local Codex delivery is disabled for remote asks. Copy the session reference to open it on
its originating machine. Remote agents receive durable answers through their MCP checkpoint reads.

SSH account access is the authentication boundary; machine labels describe origin, not separate users.
The same-user trust limit still applies. For a restricted SSH key, an administrator can configure a forced
`agentbolt relay` command and restrict forwarding in SSH itself. This repository does not modify account
or SSH configuration. Live cross-machine validation follows the full feature implementation.
