//! Opt-in startup timings. No ask text, paths from asks, tokens or answers are recorded.
//! Set AGENTBOLT_STARTUP_LOG to a JSON-lines file before launching the Desk.
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

struct Trace {
    start: Instant,
    file: File,
    seen: BTreeSet<&'static str>,
}

static TRACE: OnceLock<Option<Mutex<Trace>>> = OnceLock::new();

pub fn init() {
    let start = Instant::now();
    TRACE.get_or_init(|| {
        let path = std::env::var_os("AGENTBOLT_STARTUP_LOG")?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Mutex::new(Trace {
            start,
            file,
            seen: BTreeSet::new(),
        }))
    });
    mark("main_entered");
}

pub fn enabled() -> bool {
    TRACE.get().is_some_and(Option::is_some)
}

/// Each milestone is recorded once per process, keeping tracing out of steady-state rendering.
pub fn mark(stage: &'static str) {
    write(stage, true);
}

pub fn record(stage: &'static str) {
    write(stage, false);
}

fn write(stage: &'static str, once: bool) {
    let Some(Some(trace)) = TRACE.get() else {
        return;
    };
    let Ok(mut trace) = trace.lock() else { return };
    if once && !trace.seen.insert(stage) {
        return;
    }
    let event = serde_json::json!({
        "pid": std::process::id(),
        "stage": stage,
        "elapsed_ms": trace.start.elapsed().as_secs_f64() * 1000.0,
        "unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
    });
    let _ = writeln!(trace.file, "{event}");
}
