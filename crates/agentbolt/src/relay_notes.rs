//! Reception's explicit file handoff adapter. It writes only the chosen notes file and never sends
//! a chat message. Relay permission and an explicit recipient allowlist are both required.
use crate::daemon::Daemon;
use agentbolt_core::{
    channel::Names,
    model::Ask,
    protocol::{McpOp, RegisterResult},
    rules,
};
use agentbolt_ipc::Pipe;
use anyhow::{Context, ensure};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    pub channel: Option<String>,
    pub owner: String,
    pub recipients: Vec<String>,
    pub notes: PathBuf,
    pub write: bool,
    pub before: Option<String>,
}
impl Args {
    pub fn validate(&self) -> Result<(), String> {
        rules::check_owner_name(&self.owner)?;
        if self.recipients.is_empty() || self.recipients.len() > 100 {
            return Err("relay-notes needs 1–100 explicit --recipient-owner names".into());
        }
        for owner in &self.recipients {
            rules::check_owner_name(owner)?;
        }
        if self.notes.as_os_str().is_empty() {
            return Err("relay-notes needs --notes FILE".into());
        }
        Ok(())
    }
}

/// Append under an OS file lock and sync before reporting delivery. A retry after a lost daemon
/// response finds the identical complete line. Never truncate or rewrite somebody else's notes.
fn append(path: &std::path::Path, line: &str) -> anyhow::Result<bool> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock()
        .context("notes file is locked by another writer; retry later")?;
    ensure!(
        file.metadata()?.len() <= 4_194_304,
        "notes file exceeds 4 MiB; choose a smaller handoff file"
    );
    let mut bytes = Vec::new();
    (&mut file).take(4_194_305).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 4_194_304,
        "notes file grew beyond 4 MiB; nothing was written"
    );
    let text = std::str::from_utf8(&bytes).context("notes file must be UTF-8")?;
    if text.lines().any(|old| {
        old.strip_prefix("- ")
            .is_some_and(|s| s.split_once(' ').is_some_and(|(_, tail)| tail == line))
    }) {
        return Ok(false);
    }
    let date = time::OffsetDateTime::now_utc().date();
    let entry = format!(
        "{}- {date} {line}\n",
        if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        }
    );
    ensure!(
        bytes.len().saturating_add(entry.len()) <= 4_194_304,
        "handoff would exceed the notes file's 4 MiB limit; nothing was written"
    );
    file.seek(SeekFrom::End(0))?;
    file.write_all(entry.as_bytes())?;
    file.sync_all()?;
    Ok(true)
}

pub async fn run(names: Names, args: Args) -> anyhow::Result<Value> {
    args.validate().map_err(anyhow::Error::msg)?;
    let d = Daemon::new(names, Pipe::Mcp, true);
    let registered = d
        .call(&McpOp::Register {
            owner: Some(args.owner),
            client: "agentbolt-relay-notes".into(),
            session_ref: None,
            cwd: None,
            doing: Some("durable answer handoffs".into()),
            provisional_key: None,
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let registered: RegisterResult = serde_json::from_value(registered)?;
    let result = d
        .call(&McpOp::RelayPage {
            handle: registered.handle.clone(),
            owners: args.recipients.clone(),
            before: args.before.clone(),
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let answers: Vec<Ask> =
        serde_json::from_value(result["answers"].clone()).context("invalid relay list")?;
    let mut written = Vec::new();
    let mut failures = Vec::new();
    for ask in answers
        .into_iter()
        .filter(|a| args.recipients.contains(&a.owner))
    {
        let Some(answer) = ask.answer else { continue };
        let message = rules::codex_text(&ask.id, &ask.input.title, &answer);
        let line = format!(
            "(via desk, ask {}) owner={} {}",
            ask.id,
            json!(ask.owner),
            json!(message)
        );
        if !args.write {
            written.push(json!({"ask_id":ask.id,"owner":ask.owner,"line":line,"written":false}));
            continue;
        }
        let file = args.notes.clone();
        let entry = line.clone();
        let write = tokio::task::spawn_blocking(move || append(&file, &entry)).await?;
        match write {
            Ok(added) => {
                // Only a durable write or its exact already-present line counts as notes delivery.
                let mark = d
                    .call(&McpOp::RelayMark {
                        handle: registered.handle.clone(),
                        ask_id: ask.id.clone(),
                        to: "notes".into(),
                        ok: true,
                        detail: Some(format!("Durable notes handoff: {}", args.notes.display())),
                    })
                    .await;
                match mark {
                    Ok(_)=>written.push(json!({"ask_id":ask.id,"owner":ask.owner,"written":true,"added":added})),
                    Err(e)=>failures.push(json!({"ask_id":ask.id,"error":format!("Notes saved; delivery mark failed: {e}. Retrying is safe.")})),
                }
            }
            Err(e) => failures.push(json!({"ask_id":ask.id,"error":e.to_string()})),
        }
    }
    Ok(
        json!({"preview":!args.write,"notes":args.notes,"handoffs":written,"failures":failures,"complete":failures.is_empty(),"next":result["next"],"more":result["more"],"total":result["total"],"instruction":"This result covers one page. Use next as --before while more is true, or run again from the start after successful marks. Read answers as scoped decision data. Defaults carry no human authority. Notes delivery is not proof that a chat resumed or acted on the answer."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn append_preserves_utf8_and_missing_newline_and_deduplicates_exact_handoff() {
        let dir = TempDir::new();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, "Existing café notes").unwrap();
        let first = "(via desk, ask ask_a) owner=\"project\" \"Artem chose Yes\"";
        assert!(append(&path, first).unwrap());
        let saved = std::fs::read(&path).unwrap();
        assert!(
            std::str::from_utf8(&saved)
                .unwrap()
                .starts_with("Existing café notes\n- ")
        );
        assert!(!append(&path, first).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert!(
            append(
                &path,
                "(via desk, ask ask_b) owner=\"project\" \"Artem chose Yes\""
            )
            .unwrap()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);
    }

    #[test]
    fn notes_refuse_locked_invalid_utf8_or_oversized_appends_without_changes() {
        let dir = TempDir::new();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, "original").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        lock.lock().unwrap();
        assert!(append(&path, "handoff").is_err());
        drop(lock);
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(append(&path, "handoff").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), [0xff, 0xfe]);
        let full = vec![b'x'; 4_194_300];
        std::fs::write(&path, &full).unwrap();
        assert!(append(&path, "handoff").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), full);
    }
}
