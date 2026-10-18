//! The daemon's brain, behind the pipes. The IPC layer decodes a line into an op and calls `Service`;
//! `Service` owns all rules and the store. Implemented in `service_impl` (store-backed).

use crate::protocol::{DeskOp, McpOp};

/// Who is on the other end of the MCP pipe, as the daemon saw them (not as they claim).
#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    /// The client process id (from the pipe), if known.
    pub pid: Option<u32>,
    /// The connection's own id, used to tie handles to a connection for liveness.
    pub connection: u64,
}

/// What the daemon must do after an operation, outside the store (the daemon acts on these).
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// The Inbox changed: refresh the tray count and, if configured, the ntfy count push.
    InboxChanged {
        open: u32,
        /// Only unmuted owners contribute to the count-only phone notification.
        notifiable: u32,
        /// Earliest unmuted deadline.
        next_deadline: Option<String>,
    },
    /// Show a toast for this ask. Buttons carry per-button tokens (reversible choose/approve asks only).
    Toast {
        ask_id: String,
        /// Stable, opaque owner tag: updates replace the same notification.
        tag: String,
        /// A waiting-count update goes to Notification Center without another popup.
        suppress_popup: bool,
        title: String,
        body: String,
        buttons: Vec<(String, String)>,
    },
    /// An answer exists for an owner marked codex-interactive: queue it into the thread.
    CodexQueue {
        ask_id: String,
        thread: String,
        text: String,
    },
}

pub trait Service: Send + Sync {
    /// Handle one MCP-pipe op. Ok = the JSON `result`; Err = the error text sent back.
    fn mcp(
        &self,
        client: &ClientInfo,
        op: McpOp,
    ) -> Result<(serde_json::Value, Vec<Effect>), String>;
    /// Handle one Desk-pipe op. Only called for a client the IPC layer verified as the installed Desk.
    fn desk(&self, op: DeskOp) -> Result<(serde_json::Value, Vec<Effect>), String>;
    /// Called about once a second: defaults (only if shown and reversible), due toasts and handle expiry.
    fn tick(&self, now: time::OffsetDateTime) -> Vec<Effect>;
    /// The client connection closed (handles stay; liveness is per handle, PLAN.md rule 5).
    fn disconnected(&self, connection: u64);
}
