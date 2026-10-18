//! A separate, per-process lifecycle endpoint. These messages cannot read or write answers.
//! The installer verifies the Desk image before preparing it, then retains that connection across
//! file replacement (when the old process's image is renamed).

use agentbolt_core::channel::Names;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "op",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Op {
    Inspect,
    Prepare {
        transaction: String,
    },
    Cancel {
        transaction: String,
    },
    Close {
        transaction: String,
    },
    /// A prepared standalone Desk launches its replacement in its own Windows session, then exits.
    Reopen {
        transaction: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Prepared {
    pub lifecycle_version: u32,
    pub pid: u32,
    pub session: u32,
    pub visible: bool,
    pub warm: bool,
}

pub fn endpoint(names: &Names, pid: u32) -> Names {
    let mut control = names.clone();
    control.mcp_pipe = format!("{}-desk-control-{pid}", names.mcp_pipe);
    control
}

pub fn valid_transaction(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A short-lived, channel-local request to show a newly preloaded Desk in its original session.
/// This only changes visibility. It cannot carry an answer, path or executable to run.
pub fn request_reopen(names: &Names, session: u32) -> std::io::Result<()> {
    let dir = names.data_dir().map_err(std::io::Error::other)?;
    std::fs::create_dir_all(&dir)?;
    let until = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_secs()
        + 300;
    std::fs::write(
        dir.join(format!("desk-reopen-{session}.json")),
        until.to_string(),
    )
}

pub fn take_reopen(names: &Names, session: u32) -> bool {
    let Ok(dir) = names.data_dir() else {
        return false;
    };
    let path = dir.join(format!("desk-reopen-{session}.json"));
    let result = (|| {
        use std::io::Read;
        let file = std::fs::File::open(&path).ok()?;
        let mut value = String::new();
        file.take(32).read_to_string(&mut value).ok()?;
        let until: u64 = value.parse().ok()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        Some(until >= now && until <= now + 300)
    })()
    .unwrap_or(false);
    let _ = std::fs::remove_file(path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_is_separate_and_cannot_deserialize_answer_operations() {
        let names = Names::for_channel("control-test").unwrap();
        assert_ne!(endpoint(&names, 1).mcp_pipe, names.mcp_pipe);
        assert_ne!(endpoint(&names, 1).mcp_pipe, endpoint(&names, 2).mcp_pipe);
        for op in [
            "answer",
            "answer_batch",
            "answer_token",
            "not_needed",
            "list",
        ] {
            assert!(
                serde_json::from_value::<Op>(serde_json::json!({"op": op, "args": {}})).is_err()
            );
        }
        assert!(valid_transaction(&"a".repeat(32)));
        assert!(!valid_transaction("../elsewhere"));
    }
}
