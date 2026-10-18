//! The tray icon on the main thread with a plain Win32 message loop (PLAN.md: tray-icon 0.26 needs a running
//! loop before a tray is created, and a window that hides into the tray is what broke egui).
//!
//! Left click opens the Desk. The menu has Open Desk, Pause notifications (`Settings.toasts_off`, through the
//! service) and Quit Agentbolt. The tooltip carries the Inbox count, and the icon a count badge.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use agentbolt_core::channel::Names;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_NOREMOVE, PM_REMOVE, PeekMessageW,
    QS_ALLINPUT, TranslateMessage, WM_QUIT,
};

use crate::daemon::{Shared, Ui};
use crate::icon;
use crate::log::{info, warning};
use crate::winutil::HandlerKey;

pub enum UiCmd {
    Counts(u32, u32),
    Paused(bool),
}

/// The effects task's handle on the tray: commands queued for the main thread (it polls every 200 ms).
pub struct TrayUi(pub std::sync::Mutex<Sender<UiCmd>>);

impl Ui for TrayUi {
    fn counts(&self, open: u32, digest: u32) {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(UiCmd::Counts(open, digest));
        }
    }
    fn paused(&self, paused: bool) {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(UiCmd::Paused(paused));
        }
    }
}

pub struct TrayState {
    pub open: u32,
    pub digest: u32,
    pub paused: bool,
    pub handler: HandlerKey,
}

pub fn tooltip(names: &Names, s: &TrayState) -> String {
    let mut t = if names.is_main() {
        "Agentbolt".to_string()
    } else {
        format!("Agentbolt ({})", names.channel)
    };
    t.push_str(&format!(": {} waiting", s.open));
    if s.digest > 0 {
        t.push_str(&format!(", {} in the digest", s.digest));
    }
    if s.paused {
        t.push_str(", paused");
    }
    match s.handler {
        HandlerKey::Ours => {}
        HandlerKey::Missing => t.push_str(" - handler missing"),
        HandlerKey::Changed(_) => t.push_str(" - handler changed"),
    }
    t
}

fn make_icon(open: u32) -> Option<Icon> {
    Icon::from_rgba(icon::rgba(Some(open)), icon::SIZE as u32, icon::SIZE as u32).ok()
}

/// Start `agentbolt-desk.exe` from the daemon's folder (with `--channel` off the main channel).
pub fn launch_desk(names: &Names) {
    let exe = agentbolt_ipc::sibling_paths(agentbolt_ipc::DESK_EXE)
        .into_iter()
        .find(|p| p.is_file());
    let Some(exe) = exe else {
        warning!("open desk: agentbolt-desk.exe isn't next to the daemon");
        return;
    };
    let mut cmd = Command::new(&exe);
    if !names.is_main() {
        cmd.args(["--channel", &names.channel]);
    }
    match cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => info!("opened the desk"),
        Err(e) => warning!("open desk: {e}"),
    }
}

/// Run the tray until Quit or `shutdown`. Returns when the daemon should stop.
pub fn run(
    shared: Arc<Shared>,
    rx: Receiver<UiCmd>,
    shutdown: Arc<AtomicBool>,
    mut state: TrayState,
) -> Result<(), String> {
    let names = shared.names.clone();
    let mut msg = MSG::default();
    // Make sure this thread has a message queue before the tray creates its window.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    let menu = Menu::new();
    let open_item = MenuItem::new("Open Desk", true, None);
    let pause_item = CheckMenuItem::new("Pause notifications", true, state.paused, None);
    let quit_item = MenuItem::new("Quit Agentbolt", true, None);
    menu.append_items(&[
        &open_item,
        &pause_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])
    .map_err(|e| format!("tray menu: {e}"))?;
    let mut builder = TrayIconBuilder::new()
        .with_tooltip(tooltip(&names, &state))
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false);
    if let Some(icon) = make_icon(state.open) {
        builder = builder.with_icon(icon);
    }
    let tray: TrayIcon = builder.build().map_err(|e| format!("tray icon: {e}"))?;
    info!("tray icon shown");
    let mut warm_desk = crate::warm::WarmDesk::new(names.clone());

    loop {
        warm_desk.tick();
        unsafe {
            MsgWaitForMultipleObjects(None, false, 200, QS_ALLINPUT);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return Ok(());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                launch_desk(&names);
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == *open_item.id() {
                launch_desk(&names);
            } else if event.id == *pause_item.id() {
                match shared.toggle_pause() {
                    Ok(paused) => {
                        info!(
                            "notifications {}",
                            if paused { "paused" } else { "resumed" }
                        );
                        state.paused = paused;
                    }
                    Err(e) => warning!("pause notifications: {e}"),
                }
                pause_item.set_checked(state.paused);
                let _ = tray.set_tooltip(Some(tooltip(&names, &state)));
            } else if event.id == *quit_item.id() {
                info!("quit from the tray menu");
                return Ok(());
            }
        }
        let mut changed = false;
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                UiCmd::Counts(open, digest) => {
                    if open != state.open {
                        let _ = tray.set_icon(make_icon(open));
                    }
                    state.open = open;
                    state.digest = digest;
                }
                UiCmd::Paused(paused) => {
                    state.paused = paused;
                    pause_item.set_checked(paused);
                }
            }
            changed = true;
        }
        if changed {
            let _ = tray.set_tooltip(Some(tooltip(&names, &state)));
        }
        if shutdown.load(Ordering::SeqCst) {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_says_the_count_and_flags_the_handler() {
        let main = Names::for_channel("main").unwrap();
        let test = Names::for_channel("ipctest-tip").unwrap();
        let mut s = TrayState {
            open: 2,
            digest: 0,
            paused: false,
            handler: HandlerKey::Ours,
        };
        assert_eq!(tooltip(&main, &s), "Agentbolt: 2 waiting");
        s.digest = 3;
        s.paused = true;
        s.handler = HandlerKey::Changed("evil.exe %1".into());
        assert_eq!(
            tooltip(&test, &s),
            "Agentbolt (ipctest-tip): 2 waiting, 3 in the digest, paused - handler changed"
        );
        s.handler = HandlerKey::Missing;
        assert!(tooltip(&main, &s).ends_with("handler missing"));
        assert!(
            tooltip(&test, &s).chars().count() < 128,
            "fits the tray's tooltip limit"
        );
    }
}
