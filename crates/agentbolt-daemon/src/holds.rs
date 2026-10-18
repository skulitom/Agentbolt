//! Read-only working-copy scanner. Empty configuration means no Git processes or repository reads.
use crate::daemon::Shared;
use agentbolt_core::holds::{Config, Scan, WorkingCopy};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
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
    // Git's -z output already uses '/' separators, even on Windows. A literal backslash is a
    // filename character on Unix and must not silently become a different held path.
    if path.chars().any(char::is_control) || path.len() > 2048 || path.contains('\\') {
        return Err("A changed path cannot be represented safely".into());
    }
    paths.insert(path.to_owned());
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

fn is_link(metadata: &std::fs::Metadata) -> bool {
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    };
    linked
}

/// Directory junctions can lead a working-tree walk outside the configured folder. Inspect entries
/// without following links before asking it to walk the working tree. This is a bounded conservative
/// check, not a sandbox against a same-user process replacing directories during the subsequent scan.
fn check_tree_boundaries(root: &Path, submodules: &BTreeSet<PathBuf>) -> Result<(), String> {
    let started = Instant::now();
    let mut pending = vec![root.to_path_buf()];
    let mut entries = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .map_err(|e| format!("cannot check working-copy boundaries: {e}"))?
        {
            let entry = entry.map_err(|e| format!("cannot check working-copy entry: {e}"))?;
            entries += 1;
            if entries > 100_000 || started.elapsed() > Duration::from_secs(2) {
                return Err(
                    "working-copy boundary check exceeded its limit; collision status is unknown"
                        .into(),
                );
            }
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|e| format!("cannot inspect working-copy entry: {e}"))?;
            if is_link(&metadata) {
                return Err("working copy contains a symbolic link or reparse point; its target will not be scanned and collision status is unknown".into());
            }
            if directory == root && entry.file_name() == ".git" || submodules.contains(&path) {
                continue;
            }
            if metadata.is_dir() {
                match std::fs::symlink_metadata(path.join(".git")) {
                    Ok(_) => return Err("working copy contains a nested repository outside the scan scope; collision status is unknown".into()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(format!("cannot check nested repository boundary: {e}")),
                }
                pending.push(path);
            }
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
        if std::fs::symlink_metadata(path.join(".git")).is_ok_and(|metadata| is_link(&metadata)) {
            return Err(
                "repository metadata is a symbolic link or reparse point and will not be scanned"
                    .into(),
            );
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
        let mut submodules = BTreeSet::new();
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
            submodules.insert(
                path.join(std::str::from_utf8(raw).map_err(|_| "submodule path is not UTF-8")?),
            );
            conservative_submodules = true;
        }
        let checked_path = path.clone();
        tokio::task::spawn_blocking(move || check_tree_boundaries(&checked_path, &submodules))
            .await
            .map_err(|e| format!("working-copy boundary check failed: {e}"))??;
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
                "--no-ext-diff",
                "--no-textconv",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    async fn setup() -> (TempDir, WorkingCopy, Config) {
        let dir = TempDir::new("holds-git");
        git(dir.path(), &["init", "--initial-branch=main"])
            .await
            .unwrap();
        git(dir.path(), &["config", "user.name", "Agentbolt Test"])
            .await
            .unwrap();
        git(
            dir.path(),
            &["config", "user.email", "agentbolt-test@example.invalid"],
        )
        .await
        .unwrap();
        git(dir.path(), &["config", "commit.gpgsign", "false"])
            .await
            .unwrap();
        git(
            dir.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/example/project.git",
            ],
        )
        .await
        .unwrap();
        std::fs::write(dir.path().join("tracked.rs"), "original\n").unwrap();
        std::fs::write(dir.path().join("old name.rs"), "original\n").unwrap();
        git(dir.path(), &["add", "--all"]).await.unwrap();
        git(dir.path(), &["commit", "-m", "base"]).await.unwrap();
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        )
        .await
        .unwrap();
        git(dir.path(), &["config", "branch.main.remote", "origin"])
            .await
            .unwrap();
        git(
            dir.path(),
            &["config", "branch.main.merge", "refs/heads/main"],
        )
        .await
        .unwrap();
        let source = WorkingCopy {
            path: dir.path().display().to_string(),
            repo: "example/project".into(),
        };
        let config = Config {
            working_copies: vec![source.clone()],
            ..Default::default()
        };
        (dir, source, config)
    }

    #[tokio::test]
    async fn real_git_scan_includes_dirty_untracked_rename_and_unpushed_paths_without_writing() {
        let (dir, source, config) = setup().await;
        assert!(scan(&source, &config).await.paths.is_empty());
        std::fs::write(dir.path().join("unpushed.rs"), "new commit\n").unwrap();
        git(dir.path(), &["add", "--all"]).await.unwrap();
        git(dir.path(), &["commit", "-m", "unpushed"])
            .await
            .unwrap();
        git(dir.path(), &["mv", "old name.rs", "new name.rs"])
            .await
            .unwrap();
        std::fs::write(dir.path().join("tracked.rs"), "modified\n").unwrap();
        std::fs::write(dir.path().join("untracked café.rs"), "new file\n").unwrap();
        let index = std::fs::read(dir.path().join(".git/index")).unwrap();
        let head = git(dir.path(), &["rev-parse", "HEAD"]).await.unwrap();
        let result = scan(&source, &config).await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(
            result.paths,
            vec![
                "new name.rs",
                "old name.rs",
                "tracked.rs",
                "unpushed.rs",
                "untracked café.rs"
            ]
        );
        assert_eq!(std::fs::read(dir.path().join(".git/index")).unwrap(), index);
        assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]).await.unwrap(), head);
        assert!(!result.conservative_submodules);
    }

    #[tokio::test]
    async fn missing_upstream_wrong_origin_and_exclusions_never_report_clear() {
        let (dir, source, config) = setup().await;
        git(dir.path(), &["config", "--unset", "branch.main.merge"])
            .await
            .unwrap();
        assert!(scan(&source, &config).await.error.is_some());
        let mut wrong = source.clone();
        wrong.repo = "other/project".into();
        assert!(
            scan(&wrong, &config)
                .await
                .error
                .unwrap()
                .contains("origin")
        );
        let mut excluded = config.clone();
        excluded
            .excluded_paths
            .push(dir.path().join("private").display().to_string());
        assert!(
            scan(&source, &excluded)
                .await
                .error
                .unwrap()
                .contains("excluded")
        );
        let mut nested = source.clone();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        nested.path = dir.path().join("nested").display().to_string();
        assert!(scan(&nested, &config).await.error.unwrap().contains("root"));
    }

    #[tokio::test]
    async fn indexed_submodule_holds_its_entire_subtree_without_opening_it() {
        let (dir, source, config) = setup().await;
        let head =
            String::from_utf8(git(dir.path(), &["rev-parse", "HEAD"]).await.unwrap()).unwrap();
        git(
            dir.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},vendor/module", head.trim()),
            ],
        )
        .await
        .unwrap();
        let result = scan(&source, &config).await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.conservative_submodules);
        assert!(result.paths.contains(&"vendor/module".into()));
        assert!(result.paths.contains(&"vendor/module/**".into()));
        assert!(!dir.path().join("vendor/module").exists());
    }

    #[test]
    fn status_parser_refuses_partial_or_unrepresentable_paths() {
        for raw in [
            &b"2 missing-fields\0"[..],
            &b"? bad\nname\0"[..],
            &b"? invalid-\xff\0"[..],
            &b"? literal\\backslash\0"[..],
            &b"unknown\0"[..],
        ] {
            assert!(dirty(raw, &mut BTreeSet::new()).is_err());
        }
    }

    #[test]
    fn boundary_walk_stops_at_nested_repositories_and_skips_gitlink_contents() {
        let dir = TempDir::new("holds-boundaries");
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(nested.join(".git")).unwrap();
        assert!(
            check_tree_boundaries(dir.path(), &BTreeSet::new())
                .unwrap_err()
                .contains("nested repository")
        );
        assert!(check_tree_boundaries(dir.path(), &BTreeSet::from([nested])).is_ok());
        // Git metadata is not a second working tree to traverse; Git itself reads its index/config.
        std::fs::create_dir_all(dir.path().join(".git/objects/nested/.git")).unwrap();
        assert!(
            check_tree_boundaries(dir.path(), &BTreeSet::from([dir.path().join("nested")])).is_ok()
        );
    }
}
