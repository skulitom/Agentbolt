//! Read-only working-copy scanner. Empty configuration means no Git processes or repository reads.
use crate::daemon::Shared;
use agentbolt_core::holds::{Config, Scan, WorkingCopy};
use std::{collections::BTreeSet, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::io::AsyncReadExt;

pub async fn run(shared: Arc<Shared>) {
    loop {
        let service = shared.svc.clone();
        let config = tokio::task::spawn_blocking(move || service.hold_config()).await;
        if let Ok(Ok(config)) = config {
            for source in &config.working_copies {
                // Recheck configuration before each folder, including after a slow previous scan.
                let service = shared.svc.clone();
                let current = tokio::task::spawn_blocking(move || service.hold_config()).await;
                let Ok(Ok(current)) = current else {
                    break;
                };
                if !current.working_copies.contains(source) {
                    continue;
                }
                let scan = scan(source, &current).await;
                let service = shared.svc.clone();
                let _ = tokio::task::spawn_blocking(move || service.save_hold_scan(scan)).await;
            }
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(path)
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "core.untrackedCache=false",
        ])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // No visible console and no shell interpretation of working-copy paths or arguments.
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start Git: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("Git stdout unavailable")?;
    let read = async {
        let mut bytes = Vec::new();
        (&mut stdout)
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        if bytes.len() > 1_048_576 {
            return Err("Git output exceeded 1 MiB; scan is incomplete".into());
        }
        let status = child.wait().await.map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "Git {} failed (exit {:?}); check the working copy and upstream",
                args.first().unwrap_or(&"read"),
                status.code()
            ));
        }
        Ok(bytes)
    };
    match tokio::time::timeout(Duration::from_secs(20), read).await {
        Ok(result) => result,
        Err(_) => Err("Git scan timed out after 20 seconds".into()),
    }
}

fn remote_repo(raw: &[u8]) -> Result<String, String> {
    let remote = std::str::from_utf8(raw)
        .map_err(|_| "remote is not UTF-8")?
        .trim();
    let remote = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
        .unwrap_or(remote);
    agentbolt_core::board::repository(remote)
}

fn add_path(paths: &mut BTreeSet<String>, raw: &[u8]) -> Result<(), String> {
    let path = std::str::from_utf8(raw)
        .map_err(|_| "A changed path is not UTF-8; collision status is unknown")?;
    if path.is_empty() {
        return Ok(());
    }
    if path.chars().any(char::is_control) || path.len() > 2048 {
        return Err("A changed path cannot be represented safely".into());
    }
    paths.insert(path.replace('\\', "/"));
    Ok(())
}

/// Porcelain v2 -z uses fixed metadata fields and raw NUL-terminated paths, including rename sources.
fn dirty(raw: &[u8], paths: &mut BTreeSet<String>) -> Result<(), String> {
    let mut records = raw.split(|b| *b == 0);
    while let Some(record) = records.next() {
        let fields = match record.first() {
            Some(b'1') => 9,
            Some(b'2') => 10,
            Some(b'u') => 11,
            Some(b'?') => 2,
            Some(b'#' | b'!') | None => continue,
            _ => return Err("unrecognized Git status record".into()),
        };
        let path = record
            .splitn(fields, |b| *b == b' ')
            .nth(fields - 1)
            .ok_or("incomplete Git status record")?;
        add_path(paths, path)?;
        if record.first() == Some(&b'2') {
            add_path(paths, records.next().ok_or("missing rename source")?)?;
        }
    }
    Ok(())
}

async fn scan(source: &WorkingCopy, config: &Config) -> Scan {
    let mut paths = BTreeSet::new();
    let mut conservative_submodules = false;
    let outcome = async {
        if agentbolt_core::holds::excluded(&source.path, config) {
            return Err("working copy overlaps an excluded root and will not be scanned".into());
        }
        let path = std::fs::canonicalize(&source.path)
            .map_err(|e| format!("working copy unavailable: {e}"))?;
        let canonical = path
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_owned();
        if agentbolt_core::holds::excluded(&canonical, config) {
            return Err("working copy resolves into an excluded root".into());
        }
        // The chosen folder must be the repository root; never silently scan a parent or a nested repository.
        let root = git(&path, &["rev-parse", "--show-toplevel"]).await?;
        let root = std::str::from_utf8(&root)
            .map_err(|_| "Git root is not UTF-8")?
            .trim();
        let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        if root != path {
            return Err("configured path is not the Git working-copy root".into());
        }
        let remote = git(&path, &["config", "--get", "remote.origin.url"]).await?;
        if remote_repo(&remote)? != source.repo {
            return Err("origin does not match the configured repository".into());
        }
        // Read gitlinks from this repository's index, without opening a nested checkout. Status below
        // deliberately ignores submodules, so their paths must be held conservatively, not called clean.
        let index = git(&path, &["ls-files", "--stage", "-z"]).await?;
        for record in index
            .split(|b| *b == 0)
            .filter(|r| r.starts_with(b"160000 "))
        {
            let raw = record
                .splitn(2, |b| *b == b'\t')
                .nth(1)
                .ok_or("incomplete Git index record")?;
            add_path(&mut paths, raw)?;
            let nested = format!(
                "{}/**",
                std::str::from_utf8(raw).map_err(|_| "submodule path is not UTF-8")?
            );
            add_path(&mut paths, nested.as_bytes())?;
            conservative_submodules = true;
        }
        let status = git(
            &path,
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=all",
            ],
        )
        .await?;
        dirty(&status, &mut paths)?;
        let log = git(
            &path,
            &[
                "log",
                "--format=",
                "--name-only",
                "-z",
                "--no-renames",
                "@{upstream}..HEAD",
                "--",
            ],
        )
        .await?;
        for path in log.split(|b| *b == 0) {
            add_path(&mut paths, path.strip_prefix(b"\n").unwrap_or(path))?;
        }
        Ok::<(), String>(())
    }
    .await;
    let mut error = outcome.err();
    let mut bytes = 0;
    let paths = paths
        .into_iter()
        .take_while(|p| {
            bytes += p.len();
            if bytes > 16_384 {
                error = Some(
                    "More changed paths than the scan limit; collision status remains unknown"
                        .into(),
                );
                false
            } else {
                true
            }
        })
        .collect();
    Scan {
        path: source.path.clone(),
        repo: source.repo.clone(),
        at_ms: agentbolt_core::store::to_ms(time::OffsetDateTime::now_utc()),
        paths,
        error,
        conservative_submodules,
    }
}
