//! Agentbolt's local pipe transport: JSON lines over user-only named pipes on Windows, Unix sockets elsewhere.
//!
//! The API below is the contract the MCP front-end and the Desk code against. The daemon side is `serve`.

use agentbolt_core::channel::Names;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipe {
    /// For `agentbolt mcp` clients. Never offers answers.
    Mcp,
    /// For the installed Desk window and protocol handler only.
    Desk,
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("Agentbolt isn't running (channel {0}). Start it with `agentbolt start`, or sign out and in again.")]
    NotRunning(String),
    #[error("the pipe {0} is served by an unexpected program ({1}); refusing to talk to it")]
    WrongServer(String, String),
    #[error("{0}")]
    Remote(String),
    #[error("pipe I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(String),
}

/// A connected client. One request at a time; each call writes one line and reads one line.
pub struct Client {
    _private: (),
}

impl Client {
    /// Connect to the daemon of `names.channel` on the given pipe, checking that the server process's image
    /// path is the installed (or the channel's) daemon executable.
    pub async fn connect(_names: &Names, _pipe: Pipe) -> Result<Self, IpcError> {
        todo!("IPC work")
    }

    /// Send an op (serialised as `{"id":..,"op":..,"args":..}`) and return the `result` value.
    pub async fn call<T: serde::Serialize>(&mut self, _op: &T) -> Result<serde_json::Value, IpcError> {
        todo!("IPC work")
    }
}
