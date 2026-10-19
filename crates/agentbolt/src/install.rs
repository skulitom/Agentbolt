//! `agentbolt install` / `uninstall` / `start`: the install folder, the HKCU registrations (AUMID and URL
//! protocol) and the sign-in task (PLAN.md, "The sign-in task is specified, not left to defaults").
//!
//! The pure parts (paths, the task XML, the handler command) build and test everywhere; the Windows parts are
//! behind `cfg(windows)`.

use std::path::{Path, PathBuf};

use agentbolt_core::channel::{Names, env_dir};

pub const CLI_EXE: &str = "agentbolt.exe";
pub const DAEMON_EXE: &str = "agentbolt-daemon.exe";
pub const DESK_EXE: &str = "agentbolt-desk.exe";
pub const EXES: [&str; 3] = [CLI_EXE, DAEMON_EXE, DESK_EXE];

/// `%LOCALAPPDATA%\Programs\Agentbolt` for main, `...\Agentbolt-<channel>` for a test channel. An error (never
/// a relative fallback) when `LOCALAPPDATA` is unset: install copies into this folder and uninstall deletes it.
pub fn install_dir(names: &Names) -> anyhow::Result<PathBuf> {
    let local = env_dir("LOCALAPPDATA").map_err(anyhow::Error::msg)?;
    let folder = if names.is_main() {
        "Agentbolt".to_string()
    } else {
        format!("Agentbolt-{}", names.channel)
    };
    Ok(local.join("Programs").join(folder))
}

/// The toast sender's display name.
pub fn display_name(names: &Names) -> String {
    if names.is_main() {
        "Agentbolt".into()
    } else {
        format!("Agentbolt ({})", names.channel)
    }
}

/// `HKCU\<this>` holds the AUMID's display name.
pub fn aumid_key(names: &Names) -> String {
    format!("Software\\Classes\\AppUserModelId\\{}", names.aumid)
}

/// `HKCU\<this>` is the URL protocol's key.
pub fn protocol_key(names: &Names) -> String {
    format!("Software\\Classes\\{}", names.protocol)
}

/// The per-user Installed Apps entry. Test channels have separate entries, like their other registrations.
pub fn uninstall_key(names: &Names) -> String {
    format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        names.aumid
    )
}

/// Windows launches this directly, without a shell. Uninstall keeps the user's questions and answers.
pub fn uninstall_command(install: &Path, channel: &str) -> String {
    format!(
        "\"{}\" uninstall --channel {channel}",
        install.join(CLI_EXE).display()
    )
}

