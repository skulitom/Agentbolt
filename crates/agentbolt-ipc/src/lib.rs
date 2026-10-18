//! Agentbolt's local pipe transport: JSON lines over user-only named pipes on Windows, Unix sockets elsewhere.
//!
//! The API below is the contract the MCP front-end and the Desk code against. The daemon side is [`Server`]
//! (or the [`serve`] shorthand).
//!
//! Wire format: the client writes one line `{"id":N,"op":..,"args":..}` (a `protocol::Request` of a
//! `protocol::McpOp` or `protocol::DeskOp`), the server answers one line, a `protocol::Response` with the same id.
//!
//! Windows (PLAN.md, "The local channel is locked to this user"):
//! - Fresh random pipe addresses for each listener, discovered through user-only files in the channel's
//!   profile folder. Both files and pipes are created with an explicit security descriptor
//!   whose DACL grants access to the current user's SID only, `PIPE_REJECT_REMOTE_CLIENTS`, and
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE` on the first instance, so a second daemon or a squatter fails to bind.
//! - [`Client::connect`] refuses unless the process serving the pipe is `agentbolt-daemon.exe` next to the
//!   connecting executable.
//! - The Desk pipe drops, before reading anything, any client whose image isn't `agentbolt-desk.exe` next to
//!   the daemon.
//!
//! Elsewhere: Unix sockets at `names.data_dir()/<pipe name>.sock`, mode 0600 in a 0700 folder. The image checks
//! use `/proc/<pid>/exe` on Linux and are skipped on other Unixes (best effort).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use agentbolt_core::channel::Names;
use agentbolt_core::protocol::{Request, Response};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

pub use agentbolt_core::service::ClientInfo;
pub mod desk_control;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub mod windows_task;
#[cfg(windows)]
pub mod windows_util;

#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
use win as sys;

/// The daemon executable's file stem (`agentbolt-daemon.exe` on Windows).
pub const DAEMON_EXE: &str = "agentbolt-daemon";
/// The Desk executable's file stem (`agentbolt-desk.exe` on Windows).
pub const DESK_EXE: &str = "agentbolt-desk";

/// Longest request line the server reads (an ask with its evidence fits easily).
const MAX_REQUEST: usize = 1 << 20;
/// Longest response line a client reads (Inbox listings with many asks).
const MAX_RESPONSE: usize = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipe {
    /// For `agentbolt mcp` clients. Never offers answers.
    Mcp,
    /// For the installed Desk window and protocol handler only.
    Desk,
}

impl Pipe {
    /// The pipe's base name for this channel (`names.mcp_pipe` or `names.desk_pipe`).
    pub fn name<'a>(&self, names: &'a Names) -> &'a str {
        match self {
            Pipe::Mcp => &names.mcp_pipe,
            Pipe::Desk => &names.desk_pipe,
        }
    }

    /// The discovered pipe address, or `<not running>` on Windows; the socket path elsewhere.
    pub fn path(&self, names: &Names) -> PathBuf {
        sys::pipe_path(names, self.name(names))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(
        "Agentbolt isn't running (channel {0}). Start it with `agentbolt start`, or sign out and in again."
    )]
    NotRunning(String),
    #[error("the pipe {0} is served by an unexpected program ({1}); refusing to talk to it")]
    WrongServer(String, String),
    /// Binding failed because another process already serves this pipe (a second daemon, or a squatter).
    #[error("the pipe {0} is already served by another process")]
    AlreadyServed(String),
    #[error("{0}")]
    Remote(String),
    #[error("pipe I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(String),
}

/// Which peers a pipe end accepts, by executable image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerCheck {
    /// Any local process of this user (the DACL and the remote-client flag already apply). The MCP pipe's clients.
    AnyLocalUser,
    /// The peer's executable must be one of these paths (compared after canonicalising, case-insensitively on
    /// Windows).
    Image(Vec<PathBuf>),
}

impl PeerCheck {
    /// `<stem>.exe` next to the running executable. When the running executable sits in a cargo `deps` folder
    /// (a test binary), the same file in the parent folder is accepted too, which is where cargo puts the
    /// workspace's binaries. That is the "channel's build path" rule of PLAN.md; installed copies have all three
    /// executables in one folder.
    pub fn sibling(stem: &str) -> PeerCheck {
        PeerCheck::Image(sibling_paths(stem))
    }

    /// Whether a peer with this image (None = unknown) passes.
    pub fn accepts(&self, image: Option<&Path>) -> bool {
        match self {
            PeerCheck::AnyLocalUser => true,
            PeerCheck::Image(allowed) => match image {
                Some(image) => {
                    let image = normalize(image);
                    allowed.iter().any(|a| normalize(a) == image)
                }
                None => false,
            },
        }
    }
}

/// `<stem><EXE_SUFFIX>` next to the running executable, plus the parent folder when running from cargo's `deps`.
pub fn sibling_paths(stem: &str) -> Vec<PathBuf> {
    let file = format!("{stem}{}", std::env::consts::EXE_SUFFIX);
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    else {
        return Vec::new();
    };
    let mut out = vec![dir.join(&file)];
    if dir
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("deps"))
        && let Some(parent) = dir.parent()
    {
        out.push(parent.join(&file));
    }
    out
}

/// A comparable form of a path: canonical where it exists (or its folder does), without the `\\?\` prefix,
/// lower-cased on Windows.
fn normalize(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).ok().or_else(|| {
        let parent = std::fs::canonicalize(path.parent()?).ok()?;
        Some(parent.join(path.file_name()?))
    });
    let s = canonical
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .into_owned();
    if cfg!(windows) {
        let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{rest}")
        } else if let Some(rest) = s.strip_prefix(r"\\?\") {
            rest.to_string()
        } else {
            s
        };
        s.to_lowercase()
    } else {
        s
    }
}

/// The image path of a process, if the OS will say.
pub fn process_image(pid: u32) -> Option<PathBuf> {
    sys::process_image(pid)
}

// ------------------------------------------------------------------------------------------------------------
// Client
// ------------------------------------------------------------------------------------------------------------

/// A connected client. One request at a time; each call writes one line and reads one line.
pub struct Client {
    stream: BufReader<sys::ClientStream>,
    next_id: u64,
    pipe_name: String,
}

impl Client {
    /// Connect to the daemon of `names.channel` on the given pipe, checking that the server process's image
    /// path is `agentbolt-daemon.exe` next to this executable (see [`PeerCheck::sibling`]).
    pub async fn connect(names: &Names, pipe: Pipe) -> Result<Self, IpcError> {
        Self::connect_to(names, pipe, &sibling_paths(DAEMON_EXE)).await
    }

    /// Connect, accepting only a server whose image is one of `expected_server`. There is deliberately no way
    /// to skip the server check.
    pub async fn connect_to(
        names: &Names,
        pipe: Pipe,
        expected_server: &[PathBuf],
    ) -> Result<Self, IpcError> {
        let pipe_name = pipe.name(names).to_string();
        let (stream, server_pid) = sys::connect(names, &pipe_name).await?;
        let image = server_pid.and_then(sys::process_image);
        let check = PeerCheck::Image(expected_server.to_vec());
        if !sys::IMAGE_CHECKS || check.accepts(image.as_deref()) {
            Ok(Client {
                stream: BufReader::new(stream),
                next_id: 1,
                pipe_name,
            })
        } else {
            let who = match (server_pid, image) {
                (_, Some(image)) => image.display().to_string(),
                (Some(pid), None) => format!("pid {pid}, image unknown"),
                (None, None) => "unknown process".to_string(),
            };
            Err(IpcError::WrongServer(pipe_name, who))
        }
    }

    /// Send an op (serialised as `{"id":..,"op":..,"args":..}`) and return the `result` value.
    pub async fn call<T: serde::Serialize>(&mut self, op: &T) -> Result<Value, IpcError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&Request { id, op })
            .map_err(|e| IpcError::Protocol(e.to_string()))?;
        line.push(b'\n');
        self.stream.write_all(&line).await?;
        self.stream.flush().await?;
        let reply = match read_line_capped(&mut self.stream, MAX_RESPONSE).await {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                return Err(IpcError::Protocol(format!(
                    "the daemon closed {} without answering (the Desk pipe drops any program that isn't the \
                     installed agentbolt-desk)",
                    self.pipe_name
                )));
            }
            Err(e) if is_disconnect(&e) => {
                return Err(IpcError::Protocol(format!(
                    "the daemon closed {}: {e}",
                    self.pipe_name
                )));
            }
            Err(e) => return Err(e.into()),
        };
        let response: Response = serde_json::from_slice(&reply)
            .map_err(|e| IpcError::Protocol(format!("unreadable response: {e}")))?;
        if response.id != id {
            return Err(IpcError::Protocol(format!(
                "response id {} for request {id}",
                response.id
            )));
        }
        if response.ok {
            Ok(response.result.unwrap_or(Value::Null))
        } else {
            Err(IpcError::Remote(
                response.error.unwrap_or_else(|| "unknown error".into()),
            ))
        }
    }
}

fn is_disconnect(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(
        e.kind(),
        BrokenPipe | ConnectionReset | ConnectionAborted | UnexpectedEof | NotConnected
    )
}

// ------------------------------------------------------------------------------------------------------------
// Server
// ------------------------------------------------------------------------------------------------------------

/// What the daemon plugs into a [`Server`]. One instance serves every connection of a pipe.
pub trait Handler: Send + Sync + 'static {
    /// Handle one request. `op` is the request line without its `id`, i.e. `{"op":..,"args":..}`, ready for
    /// `serde_json::from_value::<McpOp>` (or `DeskOp`). Ok = the `result`; Err = the error text sent back.
    /// The future is dropped if the client disconnects while it runs.
    fn handle(
        &self,
        client: &ClientInfo,
        op: Value,
    ) -> impl Future<Output = Result<Value, String>> + Send;
    /// A client connected and passed the image check.
    fn connected(&self, _client: &ClientInfo) {}
    /// The client's connection closed.
    fn disconnected(&self, _client: &ClientInfo) {}
    /// A client was dropped before anything was read from it (the Desk pipe's image check).
    fn refused(&self, _pid: Option<u32>, _image: Option<&Path>) {}
    /// The listener hit an error it recovered from (logged by the daemon).
    fn listen_error(&self, _error: &std::io::Error) {}
}

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(1);

/// A bound pipe, before it serves. Binding is separate from serving so a daemon finds out at once that another
/// process holds the name (the first-instance flag) and can stop before it shows a tray icon.
pub struct Server {
    listener: sys::Listener,
    client_check: PeerCheck,
}

impl Server {
    /// Optional compatibility listener for MCP processes opened before the 0.2 upgrade. The daemon
    /// must continue if this old global name is occupied. New clients never discover or use it.
    #[cfg(windows)]
    pub fn bind_legacy_mcp(names: &Names) -> Result<Server, IpcError> {
        Ok(Server {
            listener: win::Listener::bind_legacy_mcp(names)?,
            client_check: PeerCheck::AnyLocalUser,
        })
    }

    /// Bind with the default client check: anyone of this user on the MCP pipe, only `agentbolt-desk.exe` next
    /// to this executable on the Desk pipe. Must be called inside a Tokio runtime.
    pub fn bind(names: &Names, pipe: Pipe) -> Result<Server, IpcError> {
        let check = match pipe {
            Pipe::Mcp => PeerCheck::AnyLocalUser,
            Pipe::Desk => PeerCheck::sibling(DESK_EXE),
        };
        Self::bind_with(names, pipe, check)
    }

    /// Bind with an explicit client check (tests, or a future pipe).
    pub fn bind_with(
        names: &Names,
        pipe: Pipe,
        client_check: PeerCheck,
    ) -> Result<Server, IpcError> {
        let listener = sys::Listener::bind(names, pipe.name(names))?;
        Ok(Server {
            listener,
            client_check,
        })
    }

    /// Accept clients until an unrecoverable listener error. Each connection runs on its own task.
    pub async fn serve<H: Handler>(self, handler: Arc<H>) -> Result<(), IpcError> {
        let Server {
            mut listener,
            client_check,
        } = self;
        let check = Arc::new(client_check);
        loop {
            let (stream, pid) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(sys::AcceptError::Transient(e)) => {
                    handler.listen_error(&e);
                    continue;
                }
                Err(sys::AcceptError::Fatal(e)) => return Err(e.into()),
            };
            let handler = handler.clone();
            let check = check.clone();
            tokio::spawn(async move {
                if sys::IMAGE_CHECKS && *check != PeerCheck::AnyLocalUser {
                    let image = pid.and_then(sys::process_image);
                    if !check.accepts(image.as_deref()) {
                        handler.refused(pid, image.as_deref());
                        sys::drop_client(stream);
                        return;
                    }
                }
                let info = ClientInfo {
                    pid,
                    connection: NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed),
                };
                handler.connected(&info);
                run_connection(stream, &info, &*handler).await;
                handler.disconnected(&info);
            });
        }
    }
}

/// Bind `pipe` with the default client check and serve it with `handler`.
pub async fn serve<H: Handler>(names: &Names, pipe: Pipe, handler: Arc<H>) -> Result<(), IpcError> {
    Server::bind(names, pipe)?.serve(handler).await
}

async fn run_connection<S, H>(stream: S, info: &ClientInfo, handler: &H)
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
    H: Handler,
{
    let (rd, mut wr) = tokio::io::split(stream);
    let mut reader = BufReader::new(rd);
    loop {
        let line = match read_line_capped(&mut reader, MAX_REQUEST).await {
            Ok(Some(line)) => line,
            Ok(None) => return,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                let _ = write_response(&mut wr, &error_response(0, "request line too long")).await;
                return;
            }
            Err(_) => return,
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let (id, op) = match split_request(&line) {
            Ok(parts) => parts,
            Err((id, why)) => {
                if write_response(&mut wr, &error_response(id, &why))
                    .await
                    .is_err()
                {
                    return;
                }
                continue;
            }
        };
        // Run the op, but notice the client going away meanwhile (a long `answers` wait), and drop the op then.
        let mut work = std::pin::pin!(handler.handle(info, op));
        let mut watching = true;
        let outcome = loop {
            if !watching {
                break Some((&mut work).await);
            }
            tokio::select! {
                result = &mut work => break Some(result),
                peek = reader.fill_buf() => match peek {
                    Ok([]) | Err(_) => break None,
                    // The client sent the next request early; it waits in the buffer.
                    Ok(_) => watching = false,
                },
            }
        };
        let Some(result) = outcome else { return };
        let response = match result {
            Ok(value) => Response {
                id,
                ok: true,
                result: Some(value),
                error: None,
            },
            Err(why) => error_response(id, &why),
        };
        if write_response(&mut wr, &response).await.is_err() {
            return;
        }
    }
}

fn error_response(id: u64, why: &str) -> Response {
    Response {
        id,
        ok: false,
        result: None,
        error: Some(why.to_string()),
    }
}

/// `{"id":7,"op":..,"args":..}` -> (7, `{"op":..,"args":..}`). On error, the id if one was readable.
fn split_request(line: &[u8]) -> Result<(u64, Value), (u64, String)> {
    let value: Value =
        serde_json::from_slice(line).map_err(|e| (0, format!("bad request: {e}")))?;
    let Value::Object(mut map) = value else {
        return Err((0, "bad request: expected a JSON object".into()));
    };
    let id = match map.remove("id") {
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or((0, "bad request: id must be a whole number".to_string()))?,
        _ => return Err((0, "bad request: missing id".into())),
    };
    if !map.contains_key("op") {
        return Err((id, "bad request: missing op".into()));
    }
    Ok((id, Value::Object(map)))
}

async fn write_response<W: AsyncWrite + Unpin>(
    wr: &mut W,
    response: &Response,
) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(response).map_err(std::io::Error::other)?;
    line.push(b'\n');
    wr.write_all(&line).await?;
    wr.flush().await
}

/// Read one `\n`-terminated line (without the terminator or a trailing `\r`). Ok(None) at end of stream.
/// A line longer than `max` is an `InvalidData` error.
async fn read_line_capped<R: AsyncBufRead + Unpin>(
    r: &mut R,
    max: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut out = Vec::new();
    loop {
        let buf = r.fill_buf().await?;
        if buf.is_empty() {
            return Ok(None);
        }
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            out.extend_from_slice(&buf[..i]);
            r.consume(i + 1);
            if out.last() == Some(&b'\r') {
                out.pop();
            }
            return if out.len() > max {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "line too long",
                ))
            } else {
                Ok(Some(out))
            };
        }
        let n = buf.len();
        out.extend_from_slice(buf);
        r.consume(n);
        if out.len() > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "line too long",
            ));
        }
    }
}

#[cfg(test)]
mod tests;
