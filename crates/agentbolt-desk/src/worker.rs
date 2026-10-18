//! The Desk's pipe worker: one thread with its own runtime and one Desk-pipe connection, so the window never
//! blocks on the daemon. The UI sends `Req`s; results come back as `Resp`s and wake the UI.

use std::sync::mpsc;

use agentbolt_core::channel::Names;
use agentbolt_core::model::{Owner, Settings};
use agentbolt_core::protocol::{AgentsResult, AnswerResult, DeskOp, DigestResult, ListResult, OwnersResult};
use agentbolt_ipc::{Client, IpcError, Pipe};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Debug)]
pub enum Req {
    List,
    OpenDigest,
    Answer { ask_id: String, choice: Option<String>, values: Option<Value>, note: Option<String> },
    Agents,
    Owners,
    SetOwner(Owner),
    GetSettings,
    SetSettings(Settings),
    /// Start the daemon through its sign-in task.
    StartDaemon,
}

#[derive(Debug)]
pub enum Resp {
    List(ListResult),
    Digest(DigestResult),
    Answered { ask_id: String, result: Result<AnswerResult, String> },
    Agents(AgentsResult),
    Owners(OwnersResult),
    OwnerSaved(Result<String, String>),
    Settings(Settings),
    SettingsSaved(Result<(), String>),
    Started(Result<(), String>),
    /// A read failed. `not_running` when the daemon isn't there at all.
    Failed { what: &'static str, error: String, not_running: bool },
}

pub struct Worker {
    pub tx: UnboundedSender<Req>,
    pub rx: mpsc::Receiver<Resp>,
}

/// Start the worker. `wake` is called after each response (the UI's `request_repaint`).
pub fn spawn(names: Names, wake: impl Fn() + Send + 'static) -> Worker {
    let (tx, req_rx) = unbounded_channel();
    let (resp_tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("desk-pipe".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
            rt.block_on(run(names, req_rx, resp_tx, wake));
        })
        .expect("the pipe worker thread");
    Worker { tx, rx }
}

struct Conn {
    names: Names,
    client: Option<Client>,
}

impl Conn {
    /// One op; reads are retried once on a fresh connection (the daemon may have restarted).
    async fn call(&mut self, op: &DeskOp, retry: bool) -> Result<Value, IpcError> {
        for attempt in 0..2 {
            if self.client.is_none() {
                self.client = Some(Client::connect(&self.names, Pipe::Desk).await?);
            }
            let client = self.client.as_mut().expect("connected");
            match client.call(op).await {
                Ok(v) => return Ok(v),
                Err(IpcError::Remote(e)) => return Err(IpcError::Remote(e)),
                Err(e) => {
                    self.client = None;
                    if !retry || attempt == 1 {
                        return Err(e);
                    }
                }
            }
        }
        unreachable!()
    }

    async fn get<T: DeserializeOwned>(&mut self, op: &DeskOp) -> Result<T, IpcError> {
        let v = self.call(op, true).await?;
        serde_json::from_value(v).map_err(|e| IpcError::Protocol(e.to_string()))
    }
}

async fn run(names: Names, mut rx: UnboundedReceiver<Req>, tx: mpsc::Sender<Resp>, wake: impl Fn()) {
    let mut conn = Conn { names: names.clone(), client: None };
    while let Some(req) = rx.recv().await {
        let failed = |what: &'static str, e: IpcError| Resp::Failed {
            what,
            not_running: matches!(e, IpcError::NotRunning(_)),
            error: e.to_string(),
        };
        let resp = match req {
            Req::List => conn.get(&DeskOp::List).await.map(Resp::List).unwrap_or_else(|e| failed("the Inbox", e)),
            Req::OpenDigest => {
                conn.get(&DeskOp::OpenDigest).await.map(Resp::Digest).unwrap_or_else(|e| failed("the digest", e))
            }
            Req::Agents => conn.get(&DeskOp::Agents).await.map(Resp::Agents).unwrap_or_else(|e| failed("the agents", e)),
            Req::Owners => conn.get(&DeskOp::Owners).await.map(Resp::Owners).unwrap_or_else(|e| failed("the owners", e)),
            Req::GetSettings => {
                conn.get(&DeskOp::GetSettings).await.map(Resp::Settings).unwrap_or_else(|e| failed("the settings", e))
            }
            Req::Answer { ask_id, choice, values, note } => {
                let op = DeskOp::Answer { ask_id: ask_id.clone(), choice, values, note };
                // An answer is never retried blindly: the daemon may have recorded it.
                let result = match conn.call(&op, false).await {
                    Ok(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                Resp::Answered { ask_id, result }
            }
            Req::SetOwner(owner) => {
                let name = owner.name.clone();
                Resp::OwnerSaved(conn.call(&DeskOp::SetOwner { owner }, false).await.map(|_| name).map_err(|e| e.to_string()))
            }
            Req::SetSettings(settings) => Resp::SettingsSaved(
                conn.call(&DeskOp::SetSettings { settings }, false).await.map(|_| ()).map_err(|e| e.to_string()),
            ),
            Req::StartDaemon => {
                let n = names.clone();
                let r = tokio::task::spawn_blocking(move || crate::os::run_task(&n)).await.unwrap_or_else(|e| Err(e.to_string()));
                Resp::Started(r)
            }
        };
        if tx.send(resp).is_err() {
            return;
        }
        wake();
    }
}
