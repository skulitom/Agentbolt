//! One preloaded Desk per daemon, in a kill-on-close job so a crash or uninstall cannot orphan it.
use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::log::warning;
use agentbolt_core::channel::Names;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

pub struct WarmDesk {
    names: Names,
    job: Option<HANDLE>,
    child: Option<Child>,
    attempted: Option<Instant>,
}

impl WarmDesk {
    pub fn new(names: Names) -> Self {
        let job = unsafe { CreateJobObjectW(None, None) }
            .ok()
            .and_then(|job| {
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let result = unsafe {
                    SetInformationJobObject(
                        job,
                        JobObjectExtendedLimitInformation,
                        &info as *const _ as *const _,
                        std::mem::size_of_val(&info) as u32,
                    )
                };
                if result.is_ok() {
                    Some(job)
                } else {
                    unsafe {
                        let _ = CloseHandle(job);
                    }
                    None
                }
            });
        if job.is_none() {
            warning!("couldn't create the warm Desk job; the Desk will open on demand");
        }
        Self {
            names,
            job,
            child: None,
            attempted: None,
        }
    }

    pub fn tick(&mut self) {
        let Some(job) = self.job else { return };
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) => self.child = None,
                _ => return,
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
        match Command::new(exe)
            .args(["--channel", &self.names.channel, "--warm"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                if unsafe { AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())) }.is_ok() {
                    self.child = Some(child);
                } else {
                    let _ = child.kill();
                    let _ = child.wait();
                    warning!("couldn't contain the warm Desk process; it was stopped");
                }
            }
            Err(e) => warning!("couldn't preload the Desk: {e}"),
        }
    }
}

impl Drop for WarmDesk {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            unsafe {
                let _ = CloseHandle(job);
            }
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
}
