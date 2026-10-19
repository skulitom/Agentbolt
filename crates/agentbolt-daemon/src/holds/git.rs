//! Git reads with explicit metadata boundaries and no repository-configured executable filters.
use super::is_link;
use agentbolt_core::holds::{Config, WorkingCopy};
use std::{
    ffi::OsString,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

#[cfg(windows)]
const NULL_FILE: &str = "NUL";
#[cfg(not(windows))]
const NULL_FILE: &str = "/dev/null";

fn isolate_environment(command: &mut Command) {
    // Includes inherited object/index/config paths, trace output files and injected -c settings.
    // Do not change the daemon's environment (other workers run concurrently).
    let keys: Vec<OsString> = std::env::vars_os()
        .map(|(key, _)| key)
        .chain(command.as_std().get_envs().map(|(key, _)| key.to_owned()))
        .filter(|key| {
            key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
        })
        .collect();
    for key in keys {
        command.env_remove(key);
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", NULL_FILE)
        .env("GIT_CONFIG_GLOBAL", NULL_FILE)
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "");
}

fn command(path: &Path) -> Command {
    let mut command = Command::new("git");
    isolate_environment(&mut command);
    command.current_dir(path).args([
        "--no-pager",
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "status.submoduleSummary=false",
    ]);
    // Prevent implicit reads from the user's config/attributes/excludes, including local overrides.
    command.args(["-c", &format!("core.attributesFile={NULL_FILE}")]);
    command.args(["-c", &format!("core.excludesFile={NULL_FILE}")]);
    command
}

async fn output(mut command: Command, args: &[&str]) -> Result<Vec<u8>, String> {
    output_with_input(&mut command, args, None).await
}

async fn output_with_input(
    command: &mut Command,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    command
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start Git: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("Git stdout unavailable")?;
    let stdin = child.stdin.take();
    let read = async {
        let send = async {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                stdin.write_all(input).await.map_err(|e| e.to_string())?;
            }
            Ok::<_, String>(())
        };
        let receive = async {
            let mut bytes = Vec::new();
            (&mut stdout)
                .take(1_048_577)
                .read_to_end(&mut bytes)
                .await
                .map_err(|e| e.to_string())?;
            if bytes.len() > 1_048_576 {
                return Err("Git output exceeded 1 MiB; scan is incomplete".into());
            }
            Ok(bytes)
        };
        let (_, bytes) = tokio::try_join!(send, receive)?;
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

#[cfg(test)]
pub(super) async fn fixture_git(path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    output(command(path), args).await
}

fn normalize(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("repository metadata path is not absolute".into());
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !result.pop() {
                    return Err("repository metadata path escapes its root".into());
                }
            }
            Component::CurDir => (),
            _ => result.push(component),
        }
    }
    Ok(result)
}

fn path_text(path: &Path) -> Result<String, String> {
    Ok(path
        .to_str()
        .ok_or("repository metadata path is not UTF-8")?
        .trim_start_matches("\\\\?\\")
        .to_owned())
}

/// Check lexical policy before any filesystem access, then inspect each component without following
/// links. Metadata may be shared by explicitly allowed working copies of the same repository.
fn check_path_policy(path: &Path, source: &WorkingCopy, config: &Config) -> Result<(), String> {
    let text = path_text(path)?;
    if agentbolt_core::holds::excluded(&text, config) {
        return Err("repository metadata overlaps an excluded root and will not be read".into());
    }
    let key = agentbolt_core::holds::path_key(&text)?;
    let allowed = config
        .working_copies
        .iter()
        .filter(|copy| copy.repo == source.repo)
        .any(|copy| {
            #[cfg(windows)]
            {
                agentbolt_core::holds::path_key(&copy.path)
                    .is_ok_and(|root| key == root || key.starts_with(&(root + "/")))
            }
            #[cfg(not(windows))]
            {
                let _ = &key;
                path.starts_with(&copy.path)
            }
        });
    if !allowed {
        return Err("repository metadata is outside the allowed working-copy folders; add its owning working copy or use a dedicated clone".into());
    }
    Ok(())
}

