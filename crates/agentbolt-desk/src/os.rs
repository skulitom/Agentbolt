//! What the Desk asks of the OS: open an allowed link, show a path in Explorer, start the daemon's task,
//! and a message box for the protocol handler (which has no window of its own).
//!
//! Callers pass only links that `view::classify` allowed.

use agentbolt_core::channel::Names;

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;

    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW, SW_SHOWNORMAL};
    use windows::core::{HSTRING, w};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Open an https:// or claude:// link with its registered handler.
    pub fn open_url(url: &str) -> Result<(), String> {
        let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL) };
        // ShellExecute returns a value greater than 32 on success.
        if r.0 as isize > 32 { Ok(()) } else { Err(format!("Windows couldn't open {url} (code {})", r.0 as isize)) }
    }

    /// `explorer /select,"<path>"`: shows the file in its folder, never runs it.
    pub fn show_in_explorer(path: &Path) -> Result<(), String> {
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("couldn't start Explorer: {e}"))
    }

    pub fn run_task(task: &str) -> Result<(), String> {
        let out = Command::new("schtasks.exe")
            .args(["/Run", "/TN", task])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("couldn't run schtasks: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(format!("Task Scheduler couldn't start '{task}': {err}"))
        }
    }

    pub fn message(title: &str, text: &str) {
        unsafe {
            MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONWARNING);
        }
    }

    /// One Desk window per channel: a second launch brings the first window forward and exits.
    /// The mutex handle is never closed, so it lasts as long as the process.
    pub fn first_window(mutex: &str, title: &str) -> bool {
        use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow};
        let name = HSTRING::from(format!("Local\\{mutex}"));
        let Ok(handle) = (unsafe { CreateMutexW(None, false, &name) }) else { return true };
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            let _ = handle; // a raw HANDLE: never closed here, so the mutex lives as long as the window
            return true;
        }
        if let Ok(hwnd) = unsafe { FindWindowW(None, &HSTRING::from(title)) } {
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
        false
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    pub fn open_url(url: &str) -> Result<(), String> {
        std::process::Command::new("xdg-open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn show_in_explorer(path: &Path) -> Result<(), String> {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open").arg(dir).spawn().map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn run_task(_task: &str) -> Result<(), String> {
        Err("the sign-in task exists only on Windows".into())
    }

    pub fn message(title: &str, text: &str) {
        eprintln!("{title}: {text}");
    }

    pub fn first_window(_mutex: &str, _title: &str) -> bool {
        true
    }
}

pub use imp::{first_window, message, open_url, show_in_explorer};

pub fn run_task(names: &Names) -> Result<(), String> {
    imp::run_task(&names.task)
}
