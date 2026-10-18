//! Windows-only start-up pieces: the `Global\` single-instance mutex and the `agentbolt://` handler-key check.

use std::path::{Path, PathBuf};

use agentbolt_core::channel::Names;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::HSTRING;

/// The held mutex. Never closed: it goes with the process.
pub struct Instance(#[allow(dead_code)] HANDLE);

/// Ok(Some) = we are the only daemon of this channel; Ok(None) = another one holds it (in any session).
pub fn single_instance(names: &Names) -> Result<Option<Instance>, String> {
    let name = HSTRING::from(format!(r"Global\{}", names.mutex));
    match unsafe { CreateMutexW(None, true, &name) } {
        Ok(handle) => {
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                Ok(None)
            } else {
                Ok(Some(Instance(handle)))
            }
        }
        // A mutex of that name made under another security context.
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => Ok(None),
        Err(e) => Err(format!("single-instance mutex: {e}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerKey {
    /// Points to `agentbolt-desk.exe` next to the daemon.
    Ours,
    /// No `<protocol>\shell\open\command` key (not installed on this channel).
    Missing,
    /// Points somewhere else: the command found.
    Changed(String),
}

/// Read `HKCU\Software\Classes\<protocol>\shell\open\command` and compare its program with the Desk next to us.
pub fn handler_key(names: &Names) -> HandlerKey {
    let key = format!(r"Software\Classes\{}\shell\open\command", names.protocol);
    match read_default_value(&key) {
        None => HandlerKey::Missing,
        Some(command) => {
            let expected = agentbolt_ipc::PeerCheck::sibling(agentbolt_ipc::DESK_EXE);
            match command_program(&command) {
                Some(program) if expected.accepts(Some(Path::new(&program))) => HandlerKey::Ours,
                _ => HandlerKey::Changed(command),
            }
        }
    }
}

fn read_default_value(subkey: &str) -> Option<String> {
    let subkey = HSTRING::from(subkey);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut size = 0u32;
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, &subkey, None, flags, None, None, Some(&mut size)) };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &subkey, None, flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The program of a shell command line: the quoted first token, or everything up to the first space.
pub fn command_program(command: &str) -> Option<PathBuf> {
    let command = command.trim_start();
    let program = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        command.split_whitespace().next()?
    };
    (!program.is_empty()).then(|| PathBuf::from(program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_is_read_from_quoted_and_bare_commands() {
        assert_eq!(
            command_program(r#""C:\Program Files\Agentbolt\agentbolt-desk.exe" --channel t "%1""#),
            Some(PathBuf::from(r"C:\Program Files\Agentbolt\agentbolt-desk.exe"))
        );
        assert_eq!(command_program(r"C:\x\agentbolt-desk.exe %1"), Some(PathBuf::from(r"C:\x\agentbolt-desk.exe")));
        assert_eq!(command_program("   "), None);
        assert_eq!(command_program(r#""""#), None);
    }

    #[test]
    fn a_test_channel_has_no_handler_key() {
        let names = Names::for_channel("ipctest-nokey").unwrap();
        assert_eq!(handler_key(&names), HandlerKey::Missing);
    }

    #[test]
    fn a_second_holder_of_the_mutex_is_turned_away() {
        let names = crate::testutil::channel();
        let first = single_instance(&names).unwrap();
        assert!(first.is_some());
        // CreateMutexW on a name this process already holds reports ERROR_ALREADY_EXISTS, as for another process.
        assert!(single_instance(&names).unwrap().is_none());
    }
}