fn checked_path(path: &Path, source: &WorkingCopy, config: &Config) -> Result<PathBuf, String> {
    let path = normalize(path)?;
    check_path_policy(&path, source, config)?;
    // Git for Windows does not accept Rust's canonical \\?\ prefix for --file/config paths.
    let path = PathBuf::from(path_text(&path)?);
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        // A verbatim Windows drive prefix reports absolute even before its root separator;
        // querying that partial prefix fails with ERROR_INVALID_FUNCTION.
        if !matches!(component, Component::Normal(_)) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|e| format!("repository metadata path unavailable: {e}"))?;
        if is_link(&metadata) || !(metadata.is_dir() || metadata.is_file()) {
            return Err(
                "repository metadata contains a symbolic link, reparse point or special file"
                    .into(),
            );
        }
    }
    // Also recheck the OS-resolved spelling (for example a Windows short-name alias) before any
    // metadata contents or directory entries are read. This mirrors the working-root policy check.
    let canonical = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let canonical = PathBuf::from(path_text(&canonical)?);
    check_path_policy(&canonical, source, config)?;
    Ok(canonical)
}

fn small_file(path: &Path, limit: u64) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if is_link(&metadata) || !metadata.is_file() || metadata.len() > limit {
        return Err(
            "repository metadata is linked, not a regular file or exceeds its read limit".into(),
        );
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_string(&mut text)
        .map_err(|_| "repository metadata is unreadable or not UTF-8")?;
    if text.len() as u64 > limit || text.contains('\0') {
        return Err("repository metadata exceeds its read limit or contains NUL".into());
    }
    Ok(text)
}

fn destination(base: &Path, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim_end_matches(['\r', '\n']);
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return Err("invalid repository metadata destination".into());
    }
    normalize(&base.join(raw))
}

fn check_metadata_tree(root: &Path) -> Result<(), String> {
    let started = Instant::now();
    let mut pending = vec![root.to_path_buf()];
    let mut entries = 0;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            entries += 1;
            if entries > 100_000 || started.elapsed() > Duration::from_secs(2) {
                return Err("repository metadata boundary check exceeded its limit".into());
            }
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if is_link(&metadata) || !(metadata.is_dir() || metadata.is_file()) {
                return Err(
                    "repository metadata contains a symbolic link, reparse point or special file"
                        .into(),
                );
            }
            if metadata.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                // Windows opens reserved Git filenames case-insensitively too.
                #[cfg(windows)]
                let relative = relative.to_ascii_lowercase();
                if matches!(
                    relative.as_str(),
                    "objects/info/alternates" | "objects/info/http-alternates" | "info/grafts"
                ) || relative.ends_with(".promisor")
                {
                    return Err("repository uses alternate objects, grafts or partial-clone objects; scan is unknown".into());
                }
            }
        }
    }
    Ok(())
}

// An empty directory prevents Git's optional repository discovery from loading local config before
// `config --no-includes` takes effect. Explicit GIT_DIR also prevents walking up to a parent checkout.
struct ConfigReader(PathBuf);
impl ConfigReader {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "agentbolt-git-config-{}",
            agentbolt_core::tokens::random_hex(16)
        ));
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        builder
            .create(&path)
            .map_err(|e| format!("cannot isolate Git configuration reader: {e}"))?;
        Ok(Self(path))
    }
    async fn check(&self, file: &Path) -> Result<(), String> {
        // Bound input before Git parses it. No includes, type conversions or value interpolation.
        small_file(file, 262_144)?;
        let mut command = command(&self.0);
        command
            .env("GIT_DIR", &self.0)
            .arg("config")
            .arg("--file")
            .arg(file);
        let raw = output(command, &["--no-includes", "--null", "--list"]).await?;
        for entry in raw.split(|b| *b == 0).filter(|entry| !entry.is_empty()) {
            let key = entry.split(|b| *b == b'\n').next().unwrap_or_default();
            let key = std::str::from_utf8(key)
                .map_err(|_| "invalid Git config key")?
                .to_ascii_lowercase();
            if key.starts_with("include.") || key.starts_with("includeif.") {
                return Err("repository config includes other files; scan is unknown without reading those files".into());
            }
            if key.starts_with("filter.") {
                return Err("repository config defines a content filter; scan is unknown without running it".into());
            }
            if key == "core.worktree"
                || key == "core.alternaterefscommand"
                || key == "diff.orderfile"
                || key == "extensions.partialclone"
                || key.starts_with("remote.") && key.ends_with(".promisor")
            {
                return Err("repository config redirects reads or requires external processing; scan is unknown".into());
            }
        }
        Ok(())
    }
}
impl Drop for ConfigReader {
    fn drop(&mut self) {
        // Remove only this empty directory; never recurse into unexpected contents.
        let _ = std::fs::remove_dir(&self.0);
    }
}

pub(super) struct Repository {
    root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}
