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
    /// Legacy 0.1 value, migrated into the Inbox when the store opens.
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
    /// Required for P0: why it is urgent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p0_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ask {
    pub id: String,
    pub owner: String,
    /// Current owner preference; muted asks remain visible at the bottom of the Inbox.
    #[serde(default)]
    pub muted: bool,
    pub asking_session: Option<String>,
    /// Immutable host context, kept after the live agent's handle expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asking_client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asking_cwd: Option<String>,
    /// Origin at registration; `remote:` marks an SSH-connected machine. Never launch its cwd locally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asking_machine: Option<String>,
    #[serde(flatten)]
    pub input: AskInput,
    pub status: AskStatus,
    pub created_at: String,
    /// Set the first time the Desk shows it to Artem. No default before this.
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
    /// A protected phone reply, committed by the installed Desk (same-user trust).
    Phone,
    /// Applied at the deadline. Carries no Artem authority.
    Defaulted,
    /// The Desk dismissed the question as unnecessary. No option was approved.
    NotNeeded,
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
    #[serde(default)]
    pub delivery_total: u32,
    #[serde(default)]
    pub delivery_next: Option<u64>,
    #[serde(default)]
    pub more_deliveries: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    pub to: String,
    pub at: String,
    /// The transport accepted the attempt. This does not prove the asking agent received it.
    pub ok: bool,
    /// A relay explicitly reported delivery. A successful Codex queue command is not confirmation.
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A live client of the MCP pipe (one per `desk_register`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub handle: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_owner: Option<String>,
    /// Legacy compatibility field. New and migrated registrations are enabled automatically (false).
    pub provisional: bool,
    #[serde(default)]
    pub muted: bool,
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
    /// Legacy name proposal. Startup applies available names and clears the old review queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_owner: Option<String>,
    /// "config" (host name), "approved" (named identity), or "automatic" (generated session identity).
    pub binding: String,
    /// Owners whose Codex threads may receive `codex queue` delivery (marked on the Desk). Off by default.
    #[serde(default)]
    pub codex_interactive: bool,
    /// The relay owner may call `desk_relay` (configured on the Desk; Compass: "compass-reception").
    #[serde(default)]
    pub relay: bool,
    /// Persistent across this owner's sessions, until its deadline or a Desk unmute.
    #[serde(default)]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted_until: Option<String>,
    /// Derived from durable answers; never accepted as a writable counter.
    #[serde(default)]
    pub not_needed_count: u32,
}

/// Desk settings, stored by the daemon.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Full ntfy topic URL for the count-only push, e.g. `https://ntfy.sh/<secret-topic>`. None = off.
    pub ntfy_topic_url: Option<String>,
    /// Silence toasts (they are always silent; this turns them off entirely).
    pub toasts_off: bool,
}
