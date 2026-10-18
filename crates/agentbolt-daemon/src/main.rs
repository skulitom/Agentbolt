#![cfg_attr(windows, windows_subsystem = "windows")]
//! agentbolt-daemon: the only writer of the store. It serves the MCP pipe and the Desk pipe, ticks the service,
//! and owns the tray icon, toasts, Codex delivery and the ntfy count push. GUI subsystem, no window, no console.
//!
//! `agentbolt-daemon [--channel <name>] [--headless]`
//! - `--channel` overrides `AGENTBOLT_CHANNEL` (the sign-in task passes it; Task Scheduler can't set env).
//! - `--headless`: no tray icon and no toasts (tests; sessions with no taskbar). Pipes, ticks, Codex delivery and
//!   the ntfy push run as usual.
//!
//! Exit codes: 0 = stopped (or another daemon of the channel already runs), 1 = couldn't start or a pipe
//! failed (the sign-in task restarts it), 2 = bad arguments.

mod codex;
mod daemon;
mod delivery;
// The tray icon and toast XML are drawn and built everywhere (and tested on Linux CI), but shown only on Windows.
#[cfg_attr(not(windows), allow(dead_code))]
mod icon;
mod log;
mod ntfy;
mod store;
#[cfg_attr(not(windows), allow(dead_code))]
mod toast;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod warm;
#[cfg(windows)]
mod winutil;

#[cfg(test)]
mod testutil;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agentbolt_core::channel::Names;
use agentbolt_ipc::{IpcError, Pipe, Server};

use crate::daemon::{DeskHandler, McpHandler, NoUi, Shared, Ui};
use crate::log::{error, info, warning};

#[derive(Debug, PartialEq, Eq)]
struct Args {
    channel: Option<String>,
    headless: bool,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut out = Args {
        channel: None,
        headless: false,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--channel" => out.channel = Some(it.next().ok_or("--channel needs a name")?),
            "--headless" => out.headless = true,
            other if other.starts_with("--channel=") => {
                out.channel = other.strip_prefix("--channel=").map(str::to_string);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(out)
}

fn names_for(args: &Args) -> Result<Names, String> {
    match &args.channel {
        Some(channel) => Names::for_channel(channel.trim()),
        None => Names::from_env(),
    }
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let Ok(args) = parse_args(std::env::args().skip(1)) else {
        return 2;
    };
    let Ok(names) = names_for(&args) else {
        return 2;
    };
    // No data folder (LOCALAPPDATA unset): nowhere to log or keep the store, so stop rather than use ".".
    let Ok(data_dir) = names.data_dir() else {
        return 1;
    };
    if std::fs::create_dir_all(&data_dir).is_err() {
        return 1;
    }
    log::init(data_dir.join("agentbolt.log"));

    #[cfg(windows)]
    let _instance = match winutil::single_instance(&names) {
        Ok(Some(instance)) => instance,
        Ok(None) => {
            info!(
                "another agentbolt-daemon already runs on channel {}; exiting",
                names.channel
            );
            return 0;
        }
        Err(e) => {
            error!("{e}");
            return 1;
        }
    };
    info!(
        "agentbolt-daemon {} starting: channel {}, pid {}, {}",
        env!("CARGO_PKG_VERSION"),
        names.channel,
        std::process::id(),
        if args.headless {
            "headless"
        } else {
            "tray and toasts"
        }
    );

    remove_old_exes();

    let svc = match store::open(&names) {
        Ok(svc) => svc,
        Err(e) => {
            error!("couldn't open the store in {}: {e}", data_dir.display());
            return 1;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(16)
        .thread_name("agentbolt-daemon")
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!("runtime: {e}");
            return 1;
        }
    };

    let (shared, events) = Shared::new(names.clone(), svc);
    let shutdown = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));

    // Bind both pipes before anything shows: a squatter or a stale daemon makes us stop here.
    let bound = runtime.block_on(async {
        Ok::<_, IpcError>((
            Server::bind(&names, Pipe::Mcp)?,
            Server::bind(&names, Pipe::Desk)?,
        ))
    });
    let (mcp, desk) = match bound {
        Ok(servers) => servers,
        Err(IpcError::AlreadyServed(pipe)) => {
            // On Windows the mutex already turned a second daemon away, so this is a squatter: fail, and the task's
            // retries show in its history. Elsewhere the socket is the single-instance guard: exit quietly.
            if cfg!(windows) {
                error!("{pipe} is already served by another process (a squatter?); stopping");
                return 1;
            }
            info!("{pipe} is already served (another daemon of this channel); exiting");
            return 0;
        }
        Err(e) => {
            error!("couldn't create the pipes: {e}");
            return 1;
        }
    };

    let paused = match shared.get_settings() {
        Ok(settings) => settings.toasts_off,
        Err(e) => {
            warning!("settings: {e}");
            false
        }
    };
    let (open, digest) = shared.counts().unwrap_or((0, 0));

    #[cfg(windows)]
    let handler = {
        let key = winutil::handler_key(&names);
        match &key {
            winutil::HandlerKey::Ours => {}
            winutil::HandlerKey::Missing => {
                warning!(
                    "the {}:// handler isn't registered (HKCU\\Software\\Classes\\{}); toast buttons won't open the Desk",
                    names.protocol,
                    names.protocol
                )
            }
            winutil::HandlerKey::Changed(command) => {
                warning!(
                    "the {}:// handler doesn't point to agentbolt-desk.exe next to the daemon: {command}",
                    names.protocol
                )
            }
        }
        info!("toasts for {}: {}", names.aumid, toast::setting(&names));
        key
    };

