//! Windows paths and coordination shared by the installer, daemon and Desk.

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};

use agentbolt_core::channel::Names;
use windows::Win32::Foundation::{ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_MODE,
    OPEN_ALWAYS,
};
use windows::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
use windows::core::HSTRING;

fn os_directory(system: bool) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        let len = unsafe {
            if system {
                GetSystemDirectoryW(Some(&mut buffer))
            } else {
                GetWindowsDirectoryW(Some(&mut buffer))
            }
        } as usize;
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        if len < buffer.len() {
            let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len]));
            if !path.is_absolute() {
                return Err(io::Error::other("Windows returned a relative system path"));
            }
            return Ok(path);
        }
        buffer.resize(len + 1, 0);
    }
}

/// Use the OS directory API, never PATH, the download folder or an environment variable.
pub fn schtasks() -> io::Result<PathBuf> {
    Ok(os_directory(true)?.join("schtasks.exe"))
}
pub fn explorer() -> io::Result<PathBuf> {
    Ok(os_directory(false)?.join("explorer.exe"))
}

pub fn current_user_sid() -> io::Result<String> {
    crate::win::current_user_sid()
}

/// A retained process handle avoids confusing a reused pid with the process prepared for upgrade.
pub struct RunningImage {
    pub pid: u32,
    pub session: u32,
    pub image: PathBuf,
    handle: OwnedHandle,
}

impl RunningImage {
    pub fn exited(&self) -> io::Result<bool> {
        use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
        let state = unsafe {
            windows::Win32::System::Threading::WaitForSingleObject(
                HANDLE(self.handle.as_raw_handle()),
                0,
            )
        };
        match state {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

/// Only exact, verified executable images, across this user's accessible Windows sessions.
/// No command-line, process-name-only or pid-only match grants authority to affect a process.
pub fn running_images(expected: &[PathBuf]) -> io::Result<Vec<RunningImage>> {
    use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    let raw =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(io::Error::other)?;
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let snapshot = HANDLE(snapshot.as_raw_handle());
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let check = crate::PeerCheck::Image(expected.to_vec());
    let mut step = unsafe { Process32FirstW(snapshot, &mut entry) };
    let mut found = Vec::new();
    loop {
        match step {
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => return Err(io::Error::other(e)),
            Ok(()) => {}
        }
        let pid = entry.th32ProcessID;
        if let Some(image) = crate::process_image(pid)
            && check.accepts(Some(&image))
        {
            // Query again after opening the handle: it now pins the process identity for all later waits.
            if let Ok(raw) = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
            } {
                let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
                if crate::process_image(pid)
                    .as_deref()
                    .is_some_and(|p| check.accepts(Some(p)))
                {
                    let mut session = 0;
                    unsafe { ProcessIdToSessionId(pid, &mut session) }.map_err(io::Error::other)?;
                    found.push(RunningImage {
                        pid,
                        session,
                        image,
                        handle,
                    });
                }
            }
        }
        step = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    Ok(found)
}

/// A private file held open without sharing writes. Delete-on-close avoids a close/delete race with
/// a new holder. The explicit DACL also protects discovery if parent ACLs are more permissive.
pub(crate) fn private_file(path: &Path, share: FILE_SHARE_MODE) -> io::Result<File> {
    let descriptor = crate::win::UserOnlyDescriptor::new()?;
    let attrs = descriptor.attributes();
    let handle = unsafe {
        CreateFileW(
            &HSTRING::from(path.as_os_str()),
            GENERIC_READ.0 | GENERIC_WRITE.0 | DELETE.0,
            share,
            Some(&attrs),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_DELETE_ON_CLOSE,
            None,
        )
    }
    .map_err(|e| io::Error::from_raw_os_error((e.code().0 as u32 & 0xffff) as i32))?;
    // SAFETY: ownership of this newly created handle transfers to File exactly once.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

/// A process-lifetime lock in this user's protected channel folder, shared across Windows sessions.
/// No globally named mutex is involved, so another account cannot pre-create its name.
pub struct InstanceLock {
    file: Option<File>,
    directory: PathBuf,
}

impl InstanceLock {
    pub fn acquire(names: &Names, scope: &str) -> io::Result<Option<Self>> {
        Self::acquire_in(names.data_dir().map_err(io::Error::other)?, scope)
    }

    /// Outside the data folder so uninstall --purge can hold the lock throughout deletion.
    pub fn maintenance(names: &Names) -> io::Result<Option<Self>> {
        Self::acquire_in(
            agentbolt_core::channel::env_dir("LOCALAPPDATA")
                .map_err(io::Error::other)?
                .join("Agentbolt-maintenance"),
            &names.channel,
        )
    }

    /// Stops any newly launched supervisor/worker from touching the store during replacement or
    /// rollback. Separate from the installer mutex, which remains held while the committed build starts.
    pub fn startup(names: &Names) -> io::Result<Option<Self>> {
        Self::acquire_in(
            agentbolt_core::channel::env_dir("LOCALAPPDATA")
                .map_err(io::Error::other)?
                .join("Agentbolt-startup"),
            &names.channel,
        )
    }

    fn acquire_in(directory: PathBuf, scope: &str) -> io::Result<Option<Self>> {
        if scope.is_empty()
            || !scope
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::other("invalid instance lock scope"));
        }
        std::fs::create_dir_all(&directory)?;
        match private_file(&directory.join(format!("{scope}.lock")), FILE_SHARE_MODE(0)) {
            Ok(file) => Ok(Some(Self {
                file: Some(file),
                directory,
            })),
            Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION.0 as i32) => Ok(None),
            Err(e) => Err(e), // Access denied is a fault, never evidence of a healthy existing daemon.
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_dir(&self.directory); // Only an empty test/unused folder; never recursive.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_discovery_matches_an_image_and_retains_its_identity() {
        let me = std::env::current_exe().unwrap();
        let found = running_images(std::slice::from_ref(&me)).unwrap();
        let process = found.iter().find(|p| p.pid == std::process::id()).unwrap();
        assert!(!process.exited().unwrap());
        assert!(crate::PeerCheck::Image(vec![me.clone()]).accepts(Some(&process.image)));
        assert!(
            running_images(&[me.with_file_name("not-an-agentbolt-test.exe")])
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn system_programs_are_absolute_and_do_not_use_the_application_folder() {
        for path in [schtasks().unwrap(), explorer().unwrap()] {
            assert!(path.is_absolute() && path.is_file(), "{}", path.display());
            assert_ne!(path.parent(), std::env::current_exe().unwrap().parent());
        }
    }
    #[test]
    fn instance_locks_release_and_scopes_do_not_interfere() {
        let names = Names::for_channel(&format!("locktest-{}", std::process::id())).unwrap();
        let first = InstanceLock::acquire(&names, "daemon").unwrap().unwrap();
        assert!(InstanceLock::acquire(&names, "daemon").unwrap().is_none());
        let supervisor = InstanceLock::acquire(&names, "supervisor")
            .unwrap()
            .unwrap();
        drop(first);
        assert!(InstanceLock::acquire(&names, "daemon").unwrap().is_some());
        drop(supervisor);
        assert!(!names.data_dir().unwrap().exists());
    }
}
