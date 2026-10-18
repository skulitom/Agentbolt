# Preparing a release

Work on a clean committed checkout. Update the workspace version, lockfile and release notes together.
Use Rust 1.98.1. The release workflow runs Windows workspace checks, Linux core/IPC/MCP checks, clippy,
formatting and the license policy. The interactive checks and the uninterrupted 72-hour run in `NEXT.md`
are still required; CI does not substitute for them.

For a local Windows candidate:

```powershell
$env:AGENTBOLT_CHANNEL = 'release-check'
cargo deny check licenses
cargo about generate --locked --fail --target x86_64-pc-windows-msvc about.hbs --output-file target/THIRD_PARTY_NOTICES.md
cargo run --locked -p xtask -- package windows-x64 target/THIRD_PARTY_NOTICES.md target/dist
winget validate --manifest target/dist/winget --disable-interactivity
```

Pin cargo-deny to 0.20.2 and cargo-about to 0.9.2 (with its `cli` feature), as the workflow does. Use a fresh
output folder for every candidate. The packager builds the programs itself using the locked dependency graph
and reads their paths from Cargo. It refuses dirty sources, source changes during the build, mismatched CLI
versions and existing output folders. Stable ZIP timestamps and ordering make identical inputs produce identical ZIPs.
It does not install, tag, push, sign or publish anything.

To run the real installer fixture against an extracted Windows ZIP, set `AGENTBOLT_INSTALL_TEST=1`,
`AGENTBOLT_INSTALL_TEST_EXE` to its absolute `agentbolt.exe` path and `AGENTBOLT_TEST_DAEMON_SESSION` to the
isolated desktop's session number. Run the `installer` integration test inside that desktop. It creates
its own fresh channel and verifies installation, upgrade, an open MCP exchange and cleanup.

For the final ten-minute idle/reopen check inside the isolated desktop, set `AGENTBOLT_MEASURE_IDLE=1`
and `AGENTBOLT_STARTUP_TEST_EXE` to the extracted ZIP's absolute `agentbolt-desk.exe` path, then run
`cargo test --locked -p agentbolt-desk --release --test startup warm_desk_reopens_fast_without_showing_unseen_asks -- --ignored --nocapture --test-threads=1`.
The probe measures supervisor, daemon and hidden Desk together, fails if any process exits, and then checks
five reopen cycles and unseen-question safety. Run no other copy of that extracted daemon during the probe.

For Linux, generate notices with `--target x86_64-unknown-linux-gnu`, then use
`package linux-x64` with the same remaining arguments. Linux binaries retain executable permission in the ZIP.
Notices cover the workspace's selected target, including the packaging tools, rather than understating licenses.

A manual **Release candidate** workflow run uploads review artifacts only. A future maintainer-created `v*`
tag must exactly match the workspace version. That path creates an **unpublished draft** with Windows and Linux
ZIPs, generated manifests, SHA256SUMS and GitHub attestations. Review its checks and evidence before publishing.
Keep the release-note path in the workflow aligned with the version. The workflow has not been dispatched as
part of preparing 0.2; do not claim a successful hosted build until one is observed.

## Distribution

The Windows packager also includes the version-matched Claude plugin ZIP, with its skill, MCP configuration,
isolated eval suite and MIT license. Only committed plugin files enter that archive. Validate it and run its
evals as described in `plugins/agentbolt/README.md` before calling it ready.

The packager emits a winget singleton manifest, a Scoop manifest and an MCP Registry entry. Checksums always
come from the ZIP it just wrote. The winget Installed Apps product code is `Skulitom.Agentbolt`, matching the
main channel's per-user uninstall registry key. Scoop invokes the same installer and skips uninstall during
updates so the installer can preserve live sessions. Scoop deliberately exposes no shim to the extracted
MCP binary: clients must use the installed copy beside the verified daemon. Neither package deletes user data.

The MCP Registry entry uses its documented custom-installation guide format. It does not claim a Cargo or
MCPB package has been published, nor a measured Glama score. Check Registry acceptance and Glama separately
when publishing is authorized. Do not upload generated manifests containing future release URLs yet.
