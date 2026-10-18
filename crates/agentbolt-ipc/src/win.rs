//! Windows named pipes: a user-only DACL, no remote clients, the first-instance flag, and image checks on both
//! ends (`GetNamedPipeServerProcessId` / `GetNamedPipeClientProcessId` + `QueryFullProcessImageNameW`).

use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::core::{BOOL, HSTRING, PWSTR};

use crate::IpcError;

pub(crate) const IMAGE_CHECKS: bool = true;

pub(crate) type ClientStream = NamedPipeClient;

pub(crate) fn pipe_path(_names: &Names, name: &str) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\{name}"))
}

// ---- the security descriptor ---------------------------------------------------------------------------------

/// A self-relative security descriptor from `ConvertStringSecurityDescriptorToSecurityDescriptorW`, freed on drop.
pub(crate) struct UserOnlyDescriptor(PSECURITY_DESCRIPTOR);

// The descriptor is immutable after creation and only read by CreateNamedPipeW.
unsafe impl Send for UserOnlyDescriptor {}
unsafe impl Sync for UserOnlyDescriptor {}

impl UserOnlyDescriptor {
    /// `D:P(A;;GA;;;<user SID>)`: a protected DACL with one ACE, full access for the current user. No Everyone,
    /// no anonymous, no inherited ACEs; SYSTEM and Administrators are not granted anything either.
    pub(crate) fn new() -> io::Result<Self> {
        let sid = current_user_sid()?;
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None) }
            .map_err(io::Error::other)?;
        Ok(Self(sd))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: BOOL(0),
        }
    }
}

impl Drop for UserOnlyDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

/// The current process token's user SID as `S-1-5-21-...`.
pub(crate) fn current_user_sid() -> io::Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(io::Error::other)?;
        let token = OwnedHandle(token);
        let mut len = 0u32;
        // The first call reports the size (and fails with ERROR_INSUFFICIENT_BUFFER).
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut len);
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        // u64 storage keeps TOKEN_USER suitably aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len)
            .map_err(io::Error::other)?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(io::Error::other)?;
        let sid = text.to_string().map_err(io::Error::other);
        LocalFree(Some(HLOCAL(text.0 as *mut c_void)));
        sid
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// ---- server ---------------------------------------------------------------------------------------------------

pub(crate) enum AcceptError {
    /// One client's connection failed; keep listening.
    Transient(io::Error),
    /// The listener can't make a new pipe instance.
    Fatal(io::Error),
}

pub(crate) struct Listener {
    path: PathBuf,
    descriptor: UserOnlyDescriptor,
    /// The instance waiting for the next client.
    next: NamedPipeServer,
}

impl Listener {
    pub(crate) fn bind(names: &Names, name: &str) -> Result<Self, IpcError> {
        let path = pipe_path(names, name);
        let descriptor = UserOnlyDescriptor::new()?;
        let next = create_instance(&path, &descriptor, true).map_err(|e| {
            if e.raw_os_error() == Some(ERROR_ACCESS_DENIED.0 as i32) {
                // FILE_FLAG_FIRST_PIPE_INSTANCE fails with access denied when the name already exists.
                IpcError::AlreadyServed(path.display().to_string())
            } else {
                IpcError::Io(e)
            }
        })?;
        Ok(Self { path, descriptor, next })
    }

    /// Wait for a client; returns its stream and process id.
    pub(crate) async fn accept(&mut self) -> Result<(NamedPipeServer, Option<u32>), AcceptError> {
        let connected = self.next.connect().await;
        // Always keep one instance listening, whatever happened to this one.
        let fresh = create_instance(&self.path, &self.descriptor, false).map_err(AcceptError::Fatal)?;
        let stream = std::mem::replace(&mut self.next, fresh);
        connected.map_err(AcceptError::Transient)?;
        let mut pid = 0u32;
        let pid = unsafe { GetNamedPipeClientProcessId(HANDLE(stream.as_raw_handle()), &mut pid) }
            .ok()
            .map(|_| pid);
        Ok((stream, pid))
    }
}

fn create_instance(path: &std::path::Path, sd: &UserOnlyDescriptor, first: bool) -> io::Result<NamedPipeServer> {
    let mut attrs = sd.attributes();
    let mut options = ServerOptions::new();
    options.first_pipe_instance(first).reject_remote_clients(true);
    // SAFETY: attrs is a valid SECURITY_ATTRIBUTES whose descriptor lives as long as the listener.
    unsafe { options.create_with_security_attributes_raw(path, (&mut attrs as *mut SECURITY_ATTRIBUTES).cast()) }
}

/// Close a refused client's instance without reading from it.
pub(crate) fn drop_client(stream: NamedPipeServer) {
    let _ = stream.disconnect();
    drop(stream);
}

// ---- client ---------------------------------------------------------------------------------------------------

/// Open the pipe, waiting briefly while every instance is busy. Returns the stream and the server's pid.
pub(crate) async fn connect(names: &Names, name: &str) -> Result<(NamedPipeClient, Option<u32>), IpcError> {
    let path = pipe_path(names, name);
    let deadline = Instant::now() + Duration::from_secs(5);
    let stream = loop {
        // SECURITY_IDENTIFICATION (tokio's default): a server may identify us but never impersonate us.
        match ClientOptions::new().open(&path) {
            Ok(stream) => break stream,
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND.0 as i32) => {
                return Err(IpcError::NotRunning(names.channel.clone()));
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Err(IpcError::Io(e)),
        }
    };
    let mut pid = 0u32;
    let pid = unsafe { GetNamedPipeServerProcessId(HANDLE(stream.as_raw_handle()), &mut pid) }
        .ok()
        .map(|_| pid);
    Ok((stream, pid))
}

// ---- images ---------------------------------------------------------------------------------------------------

pub(crate) fn process_image(pid: u32) -> Option<PathBuf> {
    unsafe {
        let process = OwnedHandle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let mut buf = vec![0u16; 1024];
        loop {
            let mut len = buf.len() as u32;
            match QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) {
                Ok(()) => {
                    use std::os::windows::ffi::OsStringExt;
                    return Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize])));
                }
                Err(_) if buf.len() < 32 * 1024 => buf.resize(buf.len() * 4, 0),
                Err(_) => return None,
            }
        }
    }
}

/// The DACL of a pipe handle as SDDL (tests prove it is user-only).
#[cfg(test)]
pub(crate) fn dacl_sddl(handle: std::os::windows::io::RawHandle) -> io::Result<String> {
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT,
    };
    use windows::Win32::Security::DACL_SECURITY_INFORMATION;
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let status = GetSecurityInfo(
            HANDLE(handle),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&mut sd),
        );
        if status.is_err() {
            return Err(io::Error::from_raw_os_error(status.0 as i32));
        }
        let mut text = PWSTR::null();
        let converted = ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        );
        LocalFree(Some(HLOCAL(sd.0)));
        converted.map_err(io::Error::other)?;
        let s = text.to_string().map_err(io::Error::other);
        LocalFree(Some(HLOCAL(text.0 as *mut c_void)));
        s
    }
}
