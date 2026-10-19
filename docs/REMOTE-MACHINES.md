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

If the SSH account uses PowerShell as its command shell, add `--remote-shell powershell`. This selects
PowerShell's call syntax for the quoted executable, including installation paths containing spaces.
The default `sh` syntax also works with Windows Command Prompt; `--remote-shell cmd` names that choice
explicitly. Choose the shell on the **Desk computer's SSH account**, regardless of the agent computer's
operating system. Agentbolt does not detect or change the server's configured shell. Windows supports
[configuring an SSH command shell](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh-server-configuration),
and PowerShell requires its [call operator](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_operators?view=powershell-7)
to run a quoted executable path. Shell startup profiles must keep stdout clear for the relay protocol.

For a host-specific configuration, add `--ssh-config "C:/Users/you/Agentbolt/ssh config"` (or an
absolute Unix path). The file must already exist and is read by the local SSH client. This keeps
ports, identities and jump-host settings separate from your shared SSH configuration; it does not
copy the file to the Desk computer. Without this option, SSH uses its usual configuration files.
Agentbolt still enforces noninteractive key authentication, verified host keys and disabled forwarding.
As with your normal SSH configuration, use a file you control: SSH configuration can itself invoke
local helpers for connections and proxies.

The relay runs `agentbolt relay --machine <label>` over encrypted stdio. It accepts only the typed MCP
protocol and forwards it to the local user-only MCP pipe. There is no human-answer operation. Session
metadata still comes from the originating host. Unbound owner identities include the machine label;
explicitly configured stable owners may intentionally share their durable answers across machines.

Connections use noninteractive authentication, strict host-key verification, no agent/X11/port forwarding,
bounded frames and timeouts. Safe reads may reconnect and retry. Failed writes can have an unknown outcome:
read their durable state or reuse their dedupe key before retrying. At most four SSH connections can be
active per MCP process, including at most three long answer waits; this leaves room for asks, renewals
and Board reads. Calls over that limit are refused before submission and can be retried. Closing the
MCP process closes its SSH children; it does not own the remote daemon.
These choices follow [OpenSSH's client documentation](https://man.openbsd.org/ssh_config.5).

The Desk displays the originating machine. A remote session's folder or chat is never opened on the Desk
computer, and local Codex delivery is disabled for remote asks. Copy the session reference to open it on
its originating machine. Remote agents receive durable answers through their MCP checkpoint reads.

File paths and app chat links in remote questions and Board messages also stay on their originating
computer: the Desk offers **Copy reference**, without probing the local filesystem or launching an
app here. HTTPS evidence still opens normally. Board messages retain the sender's machine when posted,
including after its registration expires or another machine retries the same deduplicated post.
Older messages without recorded origin remain readable; their file and chat references are copyable.

SSH account access is the authentication boundary; machine labels describe origin, not separate users.
The same-user trust limit still applies. For a restricted SSH key, an administrator can configure a forced
`agentbolt relay` command and restrict forwarding in SSH itself. This repository does not modify account
or SSH configuration. Live cross-machine validation follows the full feature implementation.

The opt-in `ssh_loopback` fixture exercises a real OpenSSH client against a temporary loopback SSH
endpoint and the actual relay/daemon. It requires Python with Paramiko already installed and a built
daemon beside the CLI. Set `AGENTBOLT_SSH_TEST_PYTHON` to that Python executable, build the CLI and daemon,
then run `cargo test -p agentbolt --test ssh_loopback --locked -- --ignored --nocapture`.
The fixture creates and removes its own channel, keys and configuration; it never registers an install
or uses a real account. This is transport evidence on one computer, not a cross-machine deployment check.

The focused `remote_shells` fixture launches the actual relay from a path containing spaces through
Windows Command Prompt/PowerShell or Unix `/bin/sh`. Empty input confirms clean startup and exit without
starting a daemon. An isolated PowerShell-to-daemon check also preserves Unicode registration and Board
text. These local shell checks do not establish every SSH server's shell invocation or profile behavior.
