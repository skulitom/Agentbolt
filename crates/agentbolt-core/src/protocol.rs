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
    /// SSH relay registration. Machine comes from the user's relay configuration, not a tool argument.
    RegisterRemote {
        machine: String,
        registration: RemoteRegistration,
    },
    Holds {
        handle: String,
        action: crate::holds::HoldOp,
    },
    /// Agent coordination only. No Board action can answer a human question.
    Board {
        handle: String,
        action: crate::board::BoardOp,
    },
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
    /// Propose a name, without binding it, sharing answers, or granting any owner privileges.
    RequestOwner {
        handle: String,
        requested_owner: String,
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
    AnswersPage {
        handle: String,
        before: Option<String>,
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
    RelayPage {
        handle: String,
        #[serde(default)]
        owners: Vec<String>,
        before: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteRegistration {
    pub owner: Option<String>,
    pub client: String,
    pub session_ref: Option<String>,
    pub cwd: Option<String>,
    pub doing: Option<String>,
    pub provisional_key: Option<String>,
}

/// Results of MCP operations (the `result` field).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResult {
    pub handle: String,
    pub owner: String,
    pub provisional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_owner: Option<String>,
    /// Answered asks without confirmed delivery to this owner or its asking session.
    pub answers: Vec<Ask>,
    #[serde(default)]
    pub answers_next: Option<String>,
    #[serde(default)]
    pub more_answers: bool,
    /// This owner's open asks.
    pub open_asks: Vec<Ask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskResult {
    pub id: String,
    /// "open" (in the Inbox), "existing" (deduplicated) or "cap" (the owner already has its limit;
    /// `existing` is its oldest). Older daemons may return the legacy "digest" value.
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
    /// asks with no confirmed delivery to the asker (answered in the last 14 days, at most 50).
    pub answers: Vec<Ask>,
    /// Ids of this owner's open asks (the requested ones, or all of them with no `ids`).
    #[serde(default)]
    pub waiting: Vec<String>,
    /// Requested ids of this owner's asks that were cancelled.
    #[serde(default)]
    pub cancelled: Vec<String>,
    /// Requested ids that are unknown or belong to another owner.
    #[serde(default)]
    pub unknown: Vec<String>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub more: bool,
}

/// Result of `McpOp::RelayList`: answered asks of other owners, answered in the last 14 days, with no confirmed
/// delivery recorded yet (by `RelayMark`, to a session or to NOTES.md); newest
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
    /// Legacy wire compatibility; 0.2 admits every ask directly to the Inbox.
    pub digest: u32,
    pub agents: u32,
    pub chain_ok: bool,
    /// OS notification registration health, supplied by the daemon (not the store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_warning: Option<String>,
}

/// Operations on the Desk pipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum DeskOp {
    History {
        query: HistoryQuery,
    },
    Phone,
    SetPhone {
        config: crate::phone::Update,
    },
    /// Consume a durable phone receipt. The daemon only launches the installed Desk with its id.
    PhoneReply {
        id: String,
    },
    Holds,
    HoldsPage {
        after: Option<String>,
    },
    SetHolds {
        config: crate::holds::Config,
    },
    Metrics,
    StartBaseline {
        label: String,
    },
    RecordVerdict {
        text: String,
    },
    Board,
    BoardPage {
        query: crate::board::DeskQuery,
    },
    SetLease {
        owner: String,
        seconds: u32,
    },
    /// Inbox and recent answers. Listing the Inbox marks those asks shown, so the Desk sends it only
    /// while its window is focused and not minimized (`Status` refreshes the counts otherwise).
    List,
    InboxPage {
        after: Option<InboxCursor>,
    },
    /// Legacy wire compatibility. Migrated asks are in the Inbox; no new digest asks are created.
    OpenDigest,
    Answer {
        ask_id: String,
        choice: Option<String>,
        values: Option<serde_json::Value>,
        note: Option<String>,
    },
    /// All answers are validated and committed together; a stale or invalid item writes nothing.
    AnswerBatch {
        answers: Vec<DeskAnswer>,
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
    MuteFor {
        owner: String,
        seconds: u32,
    },
    Agents,
    Owners,
    SetOwner {
        owner: Owner,
    },
    /// Resolve exactly the proposal displayed by the Desk; stale proposals are refused.
    ResolveOwner {
        owner: String,
        requested_owner: String,
        approve: bool,
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
    /// Legacy wire compatibility; zero after migration to the direct Inbox.
    pub digest_count: u32,
    pub recent: Vec<Ask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxCursor {
    pub muted: bool,
    pub priority: String,
    pub deadline_ms: i64,
    pub created_ms: i64,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxResult {
    pub inbox: Vec<Ask>,
    pub recent: Vec<Ask>,
    pub total: u32,
    pub next: Option<InboxCursor>,
    pub more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_warning: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub before: Option<String>,
    pub search: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryResult {
    pub asks: Vec<Ask>,
    pub total: u32,
    pub next: Option<String>,
    pub more: bool,
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

pub const MAX_BATCH_ANSWERS: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeskAnswer {
    pub ask_id: String,
    pub choice: Option<String>,
    pub values: Option<serde_json::Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAnswerResult {
    pub answers: Vec<Answer>,
}
