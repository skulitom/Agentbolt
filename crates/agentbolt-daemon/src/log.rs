//! The daemon's log: `<data dir>/agentbolt.log`, appended, rolled to `agentbolt.log.old` at 1 MB.
//! Lines carry no ask content (titles, bodies, answers): only ids, counts and what went wrong.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub const ROLL_AT: u64 = 1024 * 1024;

pub struct Log {
    path: PathBuf,
    roll_at: u64,
    file: Mutex<Option<File>>,
}

impl Log {
    pub fn new(path: PathBuf, roll_at: u64) -> Self {
        Log { path, roll_at, file: Mutex::new(None) }
    }

    pub fn write(&self, level: &str, message: &str) {
        let Ok(mut guard) = self.file.lock() else { return };
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size >= self.roll_at {
            *guard = None;
            let old = self.path.with_extension("log.old");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&self.path, &old);
        }
        if guard.is_none() {
            *guard = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        }
        if let Some(file) = guard.as_mut() {
            let now = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            let message = message.replace(['\r', '\n'], " ");
            let _ = writeln!(file, "{now} {level} {message}");
            let _ = file.flush();
        }
    }
}

static LOG: OnceLock<Log> = OnceLock::new();

/// Install the process log (first call wins).
pub fn init(path: PathBuf) -> &'static Log {
    LOG.get_or_init(|| Log::new(path, ROLL_AT))
}

pub fn write(level: &str, message: &str) {
    if let Some(log) = LOG.get() {
        log.write(level, message);
    }
}

macro_rules! info {
    ($($t:tt)*) => { $crate::log::write("INFO", &format!($($t)*)) };
}
macro_rules! warning {
    ($($t:tt)*) => { $crate::log::write("WARN", &format!($($t)*)) };
}
macro_rules! error {
    ($($t:tt)*) => { $crate::log::write("ERROR", &format!($($t)*)) };
}
pub(crate) use {error, info, warning};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_rolls_to_old_at_the_limit() {
        let dir = crate::testutil::TempDir::new("log");
        let log = Log::new(dir.path().join("agentbolt.log"), 1000);
        for i in 0..40 {
            log.write("INFO", &format!("line {i} {}", "x".repeat(40)));
        }
        let old = dir.path().join("agentbolt.log.old");
        assert!(old.exists(), "rolled to .old");
        let current = std::fs::metadata(dir.path().join("agentbolt.log")).unwrap().len();
        assert!(current < 1100, "the live log restarted small: {current}");
        assert!(std::fs::read_to_string(&old).unwrap().contains("INFO line"));
        log.write("WARN", "two\nlines");
        let text = std::fs::read_to_string(dir.path().join("agentbolt.log")).unwrap();
        assert!(text.contains("WARN two lines"), "newlines can't forge log lines");
    }
}