    #[cfg(windows)]
    let (ui, ui_rx): (Arc<dyn Ui>, Option<std::sync::mpsc::Receiver<tray::UiCmd>>) =
        if !args.headless {
            let (tx, rx) = std::sync::mpsc::channel();
            (Arc::new(tray::TrayUi(std::sync::Mutex::new(tx))), Some(rx))
        } else {
            (Arc::new(NoUi), None)
        };
    #[cfg(not(windows))]
    let ui: Arc<dyn Ui> = Arc::new(NoUi);

    spawn_server(
        &runtime,
        mcp,
        Arc::new(McpHandler(shared.clone())),
        "mcp",
        &shutdown,
        &failed,
    );
    spawn_server(
        &runtime,
        desk,
        Arc::new(DeskHandler(shared.clone())),
        "desk",
        &shutdown,
        &failed,
    );
    runtime.spawn(daemon::tick_loop(shared.clone()));
    runtime.spawn(daemon::effects_loop(
        shared.clone(),
        events,
        ui,
        paused,
        !args.headless,
    ));
    info!(
        "serving {} and {}",
        Pipe::Mcp.path(&names).display(),
        Pipe::Desk.path(&names).display()
    );

    #[cfg(windows)]
    {
        if let Some(rx) = ui_rx {
            let state = tray::TrayState {
                open,
                digest,
                paused,
                handler,
            };
            if let Err(e) = tray::run(shared.clone(), rx, shutdown.clone(), state) {
                error!("{e}; carrying on without a tray");
                wait_for_shutdown(&shutdown);
            }
        } else {
            let _ = (open, digest, handler);
            wait_for_shutdown(&shutdown);
        }
    }
    #[cfg(not(windows))]
    {
        // Headless off Windows: runs until killed (SIGTERM/SIGINT end the process).
        let _ = (open, digest);
        wait_for_shutdown(&shutdown);
    }

    info!("stopping");
    runtime.shutdown_timeout(Duration::from_secs(3));
    drop(shared);
    info!("stopped");
    if failed.load(Ordering::SeqCst) { 1 } else { 0 }
}

/// Delete the `<exe>.old-*` copies `agentbolt install` renamed aside next to this daemon, best effort: one still
/// running (an older `agentbolt mcp`) can't be deleted yet and waits for the next start or install.
fn remove_old_exes() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_old_exe(&name) && std::fs::remove_file(entry.path()).is_ok() {
            info!("removed {name}, left by an earlier install");
        }
    }
}

/// `agentbolt.exe.old-<stamp>`, `agentbolt-daemon.exe.old-<stamp>` or `agentbolt-desk.exe.old-<stamp>`.
fn is_old_exe(name: &str) -> bool {
    [
        "agentbolt.exe",
        "agentbolt-daemon.exe",
        "agentbolt-desk.exe",
    ]
    .iter()
    .any(|exe| {
        name.len() > exe.len() + 5
            && name
                .get(..exe.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(exe))
            && name
                .get(exe.len()..exe.len() + 5)
                .is_some_and(|p| p.eq_ignore_ascii_case(".old-"))
    })
}

fn wait_for_shutdown(shutdown: &AtomicBool) {
    while !shutdown.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Serve one pipe; if its listener fails for good, stop the daemon with exit code 1 (the task restarts it).
fn spawn_server<H: agentbolt_ipc::Handler>(
    runtime: &tokio::runtime::Runtime,
    server: Server,
    handler: Arc<H>,
    name: &'static str,
    shutdown: &Arc<AtomicBool>,
    failed: &Arc<AtomicBool>,
) {
    let (shutdown, failed) = (shutdown.clone(), failed.clone());
    runtime.spawn(async move {
        if let Err(e) = server.serve(handler).await {
            error!("the {name} pipe failed: {e}; stopping so the sign-in task restarts the daemon");
            failed.store(true, Ordering::SeqCst);
            shutdown.store(true, Ordering::SeqCst);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments() {
        assert_eq!(
            args(&[]).unwrap(),
            Args {
                channel: None,
                headless: false
            }
        );
        assert_eq!(
            args(&["--channel", "t1"]).unwrap(),
            Args {
                channel: Some("t1".into()),
                headless: false
            }
        );
        assert_eq!(
            args(&["--channel=t2", "--headless"]).unwrap(),
            Args {
                channel: Some("t2".into()),
                headless: true
            }
        );
        assert!(args(&["--channel"]).is_err());
        assert!(args(&["--sandbox"]).is_err());
        let names = names_for(&args(&["--channel", "ipctest-args"]).unwrap()).unwrap();
        assert_eq!(names.channel, "ipctest-args");
        assert!(names_for(&args(&["--channel", "Bad Name"]).unwrap()).is_err());
    }

    #[test]
    fn only_renamed_install_leftovers_count_as_old_exes() {
        for old in [
            "agentbolt.exe.old-20261018T120000",
            "Agentbolt-Daemon.EXE.old-1",
            "agentbolt-desk.exe.old-x",
        ] {
            assert!(is_old_exe(old), "{old}");
        }
        for keep in [
            "agentbolt.exe",
            "agentbolt.exe.old-",
            "agentbolt.db",
            "agentbolt.log",
            "other.exe.old-1",
            "é.exe.old-1",
        ] {
            assert!(!is_old_exe(keep), "{keep}");
        }
    }
}
