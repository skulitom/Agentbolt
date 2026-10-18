//! Windows-only start-up pieces: a per-user instance lock and the `agentbolt://` handler-key check.

use std::path::Path;

use agentbolt_core::channel::Names;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::HSTRING;

pub use agentbolt_ipc::windows_util::InstanceLock as Instance;

/// Ok(Some) = we are the only daemon of this channel; Ok(None) = another one holds it (in any session).
pub fn single_instance(names: &Names) -> Result<Option<Instance>, String> {
    Instance::acquire(names, &names.mutex).map_err(|e| format!("single-instance lock: {e}"))
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
            if handler_command_matches(&command, &expected, &names.channel) {
                HandlerKey::Ours
            } else {
                HandlerKey::Changed(command)
            }
        }
    }
}

fn read_default_value(subkey: &str) -> Option<String> {
    let subkey = HSTRING::from(subkey);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            None,
            flags,
            None,
            None,
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            None,
            flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Accept only the installer's complete handler command, including this channel and the URL placeholder.
/// Checking the program alone could accept a registration with wrong or injected arguments.
fn handler_command_matches(
    command: &str,
    expected: &agentbolt_ipc::PeerCheck,
    channel: &str,
) -> bool {
    let Some((program, args)) = command
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.split_once('"'))
    else {
        return false;
    };
    !program.is_empty()
        && expected.accepts(Some(Path::new(program)))
        && args == format!(" --channel {channel} --url \"%1\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn handler_requires_the_correct_program_channel_and_url_arguments() {
        let expected = agentbolt_ipc::PeerCheck::Image(vec![PathBuf::from(
            r"C:\Program Files\Agentbolt\agentbolt-desk.exe",
        )]);
        let good = r#""C:\Program Files\Agentbolt\agentbolt-desk.exe" --channel t --url "%1""#;
        assert!(handler_command_matches(good, &expected, "t"));
        for bad in [
            good.replace("agentbolt-desk.exe", "other.exe"),
            good.replace("--channel t", "--channel main"),
            good.replace(" --url", ""),
            good.replace("%1", "%2"),
            format!("{good} --headless"),
            "\"\" --channel t --url \"%1\"".into(),
            "\"C:\\Program Files\\Agentbolt\\agentbolt-desk.exe".into(),
            String::new(),
        ] {
            assert!(!handler_command_matches(&bad, &expected, "t"), "{bad}");
        }
    }

    #[test]
    fn a_test_channel_has_no_handler_key() {
        let names = Names::for_channel("ipctest-nokey").unwrap();
        assert_eq!(handler_key(&names), HandlerKey::Missing);
    }

    #[test]
    fn a_second_holder_of_the_instance_lock_is_turned_away() {
        let names = crate::testutil::channel();
        // Even the exact old global mutex name can be occupied without denying this user's startup.
        let legacy = unsafe {
            windows::Win32::System::Threading::CreateMutexW(
                None,
                false,
                &HSTRING::from(format!(r"Global\{}", names.mutex)),
            )
        }
        .unwrap();
        let first = single_instance(&names).unwrap();
        assert!(first.is_some());
        assert!(single_instance(&names).unwrap().is_none());
        unsafe {
            windows::Win32::Foundation::CloseHandle(legacy).unwrap();
        }
    }
}
