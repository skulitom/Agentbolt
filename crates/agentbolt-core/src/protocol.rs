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
    /// Say who you are. `owner` comes from `agentbolt mcp --owner`; None gets a provisional per-session owner.
    Register {
        owner: Option<String>,
        client: String,
        session_ref: Option<String>,
        cwd: Option<String>,
        doing: Option<String>,
    },
    /// Keep the handle alive and update what you're doing.
    Heartbeat { handle: String, doing: Option<String> },
    Ask { handle: String, ask: AskInput },
    /// Answers for this handle's owner. `ids` empty = all of this owner's asks with answers not yet delivered.
    /// `wait_ms` is capped by the daemon (default cap 20 000; the MCP front-end may pass a larger host-specific cap).
    Answers { handle: String, ids: Vec<String>, wait_ms: u64 },
    Cancel { handle: String, id: String },
    /// Relay owner only: answers waiting for any owner.
    RelayList { handle: String },
    /// Relay owner only: record a delivery.
    RelayMark { handle: String, ask_id: String, to: String, ok: bool, detail: Option<String> },
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
    /// "open" (in the Inbox), "digest" (held by the budget) or "existing" (deduplicated).
    pub placed: String,
    /// When `placed` is "existing", or when the owner is at its cap: the ask it was matched to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<Ask>,
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
    /// Inbox (open), digest, recent answered. Listing the Inbox marks those asks shown.
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
    AnswerToken { token: String },
    Agents,
    Owners,
    SetOwner { owner: Owner },
    GetSettings,
    SetSettings { settings: Settings },
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnersResult {
    pub owners: Vec<Owner>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerResult {
    pub answer: Answer,
}
