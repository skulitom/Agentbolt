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

#[test]
fn only_admitted_files_are_tracked() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("git").arg("-C").arg(&root).args(["ls-files", "-z"]).output();
    let Ok(out) = out else {
        eprintln!("git not available; skipping the repo guard");
        return;
    };
    if !out.status.success() {
        eprintln!("not a git checkout; skipping the repo guard");
        return;
    }
    let listed = String::from_utf8_lossy(&out.stdout);
    let refused: Vec<&str> = listed
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let ext_ok = std::path::Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e));
            !(ext_ok || EXACT.contains(p))
        })
        .collect();
    assert!(refused.is_empty(), "files the repo guard doesn't admit: {refused:?}");
}
