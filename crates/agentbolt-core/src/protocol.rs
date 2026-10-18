//! The pipe protocol between the daemon and its clients: one JSON object per line, request then response.
//!
//! There are two pipes per channel (`channel::Names`):
//! - the **MCP pipe**, for `agentbolt mcp` processes. It never offers an answer operation.
//! - the **Desk pipe**, for the installed `agentbolt-desk.exe` only (the daemon checks the client's image path).
//!   It is the only way to answer.

use serde::{Deserialize, Serialize};

use crate::model::{Agent, Answer, Ask, AskInput, Owner, Settings};

/// A request line: `{"id": 7, "op": "ask", "args": {...}}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request<T> {
    pub id: u64,
    #[serde(flatten)]
    pub op: T,
}

/// A response line: `{"id": 7, "ok": true, "result": {...}}` or `{"id": 7, "ok": false, "error": "..."}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Operations on the MCP pipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum McpOp {
    /// Say who you are. `owner` comes from `agentbolt mcp --owner`; None gets a provisional owner: per host
    /// session (`session_ref`, which the front-end takes only from the host, never from a tool argument), else
    /// per MCP process (`provisional_key`, made once when the process starts), else a fresh one.
    Register {
        owner: Option<String>,
        client: String,
        session_ref: Option<String>,
        cwd: Option<String>,
        doing: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provisional_key: Option<String>,
    },
    /// Keep the handle alive and update what you're doing.
    Heartbeat {
        handle: String,
        doing: Option<String>,
    },
    Ask {
        handle: String,
        ask: AskInput,
    },
    /// Answers for this handle's owner. `ids` empty = all of this owner's asks with answers not yet delivered.
    /// `wait_ms` is capped by the daemon (default cap 20 000; the MCP front-end may pass a larger host-specific cap).
    Answers {
        handle: String,
        ids: Vec<String>,
        wait_ms: u64,
    },
    Cancel {
        handle: String,
        id: String,
    },
    /// Relay owner only: answers waiting for any owner.
    RelayList {
        handle: String,
    },
    /// Relay owner only: record a delivery.
    RelayMark {
        handle: String,
        ask_id: String,
        to: String,
        ok: bool,
        detail: Option<String>,
    },
    Status,
}

/// Results of MCP operations (the `result` field).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResult {
    pub handle: String,
    pub owner: String,
    pub provisional: bool,
    /// Answered asks of this owner not yet delivered.
    pub answers: Vec<Ask>,
    /// This owner's asks still open or in the digest.
    pub open_asks: Vec<Ask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskResult {
    pub id: String,
    /// "open" (in the Inbox), "digest" (held by the budget), "existing" (deduplicated) or "cap" (the owner
    /// already has its limit of open asks; `existing` is its oldest).
    pub placed: String,
    /// When `placed` is "existing", or when the owner is at its cap: the ask it was matched to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<Ask>,
    /// Anything the daemon changed about the ask, such as a P0 stored as P1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Result of `McpOp::Answers`. Reading never consumes, and silence is never an answer: an ask without an
/// answer is listed in `waiting`, never in `answers`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnswersResult {
    /// Asks with an answer (`status` answered or defaulted, `answer` set). With no `ids`: this owner's answered
    /// asks with no successful delivery yet (answered in the last 14 days, at most 50).
    pub answers: Vec<Ask>,
    /// Ids of this owner's asks still open or in the digest (the requested ones, or all of them with no `ids`).
    #[serde(default)]
    pub waiting: Vec<String>,
    /// Requested ids of this owner's asks that were cancelled.
    #[serde(default)]
    pub cancelled: Vec<String>,
    /// Requested ids that are unknown or belong to another owner.
    #[serde(default)]
    pub unknown: Vec<String>,
}

/// Result of `McpOp::RelayList`: answered asks of other owners, answered in the last 14 days, with no successful
/// delivery recorded yet (by `RelayMark`, to a session or to NOTES.md, or by the daemon's `codex queue`); newest
/// first, at most 100. `RelayMark` returns an `AnswerResult`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelayListResult {
    pub answers: Vec<Ask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResult {
    pub version: String,
    pub channel: String,
    pub open: u32,
    pub digest: u32,
    pub agents: u32,
    pub chain_ok: bool,
}

/// Operations on the Desk pipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum DeskOp {
    /// Inbox (open), digest, recent answered. Listing the Inbox marks those asks shown, so the Desk sends it only
    /// while its window is focused and not minimized (`Status` refreshes the counts otherwise).
    List,
    /// Opening the digest marks its asks shown.
    OpenDigest,
    Answer {
        ask_id: String,
        choice: Option<String>,
        values: Option<serde_json::Value>,
        note: Option<String>,
    },
    /// From the `agentbolt://` handler: a per-button token minted for a toast.
    AnswerToken {
        token: String,
    },
    /// Dismiss an unnecessary question, without choosing its recommended option.
    NotNeeded {
        ask_id: String,
        note: Option<String>,
    },
    SetMuted {
        owner: String,
        muted: bool,
    },
    Agents,
    Owners,
    SetOwner {
        owner: Owner,
    },
    GetSettings,
    SetSettings {
        settings: Settings,
    },
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResult {
    pub inbox: Vec<Ask>,
    pub digest_count: u32,
    pub recent: Vec<Ask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestResult {
    pub digest: Vec<Ask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsResult {
    pub agents: Vec<Agent>,
}

/// Every bound or approved owner (by name), then the most recent provisional owners (newest first, at most
/// `rules::DESK_PROVISIONAL_OWNERS`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnersResult {
    pub owners: Vec<Owner>,
    /// Older provisional owners left out of `owners`.
    #[serde(default)]
    pub more_provisional: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerResult {
    pub answer: Answer,
}