/// The Start-menu shortcut that carries the AUMID (unpackaged apps need one for toasts):
/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Agentbolt.lnk`, or `Agentbolt (<channel>).lnk`.
pub fn shortcut_path(names: &Names) -> anyhow::Result<PathBuf> {
    let roaming = env_dir("APPDATA").map_err(anyhow::Error::msg)?;
    Ok(roaming
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(format!("{}.lnk", display_name(names))))
}

/// What `install` renames a program in the install folder to before copying the new one: Windows lets a running
/// exe be renamed but not overwritten, and `agentbolt mcp` runs from there whenever an agent session is open.
pub fn old_name(exe: &str, stamp: &str) -> String {
    format!("{exe}.old-{stamp}")
}

/// A program renamed aside by an earlier install (`<exe>.old-<stamp>`, any case), left for deletion.
pub fn is_old_copy(file_name: &str) -> bool {
    EXES.iter().any(|exe| {
        let tag = format!("{exe}.old-");
        file_name.len() > tag.len()
            && file_name
                .get(..tag.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(&tag))
    })
}

/// A program an earlier `uninstall` moved into `%TEMP%` because it was running: `agentbolt-uninstalled-*.exe`.
pub fn is_uninstall_leftover(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.starts_with("agentbolt-uninstalled-") && lower.ends_with(".exe")
}

/// The shortcut's arguments: none on main, `--channel <name>` otherwise.
pub fn shortcut_args(names: &Names) -> String {
    if names.is_main() {
        String::new()
    } else {
        format!("--channel {}", names.channel)
    }
}

/// The protocol handler's command line, the value the daemon checks at start (PLAN.md, Toasts).
pub fn protocol_command(install: &Path, channel: &str) -> String {
    format!(
        "\"{}\" --channel {channel} --url \"%1\"",
        install.join(DESK_EXE).display()
    )
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// The sign-in task: at this user's logon, as this user's interactive token (so the tray and toasts appear),
/// with no time limit (the default stops a task after 72 hours), one instance, restarted on failure, and
/// allowed on battery.
pub fn task_xml(user: &str, daemon: &Path, channel: &str, workdir: &Path) -> String {
    let user = xml_escape(user);
    let command = xml_escape(&daemon.display().to_string());
    let workdir = xml_escape(&workdir.display().to_string());
    let channel = xml_escape(channel);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Agentbolt</Author>
    <Description>Agentbolt's daemon (channel {channel}): the desk where agents ask you things. Started at sign-in.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{command}"</Command>
      <Arguments>--channel {channel} --supervise</Arguments>
      <WorkingDirectory>{workdir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// The command lines `agentbolt clients` prints: (what it does, program, args).
pub fn client_commands(
    names: &Names,
    install: &Path,
) -> Vec<(&'static str, &'static str, Vec<String>)> {
    let exe = install.join(CLI_EXE).display().to_string();
    let server = if names.is_main() {
        "agentbolt".to_string()
    } else {
        format!("agentbolt-{}", names.channel)
    };
    let mut mcp = vec!["--".to_string(), exe, "mcp".to_string()];
    if !names.is_main() {
        mcp.extend(["--channel".to_string(), names.channel.clone()]);
    }
    let mut claude = vec![
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        "user".into(),
        server.clone(),
    ];
    claude.extend(mcp.iter().cloned());
    claude.extend(["--max-wait-ms".into(), "600000".into()]);
    let mut codex = vec!["mcp".into(), "add".into(), server.clone()];
    codex.extend(mcp);
    vec![
        ("Register in Claude Code", "claude", claude),
        ("Register in Codex", "codex", codex),
        (
            "Undo in Claude Code",
            "claude",
            vec![
                "mcp".into(),
                "remove".into(),
                "--scope".into(),
                "user".into(),
                server.clone(),
            ],
        ),
        (
            "Undo in Codex",
            "codex",
            vec!["mcp".into(), "remove".into(), server],
        ),
    ]
}

/// A command line for a person to paste: arguments with spaces or quotes get double quotes.
pub fn shell_line(program: &str, args: &[String]) -> String {
    let mut s = program.to_string();
    for a in args {
        s.push(' ');
        if a.is_empty() || a.contains([' ', '\t', '"']) {
            s.push('"');
            s.push_str(&a.replace('"', "\\\""));
            s.push('"');
        } else {
            s.push_str(a);
        }
    }
    s
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use agentbolt_core::channel::Names;
    use anyhow::{Context, bail};

    use super::*;

    /// No console window for the child (the MCP host or the Desk may have none of its own).
    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn startup_barrier(names: &Names) -> anyhow::Result<agentbolt_ipc::windows_util::InstanceLock> {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(lock) = agentbolt_ipc::windows_util::InstanceLock::startup(names)? {
                return Ok(lock);
            }
            anyhow::ensure!(
                Instant::now() < until,
                "A daemon is still initializing; no installation files were changed"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Run schtasks.exe; Ok(stdout) on success, Err(its message) otherwise.
    pub fn schtasks(args: &[&str]) -> Result<String, String> {
        let out = Command::new(agentbolt_ipc::windows_util::schtasks().map_err(|e| e.to_string())?)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("couldn't run schtasks: {e}"))?;
        let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
        if out.status.success() {
            Ok(text(&out.stdout))
        } else {
            let err = text(&out.stderr);
            Err(if err.is_empty() {
                text(&out.stdout)
            } else {
                err
            })
        }
    }

    /// Retry a file operation while a just-stopped process lets go of the file.
    fn retry<T>(what: &str, mut f: impl FnMut() -> std::io::Result<T>) -> anyhow::Result<T> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match f() {
                Ok(v) => return Ok(v),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(e).context(what.to_string());
                }
                Err(e) if Instant::now() >= deadline => return Err(e).context(what.to_string()),
                Err(_) => std::thread::sleep(Duration::from_millis(300)),
            }
        }
    }

    fn write_task_file(xml: &str, channel: &str) -> anyhow::Result<std::path::PathBuf> {
        let path = std::env::temp_dir().join(format!(
            "agentbolt-task-{channel}-{}.xml",
            std::process::id()
        ));
        let mut bytes = vec![0xFF, 0xFE];
        for unit in xml.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    fn register_task_xml(names: &Names, name: &str, xml: &str) -> anyhow::Result<()> {
        let file = write_task_file(xml, &names.channel)?;
        let result = schtasks(&[
            "/Create",
            "/TN",
            name,
            "/XML",
            &file.display().to_string(),
            "/F",
        ]);
        let _ = std::fs::remove_file(file);
        result.map(|_| ()).map_err(anyhow::Error::msg)
    }

    /// A stamp for `<exe>.old-<stamp>` names: seconds since 1970 plus this process's id, so two installs in one
    /// second don't collide.
    fn stamp() -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        format!("{secs}-{}", std::process::id())
    }

    /// Delete `<exe>.old-*` files an earlier install left, best effort (one still running stays until next time).
    pub fn remove_old_copies(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if is_old_copy(&entry.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// One program swapped in: where the old copy went (None when there was none), for putting it back.
    struct Swapped {
        dst: std::path::PathBuf,
        old: Option<std::path::PathBuf>,
    }

    struct Swaps(Vec<Swapped>);
    impl Swaps {
        fn rollback(self) -> anyhow::Result<()> {
            let mut errors = Vec::new();
            for s in self.0.iter().rev() {
                let result = (|| -> std::io::Result<()> {
                    if s.dst.exists() && std::fs::remove_file(&s.dst).is_err() {
                        // A host may already have opened the new MCP executable. Park that image
                        // before restoring the old one; never terminate the host's session.
                        let name = s.dst.file_name().unwrap().to_string_lossy();
                        std::fs::rename(
                            &s.dst,
                            s.dst
                                .with_file_name(old_name(&name, &format!("failed-{}", stamp()))),
                        )?;
                    }
                    if let Some(old) = &s.old {
                        std::fs::rename(old, &s.dst)?;
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    errors.push(format!("{}: {e}", s.dst.display()));
                }
            }
            anyhow::ensure!(
                errors.is_empty(),
                "Could not restore all old programs: {}",
                errors.join("; ")
            );
            Ok(())
        }
    }

    /// Copy the three programs into `dir`. Each program already there is first renamed aside (Windows allows that
    /// for a running exe, which an open agent session's `agentbolt mcp` always is) and the new one copied to its
    /// name. On any failure everything is put back as it was, so the old programs keep working.
    fn swap_in(from: &Path, dir: &Path) -> anyhow::Result<Swaps> {
        let stamp = stamp();
        let mut done: Vec<Swapped> = Vec::new();
        let result = (|| -> anyhow::Result<()> {
            for exe in EXES {
                let (src, dst) = (from.join(exe), dir.join(exe));
                let old = if dst.exists() {
                    let old = dir.join(old_name(exe, &stamp));
                    retry(&format!("renaming {} aside", dst.display()), || {
                        std::fs::rename(&dst, &old)
                    })?;
                    Some(old)
                } else {
                    None
                };
                done.push(Swapped {
                    dst: dst.clone(),
                    old,
                });
                retry(
                    &format!("copying {} to {}", src.display(), dst.display()),
                    || std::fs::copy(&src, &dst),
                )?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            Swaps(done)
                .rollback()
                .context(format!("Replacement failed: {error}"))?;
            return Err(error.context("the old programs were restored"));
        }
        Ok(Swaps(done))
    }

    pub fn install(names: &Names, from: &Path, start: bool) -> anyhow::Result<()> {
        for exe in EXES {
            if !from.join(exe).is_file() {
                bail!(
                    "{} has no {exe}; build all three (cargo build --release) or pass --from <folder>",
                    from.display()
                );
            }
        }
        // Every path first, so a missing LOCALAPPDATA or APPDATA stops here with nothing changed.
        let dir = install_dir(names)?;
        let lnk = shortcut_path(names)?;
        let _install_lock = agentbolt_ipc::windows_util::InstanceLock::maintenance(names)?
            .ok_or_else(|| anyhow::anyhow!("Another installer is working on this channel"))?;
        let startup = startup_barrier(names)?;
        let task_name = agentbolt_ipc::windows_task::name(names).map_err(anyhow::Error::msg)?;
        let old_tasks =
            agentbolt_ipc::windows_task::registrations(names).map_err(anyhow::Error::msg)?;
        let mut upgrade = crate::install_upgrade::Upgrade::prepare(names, &dir)?;
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        remove_old_copies(&dir);
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
        let swaps = if !same(from, &dir) {
            // The old daemon keeps running while the programs are swapped, so a failed copy leaves it working.
            let swaps = swap_in(from, &dir)?;
            println!("Copied the three programs to {}", dir.display());
            Some(swaps)
        } else {
            println!(
                "Installing from the install folder {} itself; nothing to copy",
                dir.display()
            );
            None
        };
        let mut replaced_task = None;
        let mut removed_tasks = Vec::new();
        let mut stopped_tasks = Vec::new();
        let before_start = (|| -> anyhow::Result<()> {
            upgrade.renew()?;
            upgrade.request_visible_warm_restore()?;
            for (index, task) in old_tasks.iter().enumerate() {
                task.stop(names).map_err(anyhow::Error::msg)?;
                stopped_tasks.push(index);
            }
            upgrade.wait_for_daemons()?;

            reg::set_string(&aumid_key(names), Some("DisplayName"), &display_name(names))?;
            let proto = protocol_key(names);
            reg::set_string(&proto, None, &format!("URL:{}", display_name(names)))?;
            reg::set_string(&proto, Some("URL Protocol"), "")?;
            reg::set_string(
                &format!("{proto}\\shell\\open\\command"),
                None,
                &protocol_command(&dir, &names.channel),
            )?;
            println!(
                "Registered the notification sender {} and the {}:// handler",
                names.aumid, names.protocol
            );
            shortcut::create(
                &lnk,
                &dir.join(DESK_EXE),
                &shortcut_args(names),
                &dir,
                &names.aumid,
            )?;
            println!("Created the Start-menu shortcut {}", lnk.display());

            let xml = task_xml(
                &agentbolt_ipc::windows_util::current_user_sid()?,
                &dir.join(DAEMON_EXE),
                &names.channel,
                &dir,
            );
            if let Some(old) = old_tasks.iter().find(|t| t.name == task_name) {
                old.ensure_unchanged(names).map_err(anyhow::Error::msg)?;
            } else {
                agentbolt_ipc::windows_task::ensure_absent(&task_name)
                    .map_err(anyhow::Error::msg)?;
            }
            register_task_xml(names, &task_name, &xml)?;
            replaced_task = agentbolt_ipc::windows_task::registrations(names)
                .map_err(anyhow::Error::msg)?
                .into_iter()
                .find(|t| t.name == task_name);
            anyhow::ensure!(
                replaced_task.is_some(),
                "The newly registered task could not be verified"
            );
            for (index, task) in old_tasks.iter().enumerate() {
                if task.name != task_name {
                    task.delete(names).map_err(anyhow::Error::msg)?;
                    removed_tasks.push(index);
                    println!("Migrated the verified historical task '{}'", task.name);
                }
            }
            println!("Created the sign-in task '{task_name}'");
            Ok(())
        })();
        if let Err(error) = before_start {
            if let Some(swaps) = swaps {
                swaps.rollback()?;
            }
            // Undo only our successful mutations. A definition changed by someone else is never
            // overwritten, even if that change is the reason the forward operation failed.
            if let Some(replacement) = replaced_task {
                replacement
                    .ensure_unchanged(names)
                    .map_err(anyhow::Error::msg)?;
                if let Some(old) = old_tasks.iter().find(|t| t.name == task_name) {
                    register_task_xml(names, &old.name, &old.xml)?;
                } else {
                    replacement.delete(names).map_err(anyhow::Error::msg)?;
                }
            }
            for index in removed_tasks {
                let task = &old_tasks[index];
                agentbolt_ipc::windows_task::ensure_absent(&task.name)
                    .map_err(anyhow::Error::msg)?;
                register_task_xml(names, &task.name, &task.xml)?;
            }
            if !stopped_tasks.is_empty() {
                drop(startup); // Rollback is now complete; only the restored version can start.
                let _ = run_task_in_session(names, upgrade.daemon_session());
            }
            return Err(error.context(
                "Upgrade stopped; old programs were restored and Desk pauses will be cancelled",
            ));
        }
        // Once the new daemon may have migrated or written the database, do not roll its files back
        // underneath it. Report start failure while leaving the complete new installation repairable.
        drop(startup);
        // Only advertise the new version after rollback is no longer possible. Still finish the Desk
        // handoff if registration fails; rerunning install repairs this per-user entry.
        let registration = register_installed_app(names, &dir);
        if start {
            let started =
                start_and_wait_in_session(names, Duration::from_secs(20), upgrade.daemon_session());
            let reopened = upgrade.finish(true);
            started?;
            reopened?;
        } else {
            upgrade.finish(false)?;
        }
        registration?;
        Ok(())
    }

    fn register_installed_app(names: &Names, dir: &Path) -> anyhow::Result<()> {
        let key = uninstall_key(names);
        let command = uninstall_command(dir, &names.channel);
        for (name, value) in [
            ("DisplayName", display_name(names)),
            ("DisplayVersion", env!("CARGO_PKG_VERSION").into()),
            ("Publisher", "Artem Skulimovskiy".into()),
            ("InstallLocation", dir.display().to_string()),
            (
                "URLInfoAbout",
                "https://github.com/skulitom/Agentbolt".into(),
            ),
            ("UninstallString", command.clone()),
            ("QuietUninstallString", command),
        ] {
            reg::set_string(&key, Some(name), &value)?;
        }
        Ok(())
    }

    /// `status` as the installed copy sees it. Pipe clients accept only the daemon next to their own exe, so
    /// from another folder (the unzipped release, a build folder) this asks the installed agentbolt.exe.
    fn installed_status(names: &Names, rt: &tokio::runtime::Runtime) -> Result<String, String> {
        let installed = install_dir(names).map_err(|e| e.to_string())?.join(CLI_EXE);
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        if installed.is_file() && !same(&installed, &me) {
            let out = Command::new(&installed)
                .args(["status", "--channel", &names.channel])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("couldn't run {}: {e}", installed.display()))?;
            let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
            return if out.status.success() {
                Ok(text(&out.stdout))
            } else {
                Err(text(&out.stderr))
            };
        }
        rt.block_on(crate::status(names))
            .map(|s| crate::status_line(&s))
            .map_err(|e| e.to_string())
    }

    /// `schtasks /Run`, then wait for the daemon to answer `status` over the MCP pipe.
    pub fn start_and_wait(names: &Names, wait: Duration) -> anyhow::Result<()> {
        start_and_wait_in_session(names, wait, None)
    }

    fn run_task_in_session(names: &Names, session: Option<u32>) -> anyhow::Result<()> {
        agentbolt_ipc::windows_task::run(names, session).map_err(anyhow::Error::msg)
    }

    pub fn start_and_wait_in_session(
        names: &Names,
        wait: Duration,
        session: Option<u32>,
    ) -> anyhow::Result<()> {
        run_task_in_session(names, session)
            .context("starting the sign-in task in its original session")?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let deadline = Instant::now() + wait;
        let mut last = String::new();
        let mut last_run = Instant::now();
        while Instant::now() < deadline {
            match installed_status(names, &rt) {
                Ok(line) => {
                    println!("{line}");
                    return Ok(());
                }
                Err(e) => last = e,
            }
            // A daemon started while the old one was still letting go of the store exits; ask again (the task's
            // IgnoreNew makes this a no-op while one runs).
            if last_run.elapsed() >= Duration::from_secs(5) {
                let _ = run_task_in_session(names, session);
                last_run = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!(
            "the task started, but the daemon didn't answer within {} s: {last}",
            wait.as_secs()
        )
    }

    /// Delete `%TEMP%\agentbolt-uninstalled-*.exe` files earlier uninstalls parked there, best effort.
    fn remove_uninstall_leftovers() {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            if is_uninstall_leftover(&entry.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// Delete the install folder. Programs still running from it (this one, an open session's `agentbolt mcp`)
    /// can't be deleted but can be moved, so they go to `%TEMP%\agentbolt-uninstalled-*.exe` for a later
    /// uninstall to clear.
    fn remove_install_dir(dir: &Path) -> anyhow::Result<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for (i, entry) in entries.flatten().enumerate() {
                let path = entry.path();
                if path.is_file() && std::fs::remove_file(&path).is_err() {
                    let name = entry.file_name().to_string_lossy().replace(['.', ' '], "_");
                    let parked = std::env::temp_dir().join(format!(
                        "agentbolt-uninstalled-{}-{i}-{name}.exe",
                        std::process::id()
                    ));
                    let _ = std::fs::rename(&path, parked);
                }
            }
        }
        retry(
            &format!("deleting {} (is Agentbolt still running?)", dir.display()),
            || std::fs::remove_dir_all(dir),
        )
    }

    pub fn uninstall(names: &Names, purge: bool) -> anyhow::Result<()> {
        // Every path first: a missing LOCALAPPDATA or APPDATA stops here, before anything is removed, and no
        // relative folder is ever deleted.
        let dir = install_dir(names)?;
        let lnk = shortcut_path(names)?;
        let data = if purge {
            Some(names.data_dir().map_err(anyhow::Error::msg)?)
        } else {
            None
        };
        for p in [&dir, &lnk].into_iter().chain(data.as_ref()) {
            if !p.is_absolute() {
                bail!("refusing to remove {}: not an absolute path", p.display());
            }
        }
        let _install_lock = agentbolt_ipc::windows_util::InstanceLock::maintenance(names)?
            .ok_or_else(|| anyhow::anyhow!("Another installer is working on this channel"))?;
        let _startup = startup_barrier(names)?;
        let tasks =
            agentbolt_ipc::windows_task::registrations(names).map_err(anyhow::Error::msg)?;
        let mut upgrade = crate::install_upgrade::Upgrade::prepare(names, &dir)?;
        remove_uninstall_leftovers();
        for task in &tasks {
            task.stop(names).map_err(anyhow::Error::msg)?;
            task.delete(names).map_err(anyhow::Error::msg)?;
            println!("Deleted the sign-in task '{}'", task.name);
        }
        upgrade.wait_for_daemons()?;
        upgrade.finish(false)?;
        for key in [protocol_key(names), aumid_key(names), uninstall_key(names)] {
            if reg::delete_tree(&key)? {
                println!("Removed HKCU\\{key}");
            }
        }
        if lnk.exists() {
            std::fs::remove_file(&lnk).with_context(|| format!("deleting {}", lnk.display()))?;
            println!("Removed {}", lnk.display());
        }
        if dir.exists() {
            remove_install_dir(&dir)?;
            println!("Deleted {}", dir.display());
        }
        if let Some(data) = data
            && data.exists()
        {
            retry(&format!("deleting {}", data.display()), || {
                std::fs::remove_dir_all(&data)
            })?;
            println!("Deleted {}", data.display());
        }
        // The MCP registrations live in the clients' own config: say how to remove them, never run it here.
        println!("\nTo remove the MCP server from your clients, run:");
        for (what, program, args) in client_commands(names, &dir)
            .iter()
            .filter(|(w, _, _)| w.starts_with("Undo"))
        {
            println!(
                "  {}  ({})",
                shell_line(program, args),
                what.trim_start_matches("Undo in ")
            );
        }
        Ok(())
    }

    /// The Start-menu shortcut with `System.AppUserModel.ID` set (IShellLinkW + IPropertyStore).
    pub mod shortcut {
        use std::mem::ManuallyDrop;
        use std::path::Path;

        use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
        use windows::Win32::System::Com::StructuredStorage::{
            PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
        };
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemAlloc, CoUninitialize, IPersistFile,
        };
        use windows::Win32::System::Variant::VT_LPWSTR;
        use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        use windows::core::{HSTRING, Interface, PWSTR};

        /// A VT_LPWSTR value (the AUMID's type); the string is CoTaskMem-allocated, so dropping the
        /// PROPVARIANT (PropVariantClear) frees it.
        fn lpwstr(s: &str) -> windows::core::Result<PROPVARIANT> {
            let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let p = CoTaskMemAlloc(wide.len() * 2).cast::<u16>();
                if p.is_null() {
                    return Err(windows::core::Error::from_hresult(
                        windows::Win32::Foundation::E_OUTOFMEMORY,
                    ));
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                Ok(PROPVARIANT {
                    Anonymous: PROPVARIANT_0 {
                        Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                            vt: VT_LPWSTR,
                            wReserved1: 0,
                            wReserved2: 0,
                            wReserved3: 0,
                            Anonymous: PROPVARIANT_0_0_0 { pwszVal: PWSTR(p) },
                        }),
                    },
                })
            }
        }

        pub fn create(
            lnk: &Path,
            target: &Path,
            args: &str,
            workdir: &Path,
            aumid: &str,
        ) -> anyhow::Result<()> {
            if let Some(dir) = lnk.parent() {
                std::fs::create_dir_all(dir)?;
            }
            unsafe {
                // S_OK or S_FALSE need a matching CoUninitialize; RPC_E_CHANGED_MODE (COM already up) doesn't.
                let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
                let result = (|| -> windows::core::Result<()> {
                    let link: IShellLinkW =
                        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                    link.SetPath(&HSTRING::from(target.as_os_str()))?;
                    link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
                    link.SetArguments(&HSTRING::from(args))?;
                    link.SetWorkingDirectory(&HSTRING::from(workdir.as_os_str()))?;
                    link.SetDescription(&HSTRING::from(
                        "Agentbolt: the desk where agents ask you",
                    ))?;
                    let store: IPropertyStore = link.cast()?;
                    store.SetValue(&PKEY_AppUserModel_ID, &lpwstr(aumid)?)?;
                    store.Commit()?;
                    let file: IPersistFile = link.cast()?;
                    file.Save(&HSTRING::from(lnk.as_os_str()), true)
                })();
                if initialized {
                    CoUninitialize();
                }
                result.map_err(|e| {
                    anyhow::anyhow!("creating the shortcut {}: {}", lnk.display(), e.message())
                })
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn read(p: &Path) -> String {
            std::fs::read_to_string(p).unwrap()
        }

        #[test]
        fn a_failed_swap_puts_the_old_programs_back_and_a_good_one_renames_them_aside() {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            let root = std::env::temp_dir().join(format!(
                "agentbolt-swap-test-{}-{nanos:x}",
                std::process::id()
            ));
            let (from, dir) = (root.join("from"), root.join("dir"));
            std::fs::create_dir_all(&from).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            for exe in EXES {
                std::fs::write(dir.join(exe), format!("old {exe}")).unwrap();
            }
            // The new build lacks the Desk, so the third copy fails after two programs were swapped.
            for exe in [CLI_EXE, DAEMON_EXE] {
                std::fs::write(from.join(exe), format!("new {exe}")).unwrap();
            }
            assert!(swap_in(&from, &dir).is_err());
            let names: Vec<String> = std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                names.len(),
                3,
                "nothing renamed aside is left over: {names:?}"
            );
            for exe in EXES {
                assert_eq!(
                    read(&dir.join(exe)),
                    format!("old {exe}"),
                    "{exe} was put back"
                );
            }

            std::fs::write(from.join(DESK_EXE), format!("new {DESK_EXE}")).unwrap();
            swap_in(&from, &dir).unwrap();
            for exe in EXES {
                assert_eq!(read(&dir.join(exe)), format!("new {exe}"));
            }
            let old: Vec<String> = std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| is_old_copy(n))
                .collect();
            assert_eq!(old.len(), 3, "{old:?}");
            remove_old_copies(&dir);
            assert_eq!(
                std::fs::read_dir(&dir).unwrap().count(),
                3,
                "the next install clears them"
            );
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    pub mod reg {
        use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
        use windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
            RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW,
            RegSetValueExW,
        };
        use windows::core::{HSTRING, PCWSTR};

        fn check(e: WIN32_ERROR, what: &str) -> anyhow::Result<()> {
            if e == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "{what}: {}",
                    windows::core::Error::from(e.to_hresult()).message()
                ))
            }
        }

        /// Create `HKCU\<subkey>` if needed and set a string value (`None` = the default value).
        pub fn set_string(subkey: &str, name: Option<&str>, value: &str) -> anyhow::Result<()> {
            let mut key = HKEY::default();
            unsafe {
                check(
                    RegCreateKeyExW(
                        HKEY_CURRENT_USER,
                        &HSTRING::from(subkey),
                        None,
                        PCWSTR::null(),
                        REG_OPTION_NON_VOLATILE,
                        KEY_WRITE | KEY_READ,
                        None,
                        &mut key,
                        None,
                    ),
                    &format!("creating HKCU\\{subkey}"),
                )?;
                let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
                let name_h = name.map(HSTRING::from);
                let name_p = name_h
                    .as_ref()
                    .map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()));
                let r = RegSetValueExW(key, name_p, None, REG_SZ, Some(bytes));
                let _ = RegCloseKey(key);
                check(r, &format!("setting HKCU\\{subkey}"))
            }
        }

        /// Read a string value, if the key and value exist.
        pub fn get_string(subkey: &str, name: Option<&str>) -> Option<String> {
            let name_h = name.map(HSTRING::from);
            let name_p = name_h
                .as_ref()
                .map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()));
            let sub = HSTRING::from(subkey);
            let mut size = 0u32;
            unsafe {
                if RegGetValueW(
                    HKEY_CURRENT_USER,
                    &sub,
                    name_p,
                    RRF_RT_REG_SZ,
                    None,
                    None,
                    Some(&mut size),
                ) != ERROR_SUCCESS
                {
                    return None;
                }
                let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
                let mut size = (buf.len() * 2) as u32;
                if RegGetValueW(
                    HKEY_CURRENT_USER,
                    &sub,
                    name_p,
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut size),
                ) != ERROR_SUCCESS
                {
                    return None;
                }
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                Some(String::from_utf16_lossy(&buf[..len]))
            }
        }

        /// Delete `HKCU\<subkey>` and everything under it. Ok(false) when it wasn't there.
        pub fn delete_tree(subkey: &str) -> anyhow::Result<bool> {
            let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(subkey)) };
            if r == ERROR_FILE_NOT_FOUND {
                return Ok(false);
            }
            check(r, &format!("deleting HKCU\\{subkey}")).map(|_| true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_xml_says_what_plan_md_specifies() {
        let x = task_xml(
            "PC\\artem",
            Path::new("C:\\A b\\agentbolt-daemon.exe"),
            "t1",
            Path::new("C:\\A b"),
        );
        for needle in [
            "<LogonTrigger>",
            "<UserId>PC\\artem</UserId>",
            "<LogonType>InteractiveToken</LogonType>",
            "<RunLevel>LeastPrivilege</RunLevel>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>",
            "<RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>3</Count>",
            "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
            "<Hidden>true</Hidden>",
            "<Command>\"C:\\A b\\agentbolt-daemon.exe\"</Command>",
            "<Arguments>--channel t1 --supervise</Arguments>",
        ] {
            assert!(x.contains(needle), "the task XML lacks {needle}");
        }
    }

    #[test]
    fn xml_values_are_escaped() {
        let x = task_xml(
            "D&D\\<me>",
            Path::new("C:\\x\\d.exe"),
            "t1",
            Path::new("C:\\x"),
        );
        assert!(x.contains("<UserId>D&amp;D\\&lt;me&gt;</UserId>"));
        assert!(!x.contains("D&D"));
    }

    #[test]
    fn names_per_channel() {
        let main = Names::for_channel("main").unwrap();
        let t = Names::for_channel("t1").unwrap();
        // Install paths are Windows-only; Linux still checks channel names and protocol quoting below.
        #[cfg(windows)]
        {
            let main_dir = install_dir(&main).unwrap();
            assert!(main_dir.is_absolute());
            assert!(
                main_dir.ends_with("Programs/Agentbolt")
                    || main_dir.ends_with("Programs\\Agentbolt")
            );
            assert!(
                install_dir(&t)
                    .unwrap()
                    .to_string_lossy()
                    .ends_with("Agentbolt-t1")
            );
        }
        assert_eq!(
            aumid_key(&t),
            "Software\\Classes\\AppUserModelId\\Skulitom.Agentbolt.t1"
        );
        assert_eq!(protocol_key(&t), "Software\\Classes\\agentbolt-t1");
        assert_eq!(display_name(&main), "Agentbolt");
        let cmd = protocol_command(Path::new("C:\\P\\Agentbolt-t1"), "t1");
        assert!(
            cmd.starts_with('"') && cmd.contains("agentbolt-desk.exe\" --channel t1 --url \"%1\""),
            "{cmd}"
        );
    }

    #[test]
    fn renamed_and_parked_programs_are_recognised() {
        let old = old_name(CLI_EXE, "1792400000-42");
        assert_eq!(old, "agentbolt.exe.old-1792400000-42");
        assert!(is_old_copy(&old));
        assert!(is_old_copy("AGENTBOLT-DAEMON.EXE.OLD-1"));
        for keep in [
            CLI_EXE,
            DAEMON_EXE,
            DESK_EXE,
            "agentbolt.exe.old-",
            "agentbolt.db",
            "x.exe.old-1",
            "é.old-1",
        ] {
            assert!(!is_old_copy(keep), "{keep}");
        }
        assert!(is_uninstall_leftover("agentbolt-uninstalled-1234.exe"));
        assert!(is_uninstall_leftover(
            "agentbolt-uninstalled-1234-0-agentbolt_exe.exe"
        ));
        assert!(!is_uninstall_leftover("agentbolt-task-t1-9.xml"));
        assert!(!is_uninstall_leftover("agentbolt.exe"));
    }

    #[test]
    #[cfg(windows)] // Windows paths: elsewhere `join` adds a '/'.
    fn client_commands_name_the_installed_exe() {
        let main = Names::for_channel("main").unwrap();
        let cmds = client_commands(&main, Path::new("C:\\Users\\a b\\Programs\\Agentbolt"));
        let lines: Vec<String> = cmds.iter().map(|(_, p, a)| shell_line(p, a)).collect();
        assert_eq!(
            lines[0],
            "claude mcp add --scope user agentbolt -- \"C:\\Users\\a b\\Programs\\Agentbolt\\agentbolt.exe\" mcp --max-wait-ms 600000"
        );
        assert_eq!(
            lines[1],
            "codex mcp add agentbolt -- \"C:\\Users\\a b\\Programs\\Agentbolt\\agentbolt.exe\" mcp"
        );
        assert_eq!(lines[2], "claude mcp remove --scope user agentbolt");
        assert_eq!(lines[3], "codex mcp remove agentbolt");
        let t = Names::for_channel("t1").unwrap();
        let cmds = client_commands(&t, Path::new("C:\\P"));
        assert!(shell_line(cmds[1].1, &cmds[1].2).ends_with("mcp --channel t1"));
        assert!(cmds[0].2.contains(&"agentbolt-t1".to_string()));
    }
}
