//! Windows paths and coordination shared by the installer, daemon and Desk.

use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::FromRawHandle;
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
        if scope.is_empty()
            || !scope
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::other("invalid instance lock scope"));
        }
        let directory = names.data_dir().map_err(io::Error::other)?;
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
