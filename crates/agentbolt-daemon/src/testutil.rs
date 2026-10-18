//! Test helpers: throwaway channels, temp folders, and a small in-memory `Service`.

use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_core::model::Settings;
use agentbolt_core::protocol::{DeskOp, McpOp};
use agentbolt_core::service::{ClientInfo, Effect, Service};
use serde_json::{Value, json};

use crate::daemon::DaemonService;

fn random_hex() -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    );
    h.write_u32(std::process::id());
    format!("{:012x}", h.finish() & 0xffff_ffff_ffff)
}

/// A fresh `ipctest-<random>` channel (never `main`).
pub fn channel() -> Names {
    let names = Names::for_channel(&format!("ipctest-{}", random_hex())).unwrap();
    assert!(!names.is_main());
    names
}

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(what: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("agentbolt-daemon-test-{what}-{}", random_hex()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[derive(Default)]
struct State {
    waiting: Vec<String>,
    answered: Vec<String>,
    answers_calls: usize,
    settings: Settings,
    disconnects: usize,
    deliveries: Vec<(String, String, bool, Option<String>)>,
}

/// Stands in for the store-backed service: asks are ids in `waiting` until `answer` moves them.
#[derive(Default)]
pub struct FakeService(Mutex<State>);

impl FakeService {
    pub fn set_waiting(&self, ids: &[&str]) {
        self.0.lock().unwrap().waiting = ids.iter().map(|s| s.to_string()).collect();
    }
    pub fn answer(&self, id: &str) {
        let mut s = self.0.lock().unwrap();
        s.waiting.retain(|w| w != id);
        s.answered.push(id.to_string());
    }
    pub fn answers_calls(&self) -> usize {
        self.0.lock().unwrap().answers_calls
    }
    pub fn settings(&self) -> Settings {
        self.0.lock().unwrap().settings.clone()
    }
    pub fn disconnects(&self) -> usize {
        self.0.lock().unwrap().disconnects
    }
    pub fn deliveries(&self) -> Vec<(String, String, bool, Option<String>)> {
        self.0.lock().unwrap().deliveries.clone()
    }
}

impl Service for FakeService {
    fn mcp(&self, _client: &ClientInfo, op: McpOp) -> Result<(Value, Vec<Effect>), String> {
        let mut s = self.0.lock().unwrap();
        match op {
            McpOp::Answers { .. } => {
                s.answers_calls += 1;
                let answers: Vec<Value> = s.answered.iter().map(|id| json!({ "id": id })).collect();
                Ok((
                    json!({ "answers": answers, "waiting": s.waiting, "cancelled": [], "unknown": [] }),
                    vec![],
                ))
            }
            McpOp::Status => {
                let open = s.waiting.len() as u32;
                Ok((
                    json!({ "open": open }),
                    vec![Effect::InboxChanged {
                        open,
                        notifiable: open,
                        next_deadline: None,
                    }],
                ))
            }
            _ => Ok((json!({}), vec![])),
        }
    }

    fn desk(&self, op: DeskOp) -> Result<(Value, Vec<Effect>), String> {
        let mut s = self.0.lock().unwrap();
        match op {
            DeskOp::GetSettings => Ok((serde_json::to_value(&s.settings).unwrap(), vec![])),
            DeskOp::SetSettings { settings } => {
                s.settings = settings;
                Ok((json!({}), vec![]))
            }
            DeskOp::Status => Ok((json!({ "open": s.waiting.len(), "digest": 0 }), vec![])),
            _ => Ok((json!({}), vec![])),
        }
    }

    fn tick(&self, _now: time::OffsetDateTime) -> Vec<Effect> {
        Vec::new()
    }

    fn disconnected(&self, _connection: u64) {
        self.0.lock().unwrap().disconnects += 1;
    }
}

impl DaemonService for FakeService {
    fn record_delivery(
        &self,
        ask_id: &str,
        to: &str,
        ok: bool,
        detail: Option<&str>,
    ) -> Result<(), String> {
        self.0.lock().unwrap().deliveries.push((
            ask_id.into(),
            to.into(),
            ok,
            detail.map(str::to_string),
        ));
        Ok(())
    }
}
