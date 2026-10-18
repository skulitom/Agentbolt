//! Bounded delivery workers, independent of Tokio's pool used by the store and pipes.
use std::sync::{Arc, Mutex, mpsc};

use crate::daemon::Shared;
use crate::log::error;

const WORKERS: usize = 2;
const QUEUED: usize = 32;

pub struct Job {
    pub ask_id: String,
    pub thread: String,
    pub text: String,
}

pub struct Pool {
    sender: mpsc::SyncSender<Job>,
}

impl Pool {
    pub fn new(shared: Arc<Shared>) -> std::io::Result<Self> {
        Self::with_handler(move |job| {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::daemon::deliver_to_codex(&shared, &job.ask_id, &job.thread, &job.text);
            }))
            .is_err()
            {
                failed(
                    &shared,
                    &job,
                    "Codex delivery worker panicked; answer remains available on the Desk",
                );
            }
        })
    }

    fn with_handler(run: impl Fn(Job) + Send + Sync + 'static) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(QUEUED);
        let receiver = Arc::new(Mutex::new(receiver));
        let run = Arc::new(run);
        for i in 0..WORKERS {
            let receiver = receiver.clone();
            let run = run.clone();
            std::thread::Builder::new()
                .name(format!("agentbolt-delivery-{i}"))
                .spawn(move || {
                    loop {
                        let job = receiver.lock().unwrap_or_else(|e| e.into_inner()).recv();
                        let Ok(job) = job else { return };
                        // Release the receive lock before running a potentially ten-minute delivery.
                        run(job);
                    }
                })?;
        }
        Ok(Self { sender })
    }

    /// Never block the event loop when every worker and queue slot is occupied.
    pub fn submit(&self, job: Job) -> Result<(), Job> {
        self.sender.try_send(job).map_err(|e| match e {
            mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job) => job,
        })
    }
}

pub fn failed(shared: &Shared, job: &Job, reason: &str) {
    if let Err(e) = shared
        .svc
        .record_delivery(&job.ask_id, &job.thread, false, Some(reason))
    {
        error!(
            "ask {}: couldn't record failed Codex delivery: {e}",
            job.ask_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Gate(Arc<(Mutex<bool>, std::sync::Condvar)>);
    impl Drop for Gate {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }

    #[test]
    fn slow_deliveries_have_bounded_concurrency_and_leave_request_workers_available() {
        let gate = Gate(Arc::new((Mutex::new(false), std::sync::Condvar::new())));
        let blocked = gate.0.clone();
        let (started, rx) = mpsc::channel();
        let pool = Pool::with_handler(move |_| {
            started.send(()).unwrap();
            let (lock, condition) = &*blocked;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = condition.wait(open).unwrap();
            }
        })
        .unwrap();
        let job = || Job {
            ask_id: "test".into(),
            thread: "test".into(),
            text: "synthetic".into(),
        };
        for _ in 0..WORKERS {
            assert!(pool.submit(job()).is_ok());
        }
        for _ in 0..WORKERS {
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        for _ in 0..QUEUED {
            assert!(pool.submit(job()).is_ok());
        }
        assert!(
            pool.submit(job()).is_err(),
            "queue saturation returns the unaccepted job"
        );
        assert!(
            rx.recv_timeout(Duration::from_millis(20)).is_err(),
            "only two deliveries execute at once"
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        rt.block_on(async {
            let quick_request = tokio::task::spawn_blocking(|| 42);
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), quick_request)
                    .await
                    .unwrap()
                    .unwrap(),
                42
            );
        });
        drop(gate);
        drop(pool);
        for _ in 0..QUEUED {
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
    }
}
