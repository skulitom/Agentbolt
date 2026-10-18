//! A child contained before its first instruction. Closing the job kills it and its descendants.
//! Used by both the supervisor and the preloaded Desk, including when their owner crashes.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessW, GetExitCodeProcess, INFINITE,
    PROCESS_INFORMATION, ResumeThread, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

fn handle(h: &OwnedHandle) -> HANDLE {
    HANDLE(h.as_raw_handle())
}

/// Quote one argument using Windows' backslash/quote rules, preserving UTF-16 paths.
fn quoted(s: &OsStr) -> Result<Vec<u16>, String> {
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for c in s.encode_wide() {
        if c == 0 {
            return Err("a process argument contains NUL".into());
        }
        if c == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            b'\\' as u16,
            if c == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    Ok(out)
}

pub struct Child {
    job: Option<OwnedHandle>,
    process: OwnedHandle,
}

impl Child {
    pub fn spawn(exe: &Path, args: &[&str]) -> Result<Self, String> {
        let mut command = quoted(exe.as_os_str())?;
        for arg in args {
            command.push(b' ' as u16);
            command.extend(quoted(OsStr::new(arg))?);
        }
        command.push(0);
        let mut application: Vec<_> = exe.as_os_str().encode_wide().collect();
        application.push(0);
        unsafe {
            let job = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
            let job = OwnedHandle::from_raw_handle(job.0);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle(&job),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
            .map_err(|e| e.to_string())?;
            let startup = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                ..Default::default()
            };
            let mut info = PROCESS_INFORMATION::default();
            CreateProcessW(
                PCWSTR(application.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_NO_WINDOW | CREATE_SUSPENDED,
                None,
                PCWSTR::null(),
                &startup,
                &mut info,
            )
            .map_err(|e| e.to_string())?;
            let process = OwnedHandle::from_raw_handle(info.hProcess.0);
            let thread = OwnedHandle::from_raw_handle(info.hThread.0);
            let contained = AssignProcessToJobObject(handle(&job), handle(&process));
            if contained.is_err() || ResumeThread(handle(&thread)) == u32::MAX {
                let _ = TerminateProcess(handle(&process), 1);
                let _ = WaitForSingleObject(handle(&process), 5000);
                return Err("couldn't contain or resume the child process".into());
            }
            Ok(Self {
                job: Some(job),
                process,
            })
        }
    }

    pub fn try_wait(&self) -> Result<Option<u32>, String> {
        self.wait_for(0)
    }
    pub fn wait(&self) -> Result<u32, String> {
        self.wait_for(INFINITE)?
            .ok_or_else(|| "process wait ended unexpectedly".into())
    }
    fn wait_for(&self, timeout: u32) -> Result<Option<u32>, String> {
        unsafe {
            match WaitForSingleObject(handle(&self.process), timeout) {
                WAIT_TIMEOUT => Ok(None),
                WAIT_OBJECT_0 => {
                    let mut code = 0;
                    GetExitCodeProcess(handle(&self.process), &mut code)
                        .map_err(|e| e.to_string())?;
                    Ok(Some(code))
                }
                _ => Err(windows::core::Error::from_thread().to_string()),
            }
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        drop(self.job.take());
        unsafe {
            let _ = WaitForSingleObject(handle(&self.process), 5000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_preserve_spaces_quotes_backslashes_and_unicode() {
        let q = |s: &str| String::from_utf16(&quoted(OsStr::new(s)).unwrap()).unwrap();
        assert_eq!(q("a b"), "\"a b\"");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q("a\"b"), "\"a\\\"b\"");
        assert_eq!(q("c:\\x\\"), "\"c:\\x\\\\\"");
        assert_eq!(q("é 中"), "\"é 中\"");
        assert!(quoted(OsStr::new("a\0b")).is_err());
    }
}
