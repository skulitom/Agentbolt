//! Talking to the daemon over the MCP pipe. A small pool of connections, so a long `desk_answers` wait doesn't
//! hold up other calls from the same host; and, when the daemon isn't running, one start through its sign-in
//! task (never a spawn of our own: PLAN.md "Starting the daemon").

use std::sync::Mutex;
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_ipc::{Client, IpcError, Pipe};
use serde::Serialize;
use serde_json::Value;

/// How long to wait for the daemon after `schtasks /Run`.
const START_WAIT: Duration = Duration::from_secs(6);
/// Don't ask Task Scheduler again within this window after a failed start.
const START_BACKOFF: Duration = Duration::from_secs(30);

pub struct Daemon {
    names: Names,
    pipe: Pipe,
    /// Start the daemon through its task when it isn't running.
    autostart: bool,
    idle: Mutex<Vec<Client>>,
    last_start: Mutex<Option<Instant>>,
}

#[derive(Debug)]
pub enum CallError {
    /// Not running, and the start through the task didn't help. The text says what happened.
    NotRunning(String),
    /// The daemon refused the op (its own error text).
    Refused(String),
    /// Anything else: a wrong server, broken pipe, bad protocol.
    Other(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::NotRunning(s) | CallError::Refused(s) | CallError::Other(s) => {
                f.write_str(s)
            }
        }
    }
}

impl Daemon {
    pub fn new(names: Names, pipe: Pipe, autostart: bool) -> Self {
        Self {
            names,
            pipe,
            autostart,
            idle: Mutex::new(Vec::new()),
            last_start: Mutex::new(None),
        }
    }

    pub fn names(&self) -> &Names {
        &self.names
    }

    /// Send one op and return its `result`.
    pub async fn call<T: Serialize + Sync>(&self, op: &T) -> Result<Value, CallError> {
        let pooled = self.idle.lock().unwrap_or_else(|e| e.into_inner()).pop();
        let was_pooled = pooled.is_some();
        let mut client = match pooled {
            Some(c) => c,
            None => self.connect().await?,
        };
        match client.call(op).await {
            Ok(v) => {
                self.put_back(client);
                Ok(v)
            }
            Err(IpcError::Remote(e)) => {
                self.put_back(client);
                Err(CallError::Refused(e))
            }
            // A pooled connection can go stale when the daemon restarts: one fresh try.
            Err(IpcError::Io(_)) if was_pooled => {
                let mut client = self.connect().await?;
                let r = client.call(op).await;
                if matches!(r, Ok(_) | Err(IpcError::Remote(_))) {
                    self.put_back(client);
                }
                r.map_err(classify)
            }
            Err(e) => Err(classify(e)),
        }
    }

    fn put_back(&self, c: Client) {
        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        if idle.len() < 4 {
            idle.push(c);
        }
    }

    async fn connect(&self) -> Result<Client, CallError> {
        match Client::connect(&self.names, self.pipe).await {
            Err(IpcError::NotRunning(_)) if self.autostart => {}
            r => return r.map_err(classify),
        }
        let started = {
            let mut last = self.last_start.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|t| t.elapsed() < START_BACKOFF) {
                None
            } else {
                *last = Some(Instant::now());
                Some(())
            }
        };
        let not_running = IpcError::NotRunning(self.names.channel.clone()).to_string();
        if started.is_none() {
            return Err(CallError::NotRunning(not_running));
        }
        let names = self.names.clone();
        let run = tokio::task::spawn_blocking(move || run_task(&names))
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
        if let Err(why) = run {
            return Err(CallError::NotRunning(format!(
                "{not_running} Its sign-in task didn't start: {why}"
            )));
        }
        let deadline = Instant::now() + START_WAIT;
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            match Client::connect(&self.names, self.pipe).await {
                Ok(c) => {
                    *self.last_start.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    return Ok(c);
                }
                Err(IpcError::NotRunning(_)) if Instant::now() < deadline => continue,
                Err(IpcError::NotRunning(_)) => {
                    return Err(CallError::NotRunning(format!(
                        "{not_running} Its sign-in task ran, but the daemon didn't answer within {} s.",
                        START_WAIT.as_secs()
                    )));
                }
                Err(e) => return Err(classify(e)),
            }
        }
    }
}

fn classify(e: IpcError) -> CallError {
    match e {
        IpcError::NotRunning(_) => CallError::NotRunning(e.to_string()),
        IpcError::Remote(s) => CallError::Refused(s),
        other => CallError::Other(other.to_string()),
    }
}

/// `schtasks /Run /TN <task>`. Ok when Task Scheduler accepted the start.
#[cfg(windows)]
pub fn run_task(names: &Names) -> Result<(), String> {
    crate::install::schtasks(&["/Run", "/TN", &names.task])
        .map(|_| ())
        .map_err(|e| {
            if crate::install::task_exists(names) {
                e
            } else {
                format!(
                    "there's no task '{}'; install Agentbolt with `agentbolt install`.",
                    names.task
                )
            }
        })
}

#[cfg(not(windows))]
pub fn run_task(_names: &Names) -> Result<(), String> {
    Err("the sign-in task exists only on Windows".into())
}
