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
    Prepare { transaction: String },
    Cancel { transaction: String },
    Close { transaction: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Prepared {
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
