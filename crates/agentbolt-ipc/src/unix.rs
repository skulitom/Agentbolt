//! Unix sockets for Linux and macOS (CI, and the MCP front-end's discovery on Linux). The socket is mode 0600 in a
//! 0700 data folder; image checks read `/proc/<pid>/exe` on Linux and are skipped elsewhere (best effort).

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use agentbolt_core::channel::Names;
use tokio::net::{UnixListener, UnixStream};

use crate::IpcError;

pub(crate) const IMAGE_CHECKS: bool = cfg!(target_os = "linux");

pub(crate) type ClientStream = UnixStream;

/// The socket's path, for display (`Pipe::path`). Without a data folder (no `$HOME`) it says so instead.
pub(crate) fn pipe_path(names: &Names, name: &str) -> PathBuf {
    socket_path(names, name).unwrap_or_else(|_| PathBuf::from(format!("<no data folder>/{name}.sock")))
}

/// The socket's path in the channel's data folder; an error when there's no data folder.
fn socket_path(names: &Names, name: &str) -> Result<PathBuf, IpcError> {
    Ok(data_dir(names)?.join(format!("{name}.sock")))
}

fn data_dir(names: &Names) -> Result<PathBuf, IpcError> {
    names.data_dir().map_err(|e| IpcError::Io(io::Error::new(io::ErrorKind::NotFound, e)))
}

pub(crate) enum AcceptError {
    Transient(io::Error),
    #[allow(dead_code)]
    Fatal(io::Error),
}

pub(crate) struct Listener {
    path: PathBuf,
    listener: UnixListener,
}

impl Listener {
    pub(crate) fn bind(names: &Names, name: &str) -> Result<Self, IpcError> {
        let dir = data_dir(names)?;
        std::fs::create_dir_all(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        let path = socket_path(names, name)?;
        if path.exists() {
            // A live server answers; a stale socket from a crash doesn't, and is replaced.
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                return Err(IpcError::AlreadyServed(path.display().to_string()));
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::AddrInUse {
                IpcError::AlreadyServed(path.display().to_string())
            } else {
                IpcError::Io(e)
            }
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self { path, listener })
    }

    pub(crate) async fn accept(&mut self) -> Result<(UnixStream, Option<u32>), AcceptError> {
        let (stream, _) = self.listener.accept().await.map_err(AcceptError::Transient)?;
        let pid = stream.peer_cred().ok().and_then(|c| c.pid()).and_then(|p| u32::try_from(p).ok());
        Ok((stream, pid))
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn drop_client(stream: UnixStream) {
    drop(stream);
}

pub(crate) async fn connect(names: &Names, name: &str) -> Result<(UnixStream, Option<u32>), IpcError> {
    let path = socket_path(names, name)?;
    let stream = match UnixStream::connect(&path).await {
        Ok(stream) => stream,
        Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
            return Err(IpcError::NotRunning(names.channel.clone()));
        }
        Err(e) => return Err(IpcError::Io(e)),
    };
    let pid = stream.peer_cred().ok().and_then(|c| c.pid()).and_then(|p| u32::try_from(p).ok());
    Ok((stream, pid))
}

pub(crate) fn process_image(pid: u32) -> Option<PathBuf> {
    if cfg!(target_os = "linux") { std::fs::read_link(format!("/proc/{pid}/exe")).ok() } else { None }
}
