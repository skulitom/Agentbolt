//! The Desk's pipe worker: one thread with its own runtime and one Desk-pipe connection, so the window never
//! blocks on the daemon. The UI sends `Req`s; results come back as `Resp`s and wake the UI.

use std::sync::mpsc;

use agentbolt_core::channel::Names;
use agentbolt_core::model::{Agent, Owner, Settings};
use agentbolt_core::protocol::{
    AnswerResult, BatchAnswerResult, DeskAnswer, DeskOp, DirectoryPage, DirectoryQuery,
    InboxResult, StatusResult,
};
use agentbolt_ipc::{Client, IpcError, Pipe};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Debug)]
pub enum Req {
    /// A barrier after all prior operations; closes the connection before the daemon is upgraded.
    Quiesce(tokio::sync::oneshot::Sender<()>),
    AnswerBatch(Vec<DeskAnswer>),
    /// The Inbox; marks its asks shown. Only while the window is seen.
    List(Option<agentbolt_core::protocol::InboxCursor>),
    History(agentbolt_core::protocol::HistoryQuery),
    /// The counts only; marks nothing shown.
    Status,
    DeliveryHistory(agentbolt_core::protocol::DeliveryQuery),
    DraftStates(Vec<String>),
    Answer {
        ask_id: String,
        choice: Option<String>,
        values: Option<Value>,
        note: Option<String>,
    },
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
    Agents(DirectoryQuery),
    Board(agentbolt_core::board::DeskQuery),
    Holds(Option<String>),
    SetHolds(agentbolt_core::holds::Config),
    ImportFocus {
        path: String,
        excluded: Vec<String>,
    },
    Metrics,
    StartBaseline {
        label: String,
    },
    RecordVerdict {
        text: String,
    },
    SetLease {
        owner: String,
        seconds: u32,
    },
    Owners(DirectoryQuery),
    SetOwner(Owner),
    ResolveOwner {
        owner: String,
        requested_owner: String,
        approve: bool,
    },
    GetSettings,
    Phone,
    SetPhone(agentbolt_core::phone::Update),
    SetSettings(Settings),
    /// Start the daemon through its sign-in task.
    StartDaemon,
}

