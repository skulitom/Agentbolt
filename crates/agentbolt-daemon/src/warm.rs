//! One preloaded Desk per daemon, contained before running so a crash cannot orphan it.
use crate::log::warning;
use agentbolt_core::channel::Names;
use std::time::{Duration, Instant};

pub struct WarmDesk {
    names: Names,
    child: Option<crate::child::Child>,
    attempted: Option<Instant>,
}

impl WarmDesk {
    pub fn new(names: Names) -> Self {
        Self {
            names,
            child: None,
            attempted: None,
        }
    }

    pub fn tick(&mut self) {
        if let Some(child) = &self.child {
            match child.try_wait() {
                Ok(None) => return,
                Ok(Some(_)) => self.child = None,
                Err(e) => {
                    warning!("warm Desk status: {e}");
                    self.child = None;
                }
            }
        }
        if self
            .attempted
            .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
        {
            return;
        }
        self.attempted = Some(Instant::now());
        let Some(exe) = agentbolt_ipc::sibling_paths(agentbolt_ipc::DESK_EXE)
            .into_iter()
            .find(|p| p.is_file())
        else {
            return;
        };
        match crate::child::Child::spawn(&exe, &["--channel", &self.names.channel, "--warm"]) {
            Ok(child) => self.child = Some(child),
            Err(e) => warning!("couldn't preload the Desk: {e}"),
        }
    }
}
