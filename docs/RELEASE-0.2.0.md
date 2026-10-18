# Agentbolt 0.2.0 — unsigned release candidate

This candidate is being verified. It has not been tagged or published. See
[NEXT.md](../NEXT.md) and [the evidence log](PRODUCTION-READINESS.md) for outstanding checks.

The Desk stays warm so opening it is fast. Every question goes directly into the Inbox, ordered by priority
and deadline. Daily budgets and the Digest are gone. **Not needed** tells an agent that a question was unnecessary;
**Mute this agent** keeps its questions silent and at the bottom. Notifications remain silent and are grouped
per agent. **Open chat** links questions and agents to their originating session where the host supplies one.

Edited answers can be sent as an atomic batch. Unsent drafts survive restarts and coordinated upgrades.
Optional yes/no fields retain “Not answered” unless a value is chosen or supplied as the question's default.
Owner-name proposals require approval in the Desk. A supervisor restarts a failed daemon; installed tasks and
pipe discovery are isolated by Windows account. Upgrades preserve open MCP clients and reopen visible Desks
in their original Windows sessions. Windows Installed Apps can detect and remove the installation.

Codex answer wake remains explicitly opt-in per owner. Live checks confirm receipt in active and idle loaded
chats with their original sandbox and approval policy. An unloaded chat did not automatically resume on the
tested Codex version even though its queue command succeeded. Queue acceptance is now recorded separately
from confirmed delivery; answers remain in normal checkpoint reads when the chat resumes. The database
upgrade preserves explicit relay confirmations and resurfaces older unconfirmed queue records within the
normal 14-day checkpoint window. Exact-ID reads remain available afterward. Archived chats reject the
queue command. See the live-client evidence before
relying on unattended wake for a particular host version.

## Install or upgrade on Windows x64

1. Download the Windows ZIP and `SHA256SUMS` from the same release. Compare the ZIP with
   `Get-FileHash .\agentbolt-0.2.0-windows-x64.zip -Algorithm SHA256` before extracting it.
2. Extract all files together, then run `.\agentbolt.exe install` from that folder.
3. Run `& "$env:LOCALAPPDATA\Programs\Agentbolt\agentbolt.exe" clients` for MCP setup commands.
   Use the installed executable for MCP, status and start operations.

If the existing Desk predates draft-preserving upgrades, the installer refuses before replacing anything.
Finish or copy its unsent drafts, quit Agentbolt from the tray, and run the installer again. Restart older
client sessions after the upgrade so they use the new task startup path.

Uninstall through Windows Installed Apps, or run the installed program with `uninstall`. Questions and answers
remain on disk. Only an explicit `uninstall --purge` removes that data. MCP registrations are managed separately
by each client; uninstall prints the removal commands.

The Linux x64 ZIP contains the CLI/MCP front-end for protocol discovery and inspection. The installed Desk,
Windows notifications and sign-in integration require Windows; this release does not promise a Linux GUI.

## Provenance and limitations

Each ZIP includes the MIT license, third-party notices and `release.json` with the source revision and binary
hashes. The release workflow produces SHA-256 checksums and GitHub build attestations, and creates a draft
release only. An attestation proves provenance; it is not Windows code signing. Code signing is deferred
beyond 0.2 by the maintainer's decision.

Winget, Scoop and MCP Registry manifests are prepared from the archive's actual checksum. Their presence is
not a claim that a public package repository has accepted them. Do not publish a manifest before its matching
release URL is available and the remaining readiness checks pass.
