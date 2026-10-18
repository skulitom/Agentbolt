//! The store-backed service from agentbolt-core, behind the daemon's `DaemonService` trait.

use std::sync::Arc;

use agentbolt_core::StoreService;
use agentbolt_core::channel::Names;

use crate::daemon::DaemonService;

impl DaemonService for StoreService {
    fn hold_config(&self) -> Result<agentbolt_core::holds::Config, String> {
        StoreService::hold_config(self)
    }
    fn save_hold_scan(&self, scan: agentbolt_core::holds::Scan) -> Result<(), String> {
        StoreService::save_hold_scan(self, scan)
    }
    fn toast_allowed(&self, ask_id: &str) -> bool {
        StoreService::toast_allowed(self, ask_id)
    }
    fn notification_counts(&self) -> Result<(u32, Option<String>), String> {
        StoreService::notification_counts(self)
    }
    fn codex_delivery_allowed(&self, ask_id: &str, thread: &str) -> bool {
        StoreService::codex_delivery_allowed(self, ask_id, thread)
    }
    fn record_delivery(
        &self,
        ask_id: &str,
        to: &str,
        ok: bool,
        detail: Option<&str>,
    ) -> Result<(), String> {
        StoreService::record_delivery(self, ask_id, to, ok, detail)
    }
}

/// Open `names.db_path()` (exclusive: a second open fails) for this channel.
pub fn open(names: &Names) -> Result<Arc<dyn DaemonService>, String> {
    let svc = StoreService::open(&names.db_path()?)?.with_channel(names.channel.clone());
    Ok(Arc::new(svc))
}
