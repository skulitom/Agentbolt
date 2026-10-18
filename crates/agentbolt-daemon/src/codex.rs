//! Codex delivery (PLAN.md, "Getting answers back"): for an owner Artem marked codex-interactive, the service emits
//! `Effect::CodexQueue` and the daemon runs `codex queue --thread=<thread> --message=<text>`.
//!
//! - No sandbox, approval, profile or config flag is ever passed: the resumed turn must run under the thread's own
//!   policy (M1 still has to verify that for idle and finished threads on CLI 0.160).
//! - The native `codex.exe` is run directly, never through `codex.cmd`/cmd.exe, so answer text can't be parsed
//!   as shell syntax. For an npm install the exe is found under the package, with the environment `codex.js` sets.
//! - Detached: its own process group, no console window (`CREATE_NO_WINDOW`, so its children share one hidden
//!   console), stdin/stdout/stderr on NUL, no daemon handles inherited (the daemon creates none inheritable).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long to wait for `codex queue` to exit before recording the delivery as unconfirmed.
pub const WAIT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExe {
    pub path: PathBuf,
    pub env: Vec<(String, String)>,
}

/// Find the native codex executable on PATH.
pub fn find() -> Option<CodexExe> {
    let path = std::env::var_os("PATH")?;
    find_in(&std::env::split_paths(&path).collect::<Vec<_>>())
}

pub fn find_in(dirs: &[PathBuf]) -> Option<CodexExe> {
    let exe = format!("codex{}", std::env::consts::EXE_SUFFIX);
    if let Some(dir) = dirs.iter().find(|d| d.join(&exe).is_file()) {
        return Some(CodexExe {
            path: dir.join(&exe),
            env: Vec::new(),
        });
    }
    if !cfg!(windows) {
        return None;
    }
    // An npm install puts codex.cmd on PATH and the native exe inside the package.
    let (package, triple) = if cfg!(target_arch = "aarch64") {
        ("codex-win32-arm64", "aarch64-pc-windows-msvc")
    } else {
        ("codex-win32-x64", "x86_64-pc-windows-msvc")
    };
    for dir in dirs.iter().filter(|d| d.join("codex.cmd").is_file()) {
        let root = dir.join("node_modules").join("@openai").join("codex");
        let candidates = [
            root.join("node_modules")
                .join("@openai")
                .join(package)
                .join("vendor"),
            dir.join("node_modules")
                .join("@openai")
                .join(package)
                .join("vendor"),
            root.join("vendor"),
        ];
        for vendor in candidates {
            let native = vendor.join(triple).join("bin").join("codex.exe");
            if native.is_file() {
                let env = vec![
                    ("CODEX_MANAGED_BY_NPM".to_string(), "1".to_string()),
                    (
                        "CODEX_MANAGED_PACKAGE_ROOT".to_string(),
                        root.to_string_lossy().into_owned(),
                    ),
                ];
                return Some(CodexExe { path: native, env });
            }
        }
    }
    None
}

/// The arguments, in `--flag=value` form so a message starting with `-` stays a value.
pub fn args(thread: &str, text: &str) -> Vec<String> {
    vec![
        "queue".into(),
        format!("--thread={thread}"),
        format!("--message={text}"),
    ]
}

/// Run `codex queue` and wait for it (up to `wait`). Returns (ok, detail) for the delivery record.
pub fn run(exe: &CodexExe, thread: &str, text: &str, wait: Duration) -> (bool, String) {
    let mut cmd = Command::new(&exe.path);
    cmd.args(args(thread, text))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in &exe.env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return (
                false,
                format!("could not start {}: {e}", display(&exe.path)),
            );
        }
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return (true, "Codex accepted the queued message; receipt is unconfirmed. The answer remains available through desk_answers.".into());
            }
            Ok(Some(status)) => return (false, format!("codex queue failed ({status})")),
            Ok(None) if started.elapsed() >= wait => {
                // Leave it running (it may be carrying the turn); the delivery is unconfirmed.
                return (
                    false,
                    format!("codex queue still running after {} s", wait.as_secs()),
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return (false, format!("codex queue: {e}")),
        }
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_carry_no_sandbox_or_approval_flags() {
        let a = args(
            "019a-thread",
            "-s danger-full-access --dangerously-bypass-approvals-and-sandbox",
        );
        assert_eq!(a[0], "queue");
        assert_eq!(a[1], "--thread=019a-thread");
        assert!(
            a[2].starts_with("--message=-s danger"),
            "a hostile message stays one value"
        );
        assert_eq!(a.len(), 3);
        for flag in [
            "-s",
            "--sandbox",
            "--approve-for-me",
            "-c",
            "--config",
            "-p",
            "--profile",
            "-C",
            "--cd",
        ] {
            assert!(!a.iter().any(|x| x == flag), "{flag} is never passed");
        }
    }

    #[test]
    fn the_native_exe_is_found_behind_an_npm_shim() {
        let tmp = crate::testutil::TempDir::new("codex");
        let npm = tmp.path().join("npm");
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&npm).unwrap();
        assert_eq!(find_in(&[empty.clone(), npm.clone()]), None);
        if cfg!(windows) {
            std::fs::write(npm.join("codex.cmd"), "@echo off").unwrap();
            assert_eq!(
                find_in(&[empty.clone(), npm.clone()]),
                None,
                "a shim alone is never run"
            );
            let triple = if cfg!(target_arch = "aarch64") {
                "aarch64-pc-windows-msvc"
            } else {
                "x86_64-pc-windows-msvc"
            };
            let package = if cfg!(target_arch = "aarch64") {
                "codex-win32-arm64"
            } else {
                "codex-win32-x64"
            };
            let bin = npm.join(format!(
                "node_modules/@openai/codex/node_modules/@openai/{package}/vendor/{triple}/bin"
            ));
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("codex.exe"), "").unwrap();
            let found = find_in(&[empty.clone(), npm.clone()]).expect("found");
            assert_eq!(found.path, bin.join("codex.exe"));
            assert!(
                found
                    .env
                    .iter()
                    .any(|(k, v)| k == "CODEX_MANAGED_BY_NPM" && v == "1")
            );
        }
        // A codex executable directly on PATH wins.
        let direct = tmp.path().join("direct");
        std::fs::create_dir_all(&direct).unwrap();
        let exe = direct.join(format!("codex{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, "").unwrap();
        assert_eq!(find_in(&[direct, npm]).unwrap().path, exe);
    }

    #[test]
    fn a_missing_exe_is_a_failed_delivery() {
        let exe = CodexExe {
            path: PathBuf::from("Z:/nowhere/codex.exe"),
            env: Vec::new(),
        };
        let (ok, detail) = run(&exe, "t", "x", Duration::from_secs(1));
        assert!(!ok);
        assert!(detail.starts_with("could not start"), "{detail}");
    }
}
