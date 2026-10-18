//! The repo guard: only source, manifests, docs and lock files are tracked, plus a short list of exact paths.
//! Like the guards in Artem's other Rust repos, it keeps generated files, binaries and stray tooling out of `main`.

use std::process::Command;

const EXTENSIONS: &[&str] = &["rs", "toml", "md", "json", "lock"];
const EXACT: &[&str] = &[
    ".gitattributes",
    ".gitignore",
    "LICENSE",
    ".github/workflows/ci.yml",
    "about.hbs",
];

/// Whether the guard admits a tracked path (as `git ls-files` prints it: `/`-separated, relative to the root).
fn admitted(path: &str) -> bool {
    let ext_ok = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e));
    ext_ok || EXACT.contains(&path)
}

/// The tracked paths the guard refuses, from `git ls-files -z` output.
fn refused(listed: &str) -> Vec<&str> {
    listed
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| !admitted(p))
        .collect()
}

#[test]
fn the_classifier_refuses_stray_files() {
    for ok in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/agentbolt/src/main.rs",
        "docs/PRIOR-ART.md",
        "LICENSE",
        "x.json",
    ] {
        assert!(admitted(ok), "{ok} should be admitted");
    }
    for bad in [
        "crates/agentbolt/src/tool.ts",
        "index.ts",
        "package.js",
        "build.ps1",
        "assets/icon.ico",
        "LICENSE.txt",
        "src/main.RS",
        "Makefile",
        ".github/workflows/release.yml",
        "x.rs.bak",
    ] {
        assert!(!admitted(bad), "{bad} should be refused");
    }
    let listed = "Cargo.toml\0crates/agentbolt-core/src/lib.rs\0web/stray.ts\0README.md\0";
    assert_eq!(refused(listed), ["web/stray.ts"]);
}

#[test]
fn only_admitted_files_are_tracked() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output();
    let Ok(out) = out else {
        eprintln!("git not available; skipping the repo guard");
        return;
    };
    if !out.status.success() {
        eprintln!("not a git checkout; skipping the repo guard");
        return;
    }
    let listed = String::from_utf8_lossy(&out.stdout);
    let refused = refused(&listed);
    assert!(
        refused.is_empty(),
        "files the repo guard doesn't admit: {refused:?}"
    );
}
