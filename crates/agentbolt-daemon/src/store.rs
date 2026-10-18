//! The store-backed service from agentbolt-core, behind the daemon's `DaemonService` trait.

use std::sync::Arc;

use agentbolt_core::StoreService;
use agentbolt_core::channel::Names;

use crate::daemon::DaemonService;
use crate::log::warning;

impl DaemonService for StoreService {
    fn record_delivery(&self, ask_id: &str, to: &str, ok: bool, detail: Option<&str>) -> Result<(), String> {
        StoreService::record_delivery(self, ask_id, to, ok, detail)
    }

    fn set_utc_offset_minutes(&self, minutes: i32) -> Result<(), String> {
        StoreService::set_utc_offset_minutes(self, minutes)
    }
}

/// Open `names.db_path()` (exclusive: a second open fails) and set the budget's local day from the PC's clock.
pub fn open(names: &Names) -> Result<Arc<dyn DaemonService>, String> {
    let svc = StoreService::open(&names.db_path()?)?.with_channel(names.channel.clone());
    if let Some(minutes) = local_offset_minutes()
        && let Err(e) = svc.set_utc_offset_minutes(minutes)
    {
        warning!("utc offset: {e}");
    }
    Ok(Arc::new(svc))
}

/// Minutes east of UTC on this PC now (BST = 60). On Unix this only works before other threads start.
pub fn local_offset_minutes() -> Option<i32> {
    let offset = time::UtcOffset::current_local_offset().ok()?;
    Some(offset.whole_minutes() as i32)
}