impl Repository {
    pub(super) async fn open(
        root: &Path,
        source: &WorkingCopy,
        config: &Config,
    ) -> Result<Self, String> {
        let git_file = checked_path(&root.join(".git"), source, config)
            .map_err(|e| format!("configured root has no safe Git metadata: {e}"))?;
        let git_dir = if std::fs::symlink_metadata(&git_file)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            git_file
        } else {
            let text = small_file(&git_file, 8192)?;
            let target = text
                .strip_prefix("gitdir: ")
                .ok_or("invalid .git metadata pointer")?;
            checked_path(&destination(root, target)?, source, config)?
        };
        let common_file = git_dir.join("commondir");
        let common_dir = match std::fs::symlink_metadata(&common_file) {
            Ok(_) => checked_path(
                &destination(&git_dir, &small_file(&common_file, 8192)?)?,
                source,
                config,
            )?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => git_dir.clone(),
            Err(e) => return Err(e.to_string()),
        };
        let dirs = (git_dir.clone(), common_dir.clone());
        tokio::task::spawn_blocking(move || {
            check_metadata_tree(&dirs.0)?;
            if dirs.0 != dirs.1 {
                check_metadata_tree(&dirs.1)?;
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|e| e.to_string())??;
        let reader = ConfigReader::new()?;
        reader.check(&common_dir.join("config")).await?;
        let worktree_config = git_dir.join("config.worktree");
        match std::fs::symlink_metadata(&worktree_config) {
            Ok(_) => reader.check(&worktree_config).await?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
        Ok(Self {
            root: PathBuf::from(path_text(root)?),
            git_dir,
            common_dir,
        })
    }

    fn command(&self) -> Command {
        let mut command = command(&self.root);
        command
            .env("GIT_DIR", &self.git_dir)
            .env("GIT_COMMON_DIR", &self.common_dir)
            .env("GIT_WORK_TREE", &self.root);
        command
    }

    pub(super) async fn read(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        output(self.command(), args).await
    }

    pub(super) async fn check_attributes(&self, index: &[u8]) -> Result<(), String> {
        // A filter name may depend on global config, which the scanner deliberately never reads.
        // Refuse such tracked files even when no local filter command is defined: passthrough could
        // otherwise change what Git considers clean. check-attr reads attributes, never filters.
        let mut input = Vec::new();
        for record in index
            .split(|b| *b == 0)
            .filter(|r| !r.is_empty() && !r.starts_with(b"160000 "))
        {
            let path = record
                .splitn(2, |b| *b == b'\t')
                .nth(1)
                .ok_or("incomplete Git index record")?;
            input.extend_from_slice(path);
            input.push(0);
        }
        if input.is_empty() {
            return Ok(());
        }
        let raw = output_with_input(
            &mut self.command(),
            &["check-attr", "-z", "--stdin", "filter"],
            Some(&input),
        )
        .await?;
        let raw = raw
            .strip_suffix(&[0])
            .ok_or("incomplete Git attribute record")?;
        let mut fields = raw.split(|b| *b == 0);
        while fields.next().is_some() {
            if fields.next() != Some(&b"filter"[..]) {
                return Err("incomplete Git attribute record".into());
            }
            match fields.next() {
                Some(b"unspecified" | b"unset") => (),
                Some(_) => {
                    return Err(
                        "tracked paths use content filters; scan is unknown without running them"
                            .into(),
                    );
                }
                None => return Err("incomplete Git attribute record".into()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[tokio::test]
    async fn isolated_git_ignores_injected_config_paths_and_trace_writes() {
        let dir = TempDir::new("holds-git-env");
        fixture_git(dir.path(), &["init"]).await.unwrap();
        let bad_config = dir.path().join("bad-config");
        std::fs::write(&bad_config, "[invalid config").unwrap();
        let trace = dir.path().join("unexpected-trace");
        let mut process = command(dir.path());
        process
            .env("GIT_DIR", dir.path().join("nonexistent"))
            .env("GIT_CONFIG_GLOBAL", &bad_config)
            .env("GIT_CONFIG_SYSTEM", &bad_config)
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.worktree")
            .env("GIT_CONFIG_VALUE_0", "forbidden")
            .env("GIT_TRACE", &trace);
        isolate_environment(&mut process);
        assert!(output(process, &["rev-parse", "--git-dir"]).await.is_ok());
        assert!(!trace.exists());
    }

    #[tokio::test]
    async fn config_reader_never_loads_its_parent_repository_config() {
        let dir = TempDir::new("holds-config-parent");
        fixture_git(dir.path(), &["init"]).await.unwrap();
        std::fs::write(dir.path().join(".git/config"), "[invalid config").unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        let config = dir.path().join("chosen-config");
        std::fs::write(&config, "[core]\nrepositoryformatversion = 0\n").unwrap();
        ConfigReader(empty).check(&config).await.unwrap();
    }
}
