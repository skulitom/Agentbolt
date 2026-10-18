//! The data model shared by the daemon, the MCP front-end and the Desk (PLAN.md, "The data model").
//! Every type here crosses the pipe as JSON, so field names are part of the contract.

use serde::{Deserialize, Serialize};

/// What kind of answer an ask wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    /// Information only; the only answer is an acknowledgement.
    Inform,
    /// Pick one of `options`.
    Choose,
    /// Approve or decline (options default to "Approve" / "Decline").
    Approve,
    /// A free-text reply.
    Text,
    /// A small form of up to eight `fields`.
    Form,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskStatus {
    /// In the Inbox, waiting for Artem.
    Open,
    /// Held back by the daily budget; shown in the daily digest.
    Digest,
    Answered,
    /// The deadline passed after Artem had been shown it, and it was reversible.
    Defaulted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub label: String,
    /// Only `https:`, `claude://` and repository paths pass the allowlist (`rules::check_link`).
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub label: String,
    /// "text" | "number" | "boolean" | "choice"
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

/// What an agent submits with `desk_ask`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskInput {
    pub kind: AskKind,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    /// The recommended default: an option's text, or a JSON object of field values for forms.
    pub default: serde_json::Value,
    /// RFC 3339.
    pub deadline: String,
    pub reversible: bool,
    pub priority: Priority,
    /// Required for P0: why it can't wait for the budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p0_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ask {
    pub id: String,
    pub owner: String,
    pub asking_session: Option<String>,
    #[serde(flatten)]
    pub input: AskInput,
    pub status: AskStatus,
    pub created_at: String,
    /// Set the first time the Desk shows it to Artem (Inbox or an opened digest). No default before this.
    pub shown_at: Option<String>,
    pub answer: Option<Answer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerVia {
    /// Clicked in the Desk window.
    Desk,
    /// A toast button (same-user trust only; reversible asks only).
    Toast,
    /// Applied at the deadline. Carries no Artem authority.
    Defaulted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub ask_id: String,
    pub via: AnswerVia,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub answered_at: String,
    /// Delivery records (relay, codex queue).
    #[serde(default)]
    pub deliveries: Vec<Delivery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    pub to: String,
    pub at: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A live client of the MCP pipe (one per `desk_register`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub handle: String,
    pub owner: String,
    /// True when the owner was not bound by config or approved on the Desk.
    pub provisional: bool,
    pub client: String,
    pub session_ref: Option<String>,
    pub cwd: Option<String>,
    pub machine: String,
    pub doing: Option<String>,
    pub registered_at: String,
    pub last_seen: String,
}

/// An owner name and how it was established.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub name: String,
    /// "config" (from `agentbolt mcp --owner`), "approved" (on the Desk) or "pending".
    pub binding: String,
    /// Owners whose Codex threads may receive `codex queue` delivery (marked on the Desk). Off by default.
    #[serde(default)]
    pub codex_interactive: bool,
    /// The relay owner may call `desk_relay` (configured on the Desk; Compass: "compass-reception").
    #[serde(default)]
    pub relay: bool,
}

/// Desk settings, stored by the daemon.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Full ntfy topic URL for the count-only push, e.g. `https://ntfy.sh/<secret-topic>`. None = off.
    pub ntfy_topic_url: Option<String>,
    /// Inbox budget per day (default 5).
    pub daily_budget: u32,
    /// Silence toasts (they are always silent; this turns them off entirely).
    pub toasts_off: bool,
}
