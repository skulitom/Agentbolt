//! Compute a PR's complete changed-path set from immutable Git commits, then check the Desk's holds.
//! Read-only: no fetch, checkout, PR creation or merge. The caller must use the returned exact head.
use crate::daemon::Daemon;
use agentbolt_core::{
    board,
    channel::Names,
    holds::HoldOp,
    protocol::{McpOp, RegisterResult},
    rules,
};
use agentbolt_ipc::Pipe;
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    pub channel: Option<String>,
    pub owner: String,
    pub repo: String,
    pub cwd: PathBuf,
    pub base: String,
    pub head: String,
    pub stage: String,
}
impl Args {
    pub fn validate(&self) -> Result<(), String> {
        rules::check_owner_name(&self.owner)?;
        board::repository(&self.repo)?;
        if self.cwd.as_os_str().is_empty() || self.base.is_empty() || self.head.is_empty() {
            return Err("pr-check needs --cwd, --base and --head".into());
        }
        for revision in [&self.base, &self.head] {
            if revision.len() > 1024
                || revision.starts_with('-')
                || revision.chars().any(char::is_control)
            {
                return Err("invalid base/head revision".into());
            }
        }
        if !matches!(self.stage.as_str(), "pr_open" | "merge") {
            return Err("--stage must be pr_open or merge".into());
        }
        Ok(())
    }
}

async fn git(cwd: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.current_dir(cwd)
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
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().context("cannot start Git")?;
    let stdout = child.stdout.take().context("Git stdout unavailable")?;
    tokio::time::timeout(Duration::from_secs(30),async {
        let mut bytes=Vec::new(); stdout.take(2_097_153).read_to_end(&mut bytes).await?;
        ensure!(bytes.len()<=2_097_152,"Git path output exceeds 2 MiB; the check is incomplete");
        ensure!(child.wait().await?.success(),"Git could not resolve the requested repository/commits; refresh your scratch clone and try again");
        Ok(bytes)
    }).await.context("Git check timed out")?
}
fn text(bytes: Vec<u8>) -> anyhow::Result<String> {
    Ok(String::from_utf8(bytes)?.trim().to_owned())
}
async fn resolve(cwd: &Path, revision: &str) -> anyhow::Result<String> {
    let sha = text(
        git(
            cwd,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{revision}^{{commit}}"),
            ],
        )
        .await?,
    )?;
    ensure!(
        matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "Git returned an invalid commit id"
    );
    Ok(sha)
}
fn repository(remote: &str) -> anyhow::Result<String> {
    let remote = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))
        .context("origin must be a credential-free github.com repository URL")?;
    board::repository(remote.trim_end_matches('/').trim_end_matches(".git"))
        .map_err(anyhow::Error::msg)
}

pub async fn run(names: Names, args: Args) -> anyhow::Result<Value> {
    args.validate().map_err(anyhow::Error::msg)?;
    let cwd = std::fs::canonicalize(&args.cwd)?;
    let root = text(git(&cwd, &["rev-parse", "--show-toplevel"]).await?)?;
    ensure!(
        std::fs::canonicalize(root)? == cwd,
        "--cwd must be the root of your scratch clone"
    );
    let origin = text(git(&cwd, &["remote", "get-url", "origin"]).await?)?;
    let repo = board::repository(&args.repo).map_err(anyhow::Error::msg)?;
    ensure!(
        repository(&origin)? == repo,
        "--repo does not match the scratch clone's origin"
    );
    let base = resolve(&cwd, &args.base).await?;
    let head = resolve(&cwd, &args.head).await?;
    let merge_base = text(git(&cwd, &["merge-base", &base, &head]).await?)?;
    ensure!(
        matches!(merge_base.len(), 40 | 64) && merge_base.bytes().all(|b| b.is_ascii_hexdigit()),
        "Git returned an invalid merge base"
    );
    let bytes = git(
        &cwd,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            "--no-renames",
            "-z",
            &merge_base,
            &head,
            "--",
        ],
    )
    .await?;
    ensure!(
        bytes.is_empty() || bytes.last() == Some(&0),
        "Git returned incomplete changed paths"
    );
    let mut paths = BTreeSet::new();
    for path in bytes.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        paths.insert(
            std::str::from_utf8(path)
                .context("changed path is not UTF-8; clearance cannot be established")?
                .to_owned(),
        );
    }
    ensure!(!paths.is_empty(), "the selected commits have no PR changes");
    let path_count = paths.len();
    // No subset can silently stand in for a large PR. A conservative whole-repository check keeps
    // the wire bounded and cannot miss a collision, including unusual filenames/glob characters.
    let whole_repo = paths.len() > 100
        || paths
            .iter()
            .any(|p| p.contains(['*', '?', '\\']) || board::paths(vec![p.clone()]).is_err());
    let paths = if whole_repo {
        vec!["**".into()]
    } else {
        paths.into_iter().collect()
    };
    let d = Daemon::new(names, Pipe::Mcp, true);
    let reg = d
        .call(&McpOp::Register {
            owner: Some(args.owner),
            client: "agentbolt-pr-check".into(),
            session_ref: None,
            cwd: Some(cwd.display().to_string()),
            doing: Some(format!("hold check before {}", args.stage)),
            provisional_key: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let reg: RegisterResult = serde_json::from_value(reg)?;
    let mut result = d
        .call(&McpOp::Holds {
            handle: reg.handle,
            action: HoldOp::Check {
                repo,
                paths,
                stage: Some(args.stage.clone()),
            },
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let stable =
        resolve(&cwd, &args.base).await? == base && resolve(&cwd, &args.head).await? == head;
    if !stable {
        result["clear"] = json!(false);
    }
    result["git"] = json!({"base_commit":base,"head_commit":head,"merge_base":merge_base,"refs_unchanged":stable,"changed_paths":path_count,"checked_whole_repository":whole_repo,"stage":args.stage});
    result["instruction"] = json!(
        "Use only this exact head for the dependent PR action. Recheck after any change to the PR, base or working-copy holds; immediately before merge use the fetched current PR head and --match-head-commit. A clear hold check is advisory, not review or merge approval."
    );
    Ok(result)
}
