//! The sign-in task runs this small process. A clean Quit exits it; a crashed worker is restarted.

use crate::log::{error, info, warning};
use agentbolt_core::channel::Names;
use std::time::{Duration, Instant};

pub fn run(names: &Names, headless: bool) -> i32 {
    let mut guard_name = names.clone();
    guard_name.mutex.push_str("-supervisor");
    let _instance = match crate::winutil::single_instance(&guard_name) {
        Ok(Some(instance)) => instance,
        Ok(None) => return 0,
        Err(e) => {
            error!("supervisor mutex: {e}");
            return 1;
        }
    };
    let Ok(exe) = std::env::current_exe() else {
        return 1;
    };
    let mut args = vec!["--channel", names.channel.as_str()];
    if headless {
        args.push("--headless");
    }
    let mut failures = 0u32;
    loop {
        let started = Instant::now();
        let result = crate::child::Child::spawn(&exe, &args).and_then(|child| child.wait());
        match result {
            Ok(0) => {
                info!("daemon stopped normally; supervisor exiting");
                return 0;
            }
            Ok(code) => warning!("daemon exited with code {code}; restarting"),
            Err(e) => warning!("daemon start/wait failed: {e}; retrying"),
        }
        if started.elapsed() >= Duration::from_secs(60) {
            failures = 0;
        }
        let delay = retry_delay(failures);
        failures = failures.saturating_add(1);
        std::thread::sleep(delay);
    }
}

fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.min(5)).min(30))
}

#[cfg(test)]
mod tests {
    #[test]
    fn crash_loops_back_off_without_stopping_recovery() {
        assert_eq!(
            (0..8)
                .map(|n| super::retry_delay(n).as_secs())
                .collect::<Vec<_>>(),
            [1, 2, 4, 8, 16, 30, 30, 30]
        );
        assert_eq!(super::retry_delay(u32::MAX).as_secs(), 30);
    }
}
