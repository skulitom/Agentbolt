//! Windows warm Desk support. The daemon owns the process; Close hides its window.
//! eframe 0.37 unconditionally shows the root after its first frame (#8459). Suppress that
//! native show while preloading, then use a native message to reveal it even when no egui
//! paint is pending (#5229). Actual visibility gates all Inbox reads.

#[cfg(windows)]
mod imp {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetPropW, IsWindowVisible, PostMessageW, STYLESTRUCT, SW_HIDE, SW_RESTORE,
        SWP_SHOWWINDOW, SetForegroundWindow, SetPropW, ShowWindow, WINDOWPOS, WM_APP, WM_CLOSE,
        WM_STYLECHANGING, WM_WINDOWPOSCHANGING, WS_VISIBLE,
    };
    use windows::core::w;

    // Only opens/hides the window; this message cannot read or write an answer.
    const SHOW: u32 = WM_APP + 0x41;
    static HIDDEN: AtomicBool = AtomicBool::new(false);
    static WINDOW: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn subclass(
        hwnd: HWND,
        msg: u32,
        w: WPARAM,
        l: LPARAM,
        _id: usize,
        resident: usize,
    ) -> LRESULT {
        if msg == SHOW {
            HIDDEN.store(false, Ordering::SeqCst);
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            return LRESULT(0);
        }
        if resident != 0 && msg == WM_CLOSE {
            HIDDEN.store(true, Ordering::SeqCst);
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            return LRESULT(0);
        }
        if msg == WM_WINDOWPOSCHANGING && HIDDEN.load(Ordering::SeqCst) && l.0 != 0 {
            // Windows supplies a mutable WINDOWPOS for exactly this message and callback lifetime.
            let pos = unsafe { &mut *(l.0 as *mut WINDOWPOS) };
            pos.flags &= !SWP_SHOWWINDOW;
        }
        if msg == WM_STYLECHANGING
            && w.0 as i32 == GWL_STYLE.0
            && HIDDEN.load(Ordering::SeqCst)
            && l.0 != 0
        {
            // winit also writes WS_VISIBLE directly after ShowWindow. Blocking only the
            // SHOWWINDOW positioning flag would leave that second path visible.
            let style = unsafe { &mut *(l.0 as *mut STYLESTRUCT) };
            style.styleNew &= !WS_VISIBLE.0;
        }
        unsafe { DefSubclassProc(hwnd, msg, w, l) }
    }

    pub fn prepare(
        cc: &eframe::CreationContext<'_>,
        warm: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let RawWindowHandle::Win32(raw) = cc.window_handle()?.as_raw() else {
            return Err("the Desk has no Windows handle".into());
        };
        let hwnd = HWND(raw.hwnd.get() as *mut _);
        HIDDEN.store(warm, Ordering::SeqCst);
        WINDOW.store(raw.hwnd.get(), Ordering::SeqCst);
        if !unsafe { SetWindowSubclass(hwnd, Some(subclass), 1, usize::from(warm)) }.as_bool() {
            return Err("couldn't initialize the Desk window controller".into());
        }
        unsafe {
            SetPropW(
                hwnd,
                w!("AgentboltDeskReady"),
                Some(HANDLE(std::ptr::dangling_mut())),
            )?;
        }
        Ok(())
    }

    pub fn show(hwnd: HWND) -> bool {
        if unsafe { GetPropW(hwnd, w!("AgentboltDeskReady")) }.is_invalid() {
            return false;
        }
        // Posting to the window's thread works while hidden, without relying on a render callback.
        unsafe { PostMessageW(Some(hwnd), SHOW, WPARAM(0), LPARAM(0)).is_ok() }
    }

    pub fn visible() -> bool {
        let hwnd = WINDOW.load(Ordering::SeqCst);
        hwnd != 0
            && !HIDDEN.load(Ordering::SeqCst)
            && unsafe { IsWindowVisible(HWND(hwnd as *mut _)) }.as_bool()
    }
}

#[cfg(windows)]
pub use imp::{prepare, show, visible};

#[cfg(not(windows))]
pub fn prepare(
    _cc: &eframe::CreationContext<'_>,
    _warm: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Ok(())
}

#[cfg(not(windows))]
pub fn visible() -> bool {
    true
}
