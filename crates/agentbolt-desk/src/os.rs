//! What the Desk asks of the OS: open an allowed link, show a path in Explorer, start the daemon's task,
//! and a message box for the protocol handler (which has no window of its own).
//!
//! Callers pass only links that `view::classify` allowed.

use agentbolt_core::channel::Names;

#[cfg(windows)]
pub type WindowInstance = agentbolt_ipc::windows_util::InstanceLock;
#[cfg(not(windows))]
pub type WindowInstance = ();

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{
        MB_ICONWARNING, MB_OK, MessageBoxW, SW_SHOWNORMAL,
    };
    use windows::core::{HSTRING, w};

    /// Open an https:// or claude:// link with its registered handler.
    pub fn open_url(url: &str) -> Result<(), String> {
        let r = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(url),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        // ShellExecute returns a value greater than 32 on success.
        if r.0 as isize > 32 {
            Ok(())
        } else {
            Err(format!(
                "Windows couldn't open {url} (code {})",
                r.0 as isize
            ))
        }
    }

    /// `explorer /select,"<path>"`: shows the file in its folder, never runs it.
    pub fn show_in_explorer(path: &Path) -> Result<(), String> {
        Command::new(agentbolt_ipc::windows_util::explorer().map_err(|e| e.to_string())?)
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("couldn't start Explorer: {e}"))
    }

    pub fn message(title: &str, text: &str) {
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(text),
                &HSTRING::from(title),
                MB_OK | MB_ICONWARNING,
            );
        }
    }

    /// One Desk per user, channel and Windows session. The caller holds the lock for the window's life.
    pub fn first_window(
        names: &agentbolt_core::channel::Names,
        title: &str,
        activate: bool,
    ) -> Result<Option<agentbolt_ipc::windows_util::InstanceLock>, String> {
        use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
        let lock = agentbolt_ipc::windows_util::InstanceLock::acquire(
            names,
            &format!("desk-{}", crate::drafts::session_id()),
        )
        .map_err(|e| format!("Desk instance lock: {e}"))?;
        if lock.is_some() {
            return Ok(lock);
        }
        if activate {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(10) {
                if let Ok(hwnd) = unsafe { FindWindowW(None, &HSTRING::from(title)) }
                    && crate::resident::show(hwnd)
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        Ok(None)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    pub fn open_url(url: &str) -> Result<(), String> {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn show_in_explorer(path: &Path) -> Result<(), String> {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn message(title: &str, text: &str) {
        eprintln!("{title}: {text}");
    }

    pub fn first_window(
        _names: &agentbolt_core::channel::Names,
        _title: &str,
        _activate: bool,
    ) -> Result<Option<()>, String> {
        Ok(Some(()))
    }
}

pub use imp::{first_window, message, open_url, show_in_explorer};

/// Match the installed Start-menu shortcut so Windows groups the running Desk under Agentbolt.
pub fn set_app_identity(names: &Names) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
            &windows::core::HSTRING::from(&names.aumid),
        )
        .map_err(|e| format!("Couldn't set Agentbolt's window identity: {e}"))?;
    }
    #[cfg(not(windows))]
    let _ = names;
    Ok(())
}

/// Observe actual Win32 visibility, independently of egui's requested viewport visibility.
pub fn trace_window_shown(title: &str) {
    if !crate::startup::enabled() {
        return;
    }
    #[cfg(windows)]
    {
        let title = title.to_owned();
        std::thread::spawn(move || {
            use windows::Win32::UI::WindowsAndMessaging::{
                FindWindowW, GetWindowThreadProcessId, IsWindowVisible,
            };
            use windows::core::HSTRING;
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(30) {
                if let Ok(hwnd) = unsafe { FindWindowW(None, &HSTRING::from(&title)) } {
                    let mut pid = 0;
                    unsafe {
                        GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    }
                    if pid == std::process::id() && unsafe { IsWindowVisible(hwnd) }.as_bool() {
                        crate::startup::mark("window_shown");
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
    }
    #[cfg(not(windows))]
    let _ = title;
}

pub fn run_task(names: &Names) -> Result<(), String> {
    #[cfg(windows)]
    return agentbolt_ipc::windows_task::run(names, None);
    #[cfg(not(windows))]
    {
        let _ = names;
        Err("the sign-in task exists only on Windows".into())
    }
}

pub fn open_chat(
    target: &agentbolt_core::navigation::ChatTarget,
    cwd: Option<&str>,
) -> Result<(), String> {
    use agentbolt_core::navigation::ChatTarget;
    if !agentbolt_core::navigation::valid_session_id(target.id()) {
        return Err("The host supplied an invalid session id.".into());
    }
    match target {
        ChatTarget::ClaudeDesktop(_) => return open_url(&target.url().ok_or("Invalid chat link")?),
        ChatTarget::Codex(_) => {
            // The installed Codex app registers codex://threads/<id>. A CLI-only machine can resume in wt.
            if open_url(&target.url().ok_or("Invalid chat link")?).is_ok() {
                return Ok(());
            }
        }
        ChatTarget::ClaudeTerminal(_) => {}
    }
    let home =
        std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    let args = crate::chat::terminal_args(target, cwd, &home)?;
    #[cfg(windows)]
    return std::process::Command::new(
        agentbolt_core::channel::env_dir("LOCALAPPDATA")?.join("Microsoft/WindowsApps/wt.exe"),
    )
    .args(args)
    .spawn()
    .map(|_| ())
    .map_err(|e| format!("Couldn't start Windows Terminal: {e}"));
    #[cfg(not(windows))]
    {
        let _ = args;
        Err("Terminal resume from the Desk is available on Windows.".into())
    }
}
