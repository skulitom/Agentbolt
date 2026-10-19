# Preparing a release

The 0.2 candidate is not ready to tag or publish until the outstanding checks in [NEXT.md](../NEXT.md)
pass. The [feature inventory](FEATURE-INVENTORY.md) and [expanded validation](EXPANDED-VALIDATION.md)
cover the full M2–M7 scope; [PRODUCTION-READINESS.md](PRODUCTION-READINESS.md) retains the earlier
M0/M1 release evidence. Keep generated artifacts under `target/`.

The packager includes tracked Markdown and JSON documentation and the README's linked `AGENTS.md`, so
local evidence and development-guidance links remain readable in the main ZIP. The standalone Claude plugin ZIP points its repository documentation links at the
same source revision as the binaries. Those hosted links become available when that revision is pushed;
packaging does not push, tag or publish it.

## Third-party notices

Use cargo-about 0.9.2, installed with its `cli` feature. Generate against the locked dependency graph for
each shipped target. For the Windows bundle:

```powershell
cargo about generate --workspace --locked --fail --target x86_64-pc-windows-msvc --format json -o target/license-report.json
cargo about generate --workspace --locked --fail --target x86_64-pc-windows-msvc -o target/THIRD_PARTY_NOTICES.md about.hbs
```

Reject the report if any licence has empty text or no `source_path`. A successful cargo-about exit alone
does not establish this: otherwise it can substitute an SPDX template containing placeholder copyright
holders. Include the generated notices and Agentbolt's `LICENSE` in the bundle.

`about.toml` uses hash-checked clarifications where crates omit licence files from their published package,
or combine several licences in a file the scanner does not recognise. Git files normally come from the
commit in that package's `.cargo_vcs_info.json`, never from a moving branch. The egui 0.37.0 family is an
exception: its packaged commit `7c0b40402271e8d40cda69f87cf3c2617c2e4382` returns 404 upstream. Those entries
pin the official `0.37.0` release tag's commit, `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`. The MIT file
also matches eframe 0.37.0's available packaged commit byte for byte. Recheck these entries on dependency
updates rather than copying a new hash without reviewing the licence text.

The rmcp entries preserve the complete upstream licence file, including its licensing-transition notice
and retained MIT terms. The dpi entry preserves both Apache terms and its bundled libm MIT notices.

Initial verification on Windows: 204 crates, 129 distinct notice texts, no empty texts and no generic
fallbacks. `cargo deny check licenses` also passed. Regenerate after the version bump or dependency changes.