#[derive(Debug)]
pub enum Resp {
    BatchAnswered {
        ids: Vec<String>,
        result: Result<BatchAnswerResult, String>,
    },
    List {
        after: Option<agentbolt_core::protocol::InboxCursor>,
        result: Result<InboxResult, String>,
    },
    History {
        query: agentbolt_core::protocol::HistoryQuery,
        result: Result<agentbolt_core::protocol::HistoryResult, String>,
    },
    Status(StatusResult),
    DeliveryHistory {
        query: agentbolt_core::protocol::DeliveryQuery,
        result: Result<agentbolt_core::protocol::DeliveryPage, String>,
    },
    DraftStates {
        ids: Vec<String>,
        result: Result<agentbolt_core::protocol::DraftStates, String>,
    },
    Answered {
        ask_id: String,
        result: Result<AnswerResult, String>,
    },
    Agents {
        query: DirectoryQuery,
        result: Result<DirectoryPage<Agent>, String>,
    },
    Board {
        query: agentbolt_core::board::DeskQuery,
        result: Result<agentbolt_core::board::Snapshot, String>,
    },
    Holds {
        after: Option<String>,
        result: Result<agentbolt_core::holds::Snapshot, String>,
    },
    HoldsSaved(Result<(), String>),
    FocusImported(Result<agentbolt_core::holds::Config, String>),
    Metrics(Box<agentbolt_core::metrics::Report>),
    MetricsSaved(Result<(), String>),
    LeaseSaved(Result<(), String>),
    Owners {
        query: DirectoryQuery,
        result: Result<DirectoryPage<Owner>, String>,
    },
    OwnerSaved(Result<String, String>),
    Settings(Settings),
    Phone(agentbolt_core::phone::Status),
    PhoneSaved(Result<(), String>),
    SettingsSaved(Result<(), String>),
    Started(Result<(), String>),
    /// A read failed. `not_running` when the daemon isn't there at all.
    Failed {
        what: &'static str,
        error: String,
        not_running: bool,
    },
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
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a tokio runtime");
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

async fn run(
    names: Names,
    mut rx: UnboundedReceiver<Req>,
    tx: mpsc::Sender<Resp>,
    wake: impl Fn(),
) {
    let mut conn = Conn {
        names: names.clone(),
        client: None,
    };
    while let Some(req) = rx.recv().await {
        let failed = |what: &'static str, e: IpcError| Resp::Failed {
            what,
            not_running: matches!(e, IpcError::NotRunning(_)),
            error: e.to_string(),
        };
        let resp = match req {
            Req::Quiesce(done) => {
                conn.client = None;
                let _ = done.send(());
                continue;
            }
            Req::AnswerBatch(answers) => {
                let ids = answers.iter().map(|a| a.ask_id.clone()).collect();
                let result = match conn.call(&DeskOp::AnswerBatch { answers }, false).await {
                    Ok(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                Resp::BatchAnswered { ids, result }
            }
            Req::History(query) => {
                let result = conn
                    .get(&DeskOp::History {
                        query: query.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string());
                Resp::History { query, result }
            }
            Req::List(after) => {
                let result = conn
                    .get(&DeskOp::InboxPage {
                        after: after.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string());
                Resp::List { after, result }
            }
            Req::DeliveryHistory(query) => {
                let result = conn
                    .get(&DeskOp::DeliveryHistory {
                        query: query.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string());
                Resp::DeliveryHistory { query, result }
            }
            Req::DraftStates(ids) => {
                let result = conn
                    .get(&DeskOp::DraftStates { ids: ids.clone() })
                    .await
                    .map_err(|e| e.to_string());
                Resp::DraftStates { ids, result }
            }
            Req::Status => conn
                .get(&DeskOp::Status)
                .await
                .map(Resp::Status)
                .unwrap_or_else(|e| failed("the Inbox", e)),
            Req::Agents(query) => {
                let result = conn
                    .get(&DeskOp::AgentDirectory {
                        query: query.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string());
                Resp::Agents { query, result }
            }
            Req::Board(query) => {
                let result = conn
                    .get(&DeskOp::BoardPage {
                        query: query.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string());
                Resp::Board { query, result }
            }
            Req::Holds(after) => {
                let result = conn
                    .get(&DeskOp::HoldsPage {
                        after: after.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string());
                Resp::Holds { after, result }
            }
            Req::SetHolds(config) => Resp::HoldsSaved(
                conn.call(&DeskOp::SetHolds { config }, false)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            Req::ImportFocus { path, excluded } => {
                Resp::FocusImported(import_focus(&path, excluded))
            }
            Req::Metrics => conn
                .get(&DeskOp::Metrics)
                .await
                .map(Resp::Metrics)
                .unwrap_or_else(|e| failed("activity", e)),
            Req::StartBaseline { label } => Resp::MetricsSaved(
                conn.call(&DeskOp::StartBaseline { label }, false)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            Req::RecordVerdict { text } => Resp::MetricsSaved(
                conn.call(&DeskOp::RecordVerdict { text }, false)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            Req::SetLease { owner, seconds } => Resp::LeaseSaved(
                conn.call(&DeskOp::SetLease { owner, seconds }, false)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            Req::Owners(query) => {
                let result = conn
                    .get(&DeskOp::OwnerDirectory {
                        query: query.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string());
                Resp::Owners { query, result }
            }
            Req::GetSettings => conn
                .get(&DeskOp::GetSettings)
                .await
                .map(Resp::Settings)
                .unwrap_or_else(|e| failed("the settings", e)),
            Req::Phone => conn
                .get(&DeskOp::Phone)
                .await
                .map(Resp::Phone)
                .unwrap_or_else(|e| failed("phone replies", e)),
            Req::SetPhone(config) => Resp::PhoneSaved(
                conn.call(&DeskOp::SetPhone { config }, false)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            Req::Answer {
                ask_id,
                choice,
                values,
                note,
            } => {
                let op = DeskOp::Answer {
                    ask_id: ask_id.clone(),
                    choice,
                    values,
                    note,
                };
                // An answer is never retried blindly: the daemon may have recorded it.
                let result = match conn.call(&op, false).await {
                    Ok(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                Resp::Answered { ask_id, result }
            }
            Req::SetOwner(owner) => {
                let name = owner.name.clone();
                Resp::OwnerSaved(
                    conn.call(&DeskOp::SetOwner { owner }, false)
                        .await
                        .map(|_| name)
                        .map_err(|e| e.to_string()),
                )
            }
            Req::ResolveOwner {
                owner,
                requested_owner,
                approve,
            } => {
                let name = if approve {
                    requested_owner.clone()
                } else {
                    owner.clone()
                };
                Resp::OwnerSaved(
                    conn.call(
                        &DeskOp::ResolveOwner {
                            owner,
                            requested_owner,
                            approve,
                        },
                        false,
                    )
                    .await
                    .map(|_| name)
                    .map_err(|e| e.to_string()),
                )
            }
            Req::NotNeeded { ask_id, note } => {
                let op = DeskOp::NotNeeded {
                    ask_id: ask_id.clone(),
                    note,
                };
                let result = match conn.call(&op, false).await {
                    Ok(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                Resp::Answered { ask_id, result }
            }
            Req::MuteFor { owner, seconds } => Resp::OwnerSaved(
                conn.call(
                    &DeskOp::MuteFor {
                        owner: owner.clone(),
                        seconds,
                    },
                    false,
                )
                .await
                .map(|_| owner)
                .map_err(|e| e.to_string()),
            ),
            Req::SetMuted { owner, muted } => Resp::OwnerSaved(
                conn.call(
                    &DeskOp::SetMuted {
                        owner: owner.clone(),
                        muted,
                    },
                    false,
                )
                .await
                .map(|_| owner)
                .map_err(|e| e.to_string()),
            ),
            Req::SetSettings(settings) => Resp::SettingsSaved(
                conn.call(&DeskOp::SetSettings { settings }, false)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            ),
            Req::StartDaemon => {
                let n = names.clone();
                let r = tokio::task::spawn_blocking(move || crate::os::run_task(&n))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                Resp::Started(r)
            }
        };
        if tx.send(resp).is_err() {
            return;
        }
        wake();
    }
}

fn import_focus(
    path: &str,
    excluded: Vec<String>,
) -> Result<agentbolt_core::holds::Config, String> {
    use std::io::Read;
    let path = std::path::Path::new(path.trim());
    if !path.is_absolute()
        || !path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("FOCUS.json"))
    {
        return Err("Choose an absolute path to FOCUS.json".into());
    }
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(2_097_153)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2_097_152 {
        return Err("FOCUS.json exceeds 2 MiB".into());
    }
    agentbolt_core::holds::from_focus(
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        excluded,
    )
}
