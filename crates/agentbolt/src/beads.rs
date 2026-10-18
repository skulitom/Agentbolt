//! Link an existing Beads issue to a handoff. Beads remains its source of truth: no claim/close/update.
use crate::daemon::Daemon;
use agentbolt_core::{
    board::{BoardOp, RequestInput},
    channel::Names,
    protocol::{McpOp, RegisterResult},
};
use agentbolt_ipc::Pipe;
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    pub channel: Option<String>,
    pub owner: String,
    pub repo: String,
    pub issue: String,
    pub cwd: Option<PathBuf>,
    pub json_file: Option<PathBuf>,
    pub to_role: Option<String>,
    pub paths: Vec<String>,
}
impl Args {
    pub fn validate(&self) -> Result<(), String> {
        agentbolt_core::rules::check_owner_name(&self.owner)?;
        agentbolt_core::board::repository(&self.repo)?;
        if self.issue.is_empty()
            || self.issue.len() > 194
            || self.issue.starts_with('-')
            || !self
                .issue
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(
                "Beads issue ID needs 1–194 letters, digits, dots, underscores or hyphens".into(),
            );
        }
        agentbolt_core::board::paths(self.paths.clone())?;
        Ok(())
    }
}

async fn issue_json(args: &Args) -> anyhow::Result<Value> {
    let bytes = if let Some(path) = &args.json_file {
        let file = std::fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take(1_048_577).read_to_end(&mut bytes)?;
        bytes
    } else {
        let cwd = args
            .cwd
            .clone()
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)?;
        let cwd = std::fs::canonicalize(cwd)?;
        anyhow::ensure!(cwd.is_dir(), "Beads working directory is not a folder");
        let mut command = tokio::process::Command::new("bd");
        command
            .current_dir(cwd)
            .args(["show", "--json", "--id", &args.issue])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Cannot start bd show: {e}; use --json-file with an existing JSON export instead"
            )
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("bd stdout unavailable"))?;
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut bytes = Vec::new();
            stdout.take(1_048_577).read_to_end(&mut bytes).await?;
            anyhow::ensure!(bytes.len() <= 1_048_576, "Beads JSON exceeds 1 MiB");
            let status = child.wait().await?;
            anyhow::ensure!(
                status.success(),
                "bd show failed; inspect the issue in its own working directory"
            );
            Ok::<_, anyhow::Error>(bytes)
        })
        .await
        .map_err(|_| anyhow::anyhow!("bd show timed out after 30 seconds"))??
    };
    anyhow::ensure!(bytes.len() <= 1_048_576, "Beads JSON exceeds 1 MiB");
    Ok(serde_json::from_slice(&bytes)?)
}

fn request(args: &Args, value: Value) -> anyhow::Result<RequestInput> {
    let items = match value {
        Value::Array(items) => items,
        one => vec![one],
    };
    let mut matching = items.into_iter().filter_map(|v| {
        let v = if v.get("id").is_some() {
            v
        } else {
            v.get("issue")?.clone()
        };
        (v.get("id").and_then(Value::as_str) == Some(args.issue.as_str())).then_some(v)
    });
    let issue=matching.next().ok_or_else(||anyhow::anyhow!("Beads output has no exact issue ID {}; fuzzy or cross-repo substitutions are not imported",args.issue))?;
    anyhow::ensure!(
        matching.next().is_none(),
        "Beads output contains the issue more than once"
    );
    let title = issue["title"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Beads issue has no title"))?;
    let status = issue["status"].as_str().unwrap_or("unknown");
    anyhow::ensure!(
        !matches!(status, "closed" | "tombstone"),
        "The Beads issue is closed; no new handoff was created"
    );
    let mut body = format!(
        "Linked Beads issue {} (status at import: {status}). Beads remains the source of truth; this handoff does not claim or close that issue.\n\n{title}",
        args.issue
    );
    for (key, label) in [
        ("description", "Description"),
        ("acceptance_criteria", "Acceptance criteria"),
        ("design", "Design"),
        ("notes", "Notes"),
    ] {
        if let Some(text) = issue[key].as_str().filter(|s| !s.trim().is_empty()) {
            body.push_str(&format!("\n\n{label}\n{text}"));
        }
    }
    anyhow::ensure!(
        body.len() <= 16_384,
        "Issue text exceeds the Board's 16 KiB limit; prepare a shorter issue export or handoff explicitly"
    );
    let repo = agentbolt_core::board::repository(&args.repo).map_err(anyhow::Error::msg)?;
    Ok(RequestInput {
        title: agentbolt_core::rules::clean_line(title, 200),
        body,
        to_role: args.to_role.clone(),
        repo: Some(repo.clone()),
        paths: args.paths.clone(),
        // Board dedupe already includes repository scope; keep the key within its 200-character cap.
        dedupe_key: Some(format!("beads:{}", args.issue)),
        beads_issue: Some(args.issue.clone()),
    })
}

pub async fn import(names: Names, args: Args) -> anyhow::Result<Value> {
    args.validate().map_err(anyhow::Error::msg)?;
    let input = request(&args, issue_json(&args).await?)?;
    let daemon = Daemon::new(names, Pipe::Mcp, true);
    let registration = daemon
        .call(&McpOp::Register {
            owner: Some(args.owner),
            client: "agentbolt-beads".into(),
            session_ref: None,
            cwd: args.cwd.map(|p| p.display().to_string()),
            doing: Some(format!("Beads handoff {}", args.issue)),
            provisional_key: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let registration: RegisterResult = serde_json::from_value(registration)?;
    let mut result = daemon
        .call(&McpOp::Board {
            handle: registration.handle,
            action: BoardOp::Request { input },
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    result["beads"] = json!({"issue":args.issue,"changed":false,"instruction":"This is a linked handoff. Claim/close the Beads issue only through your authorized Beads workflow."});
    Ok(result)
}
