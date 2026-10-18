//! Pause at a UI frame boundary, drain the pipe worker, then allow an installer to replace the Desk.
//! No new answer is ever generated here. A crashed installer loses its pause after two minutes.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_ipc::desk_control::{Op, Prepared, endpoint, valid_transaction};
use agentbolt_ipc::{ClientInfo, Handler, PeerCheck, Pipe, Server};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use crate::worker::Req;

const LEASE: Duration = Duration::from_secs(120);

pub struct State {
    lease: Option<(String, Instant)>,
    quiescent: bool,
    closing: bool,
    pub draft_error: Option<String>,
}

impl State {
    pub fn paused(&mut self) -> bool {
        if !self.closing
            && self
                .lease
                .as_ref()
                .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            self.lease = None;
            self.quiescent = false;
        }
        self.lease.is_some()
    }

    fn owns(&mut self, transaction: &str) -> bool {
        self.paused() && self.lease.as_ref().is_some_and(|(id, _)| id == transaction)
    }

    fn prepare(&mut self, transaction: &str) -> Result<(), String> {
        if self.closing {
            return Err("The Desk is already closing for this upgrade.".into());
        }
        if !valid_transaction(transaction) {
            return Err("Invalid upgrade transaction.".into());
        }
        if self.paused() && !self.owns(transaction) {
            return Err("Another installer is preparing this Desk.".into());
        }
        if let Some(error) = &self.draft_error {
            return Err(format!("Unsent drafts are not saved: {error}"));
        }
        self.lease = Some((transaction.to_string(), Instant::now() + LEASE));
        self.quiescent = false;
        Ok(())
    }

    fn cancel(&mut self, transaction: &str) -> bool {
        if !self.closing && self.owns(transaction) {
            self.lease = None;
            self.quiescent = false;
            return true;
        }
        false
    }

    fn close(&mut self, transaction: &str) -> Result<(), String> {
        if !self.owns(transaction) || !self.quiescent {
            return Err("The Desk has not acknowledged this upgrade.".into());
        }
        self.closing = true;
        Ok(())
    }
}

pub type Gate = Arc<Mutex<State>>;

pub fn gate(draft_error: Option<String>) -> Gate {
    Arc::new(Mutex::new(State {
        lease: None,
        quiescent: false,
        closing: false,
        draft_error,
    }))
}

struct Control {
    gate: Gate,
    worker: UnboundedSender<Req>,
    ctx: eframe::egui::Context,
    warm: bool,
}

impl Handler for Control {
    async fn handle(&self, _client: &ClientInfo, value: Value) -> Result<Value, String> {
        let op: Op = serde_json::from_value(value).map_err(|e| e.to_string())?;
        match op {
            Op::Prepare { transaction } => {
                // The UI holds this lock for every logic/render mutation and draft save. Once
                // paused, it cannot enqueue another answer or change a draft behind our snapshot.
                self.gate
                    .lock()
                    .map_err(|e| e.to_string())?
                    .prepare(&transaction)?;
                self.ctx.request_repaint();
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.worker.send(Req::Quiesce(tx)).is_err() {
                    self.gate.lock().unwrap().cancel(&transaction);
                    return Err("The Desk's pipe worker has stopped.".into());
                }
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(30), rx).await,
                    Ok(Ok(()))
                ) {
                    self.gate.lock().unwrap().cancel(&transaction);
                    self.ctx.request_repaint();
                    return Err(
                        "An earlier Desk operation has not finished; upgrade was not prepared."
                            .into(),
                    );
                }
                let mut state = self.gate.lock().map_err(|e| e.to_string())?;
                if !state.owns(&transaction) {
                    return Err("The upgrade pause expired or was cancelled.".into());
                }
                state.quiescent = true;
                serde_json::to_value(Prepared {
                    pid: std::process::id(),
                    session: crate::drafts::session_id(),
                    visible: crate::resident::visible(),
                    warm: self.warm,
                })
                .map_err(|e| e.to_string())
            }
            Op::Cancel { transaction } => {
                let resumed = self
                    .gate
                    .lock()
                    .map_err(|e| e.to_string())?
                    .cancel(&transaction);
                self.ctx.request_repaint();
                Ok(json!({"resumed": resumed}))
            }
            Op::Close { transaction } => {
                self.gate
                    .lock()
                    .map_err(|e| e.to_string())?
                    .close(&transaction)?;
                // Allow the response to be written before posting the native close. This works even
                // for a hidden warm window which is not drawing egui frames.
                tokio::spawn(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    crate::resident::exit_for_upgrade();
                });
                Ok(json!({"closing": true}))
            }
        }
    }
}

pub fn serve(
    names: Names,
    gate: Gate,
    worker: UnboundedSender<Req>,
    ctx: eframe::egui::Context,
    warm: bool,
) {
    std::thread::Builder::new()
        .name("desk-upgrade".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async {
                // A same-user installer may be running from the release folder. This separate endpoint
                // only pauses/resumes/closes the window; the daemon's answer pipe stays sibling-Desk-only.
                if let Ok(server) = Server::bind_with(
                    &endpoint(&names, std::process::id()),
                    Pipe::Mcp,
                    PeerCheck::AnyLocalUser,
                ) {
                    let _ = server
                        .serve(Arc::new(Control {
                            gate,
                            worker,
                            ctx,
                            warm,
                        }))
                        .await;
                }
            });
        })
        .expect("Desk lifecycle thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_current_unexpired_transaction_can_finish_a_pause() {
        let state = gate(None);
        let mut state = state.lock().unwrap();
        let one = "1".repeat(32);
        let two = "2".repeat(32);
        state.prepare(&one).unwrap();
        assert!(state.paused() && state.owns(&one));
        assert!(state.prepare(&two).is_err());
        state.cancel(&two);
        assert!(state.paused());
        state.cancel(&one);
        assert!(!state.paused());
        state.prepare(&two).unwrap();
        state.quiescent = true;
        state.lease.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
        assert!(!state.paused() && !state.quiescent);
        state.draft_error = Some("disk full".into());
        assert!(state.prepare(&one).unwrap_err().contains("disk full"));
        assert!(!state.paused());
        state.draft_error = None;
        state.prepare(&one).unwrap();
        state.quiescent = true;
        state.close(&one).unwrap();
        assert!(
            !state.cancel(&one),
            "committed close cannot reopen an edit/send race"
        );
        state.lease.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
        assert!(state.paused(), "no editing during committed shutdown");
        assert!(state.prepare(&one).is_err());
    }
}
