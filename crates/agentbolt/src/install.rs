//! `agentbolt install` / `uninstall` / `start`: the install folder, the HKCU registrations (AUMID and URL
//! protocol) and the sign-in task (PLAN.md, "The sign-in task is specified, not left to defaults").
//!
//! The pure parts (paths, the task XML, the handler command) build and test everywhere; the Windows parts are
//! behind `cfg(windows)`.

use std::path::{Path, PathBuf};

use agentbolt_core::channel::Names;

pub const CLI_EXE: &str = "agentbolt.exe";
pub const DAEMON_EXE: &str = "agentbolt-daemon.exe";
pub const DESK_EXE: &str = "agentbolt-desk.exe";
pub const EXES: [&str; 3] = [CLI_EXE, DAEMON_EXE, DESK_EXE];

/// `%LOCALAPPDATA%\Programs\Agentbolt` for main, `...\Agentbolt-<channel>` for a test channel.
pub fn install_dir(names: &Names) -> PathBuf {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    let folder = if names.is_main() { "Agentbolt".to_string() } else { format!("Agentbolt-{}", names.channel) };
    PathBuf::from(local).join("Programs").join(folder)
}

/// The toast sender's display name.
pub fn display_name(names: &Names) -> String {
    if names.is_main() { "Agentbolt".into() } else { format!("Agentbolt ({})", names.channel) }
}

/// `HKCU\<this>` holds the AUMID's display name.
pub fn aumid_key(names: &Names) -> String {
    format!("Software\\Classes\\AppUserModelId\\{}", names.aumid)
}

/// `HKCU\<this>` is the URL protocol's key.
pub fn protocol_key(names: &Names) -> String {
    format!("Software\\Classes\\{}", names.protocol)
}

/// The Start-menu shortcut that carries the AUMID (unpackaged apps need one for toasts):
/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Agentbolt.lnk`, or `Agentbolt (<channel>).lnk`.
pub fn shortcut_path(names: &Names) -> PathBuf {
    let roaming = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(roaming)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(format!("{}.lnk", display_name(names)))
}

/// The shortcut's arguments: none on main, `--channel <name>` otherwise.
pub fn shortcut_args(names: &Names) -> String {
    if names.is_main() { String::new() } else { format!("--channel {}", names.channel) }
}

/// The protocol handler's command line, the value the daemon checks at start (PLAN.md, Toasts).
pub fn protocol_command(install: &Path, channel: &str) -> String {
    format!("\"{}\" --channel {channel} --url \"%1\"", install.join(DESK_EXE).display())
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
      <Arguments>--channel {channel}</Arguments>
      <WorkingDirectory>{workdir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// The command lines `agentbolt clients` prints: (what it does, program, args).
pub fn client_commands(names: &Names, install: &Path) -> Vec<(&'static str, &'static str, Vec<String>)> {
    let exe = install.join(CLI_EXE).display().to_string();
    let server = if names.is_main() { "agentbolt".to_string() } else { format!("agentbolt-{}", names.channel) };
    let mut mcp = vec!["--".to_string(), exe, "mcp".to_string()];
    if !names.is_main() {
        mcp.extend(["--channel".to_string(), names.channel.clone()]);
    }
    let mut claude = vec!["mcp".into(), "add".into(), "--scope".into(), "user".into(), server.clone()];
    claude.extend(mcp.iter().cloned());
    let mut codex = vec!["mcp".into(), "add".into(), server.clone()];
    codex.extend(mcp);
    vec![
        ("Register in Claude Code", "claude", claude),
        ("Register in Codex", "codex", codex),
        ("Undo in Claude Code", "claude", vec!["mcp".into(), "remove".into(), "--scope".into(), "user".into(), server.clone()]),
        ("Undo in Codex", "codex", vec!["mcp".into(), "remove".into(), server]),
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

    /// Run schtasks.exe; Ok(stdout) on success, Err(its message) otherwise.
    pub fn schtasks(args: &[&str]) -> Result<String, String> {
        let out = Command::new("schtasks.exe")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("couldn't run schtasks: {e}"))?;
        let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
        if out.status.success() {
            Ok(text(&out.stdout))
        } else {
            let err = text(&out.stderr);
            Err(if err.is_empty() { text(&out.stdout) } else { err })
        }
    }

    pub fn task_exists(names: &Names) -> bool {
        schtasks(&["/Query", "/TN", &names.task]).is_ok()
    }

    /// `DOMAIN\user` for the task's trigger and principal.
    fn current_user() -> anyhow::Result<String> {
        let user = std::env::var("USERNAME").context("USERNAME isn't set")?;
        Ok(match std::env::var("USERDOMAIN") {
            Ok(d) if !d.is_empty() => format!("{d}\\{user}"),
            _ => user,
        })
    }

    /// Retry a file operation while a just-stopped process lets go of the file.
    fn retry<T>(what: &str, mut f: impl FnMut() -> std::io::Result<T>) -> anyhow::Result<T> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match f() {
                Ok(v) => return Ok(v),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e).context(what.to_string()),
                Err(e) if Instant::now() >= deadline => return Err(e).context(what.to_string()),
                Err(_) => std::thread::sleep(Duration::from_millis(300)),
            }
        }
    }

    fn write_task_file(xml: &str, channel: &str) -> anyhow::Result<std::path::PathBuf> {
        let path = std::env::temp_dir().join(format!("agentbolt-task-{channel}-{}.xml", std::process::id()));
        let mut bytes = vec![0xFF, 0xFE];
        for unit in xml.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn install(names: &Names, from: &Path, start: bool) -> anyhow::Result<()> {
        for exe in EXES {
            if !from.join(exe).is_file() {
                bail!("{} has no {exe}; build all three (cargo build --release) or pass --from <folder>", from.display());
            }
        }
        let dir = install_dir(names);
        if task_exists(names) {
            let _ = schtasks(&["/End", "/TN", &names.task]);
        }
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
        if !same(from, &dir) {
            for exe in EXES {
                let (src, dst) = (from.join(exe), dir.join(exe));
                retry(&format!("copying {} (is it still running?)", dst.display()), || std::fs::copy(&src, &dst))?;
            }
        }
        println!("Copied the three programs to {}", dir.display());

        reg::set_string(&aumid_key(names), Some("DisplayName"), &display_name(names))?;
        let proto = protocol_key(names);
        reg::set_string(&proto, None, &format!("URL:{}", display_name(names)))?;
        reg::set_string(&proto, Some("URL Protocol"), "")?;
        reg::set_string(&format!("{proto}\\shell\\open\\command"), None, &protocol_command(&dir, &names.channel))?;
        println!("Registered the notification sender {} and the {}:// handler", names.aumid, names.protocol);
        let lnk = shortcut_path(names);
        shortcut::create(&lnk, &dir.join(DESK_EXE), &shortcut_args(names), &dir, &names.aumid)?;
        println!("Created the Start-menu shortcut {}", lnk.display());

        let xml = task_xml(&current_user()?, &dir.join(DAEMON_EXE), &names.channel, &dir);
        let file = write_task_file(&xml, &names.channel)?;
        let created = schtasks(&["/Create", "/TN", &names.task, "/XML", &file.display().to_string(), "/F"]);
        let _ = std::fs::remove_file(&file);
        created.map_err(|e| anyhow::anyhow!("creating the sign-in task '{}': {e}", names.task))?;
        println!("Created the sign-in task '{}'", names.task);

        if start {
            start_and_wait(names, Duration::from_secs(20))?;
        }
        Ok(())
    }

    /// `status` as the installed copy sees it. Pipe clients accept only the daemon next to their own exe, so
    /// from another folder (the unzipped release, a build folder) this asks the installed agentbolt.exe.
    fn installed_status(names: &Names, rt: &tokio::runtime::Runtime) -> Result<String, String> {
        let installed = install_dir(names).join(CLI_EXE);
        let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y);
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        if installed.is_file() && !same(&installed, &me) {
            let out = Command::new(&installed)
                .args(["status", "--channel", &names.channel])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("couldn't run {}: {e}", installed.display()))?;
            let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
            return if out.status.success() { Ok(text(&out.stdout)) } else { Err(text(&out.stderr)) };
        }
        rt.block_on(crate::status(names)).map(|s| crate::status_line(&s)).map_err(|e| e.to_string())
    }

    /// `schtasks /Run`, then wait for the daemon to answer `status` over the MCP pipe.
    pub fn start_and_wait(names: &Names, wait: Duration) -> anyhow::Result<()> {
        schtasks(&["/Run", "/TN", &names.task]).map_err(|e| anyhow::anyhow!("starting '{}': {e}", names.task))?;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let deadline = Instant::now() + wait;
        let mut last = String::new();
        while Instant::now() < deadline {
            match installed_status(names, &rt) {
                Ok(line) => {
                    println!("{line}");
                    return Ok(());
                }
                Err(e) => last = e,
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("the task started, but the daemon didn't answer within {} s: {last}", wait.as_secs())
    }

    pub fn uninstall(names: &Names, purge: bool) -> anyhow::Result<()> {
        if task_exists(names) {
            let _ = schtasks(&["/End", "/TN", &names.task]);
            schtasks(&["/Delete", "/TN", &names.task, "/F"])
                .map_err(|e| anyhow::anyhow!("deleting the sign-in task '{}': {e}", names.task))?;
            println!("Deleted the sign-in task '{}'", names.task);
        }
        for key in [protocol_key(names), aumid_key(names)] {
            if reg::delete_tree(&key)? {
                println!("Removed HKCU\\{key}");
            }
        }
        let lnk = shortcut_path(names);
        if lnk.exists() {
            std::fs::remove_file(&lnk).with_context(|| format!("deleting {}", lnk.display()))?;
            println!("Removed {}", lnk.display());
        }
        let dir = install_dir(names);
        if dir.exists() {
            // A running agentbolt.exe from this folder can be renamed out of the way, not deleted.
            if let Ok(me) = std::env::current_exe()
                && me.parent().is_some_and(|p| p.canonicalize().ok() == dir.canonicalize().ok())
            {
                let parked = std::env::temp_dir().join(format!("agentbolt-uninstalled-{}.exe", std::process::id()));
                let _ = std::fs::rename(&me, parked);
            }
            retry(&format!("deleting {} (is Agentbolt still running?)", dir.display()), || {
                std::fs::remove_dir_all(&dir)
            })?;
            println!("Deleted {}", dir.display());
        }
        if purge {
            let data = names.data_dir();
            if data.exists() {
                retry(&format!("deleting {}", data.display()), || std::fs::remove_dir_all(&data))?;
                println!("Deleted {}", data.display());
            }
        }
        Ok(())
    }

    /// The Start-menu shortcut with `System.AppUserModel.ID` set (IShellLinkW + IPropertyStore).
    pub mod shortcut {
        use std::mem::ManuallyDrop;
        use std::path::Path;

        use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
        use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemAlloc,
            CoUninitialize, IPersistFile,
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
                    return Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_OUTOFMEMORY));
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

        pub fn create(lnk: &Path, target: &Path, args: &str, workdir: &Path, aumid: &str) -> anyhow::Result<()> {
            if let Some(dir) = lnk.parent() {
                std::fs::create_dir_all(dir)?;
            }
            unsafe {
                // S_OK or S_FALSE need a matching CoUninitialize; RPC_E_CHANGED_MODE (COM already up) doesn't.
                let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
                let result = (|| -> windows::core::Result<()> {
                    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                    link.SetPath(&HSTRING::from(target.as_os_str()))?;
                    link.SetArguments(&HSTRING::from(args))?;
                    link.SetWorkingDirectory(&HSTRING::from(workdir.as_os_str()))?;
                    link.SetDescription(&HSTRING::from("Agentbolt: the desk where agents ask you"))?;
                    let store: IPropertyStore = link.cast()?;
                    store.SetValue(&PKEY_AppUserModel_ID, &lpwstr(aumid)?)?;
                    store.Commit()?;
                    let file: IPersistFile = link.cast()?;
                    file.Save(&HSTRING::from(lnk.as_os_str()), true)
                })();
                if initialized {
                    CoUninitialize();
                }
                result.map_err(|e| anyhow::anyhow!("creating the shortcut {}: {}", lnk.display(), e.message()))
            }
        }
    }

    pub mod reg {
        use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
        use windows::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
            RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
        };
        use windows::core::{HSTRING, PCWSTR};

        fn check(e: WIN32_ERROR, what: &str) -> anyhow::Result<()> {
            if e == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(anyhow::anyhow!("{what}: {}", windows::core::Error::from(e.to_hresult()).message()))
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
                let name_p = name_h.as_ref().map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()));
                let r = RegSetValueExW(key, name_p, None, REG_SZ, Some(bytes));
                let _ = RegCloseKey(key);
                check(r, &format!("setting HKCU\\{subkey}"))
            }
        }

        /// Read a string value, if the key and value exist.
        pub fn get_string(subkey: &str, name: Option<&str>) -> Option<String> {
            let name_h = name.map(HSTRING::from);
            let name_p = name_h.as_ref().map_or(PCWSTR::null(), |h| PCWSTR(h.as_ptr()));
            let sub = HSTRING::from(subkey);
            let mut size = 0u32;
            unsafe {
                if RegGetValueW(HKEY_CURRENT_USER, &sub, name_p, RRF_RT_REG_SZ, None, None, Some(&mut size)) != ERROR_SUCCESS {
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
        let x = task_xml("PC\\artem", Path::new("C:\\A b\\agentbolt-daemon.exe"), "t1", Path::new("C:\\A b"));
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
            "<Arguments>--channel t1</Arguments>",
        ] {
            assert!(x.contains(needle), "the task XML lacks {needle}");
        }
    }

    #[test]
    fn xml_values_are_escaped() {
        let x = task_xml("D&D\\<me>", Path::new("C:\\x\\d.exe"), "t1", Path::new("C:\\x"));
        assert!(x.contains("<UserId>D&amp;D\\&lt;me&gt;</UserId>"));
        assert!(!x.contains("D&D"));
    }

    #[test]
    fn names_per_channel() {
        let main = Names::for_channel("main").unwrap();
        let t = Names::for_channel("t1").unwrap();
        assert!(install_dir(&main).ends_with("Programs/Agentbolt") || install_dir(&main).ends_with("Programs\\Agentbolt"));
        assert!(install_dir(&t).to_string_lossy().ends_with("Agentbolt-t1"));
        assert_eq!(aumid_key(&t), "Software\\Classes\\AppUserModelId\\Skulitom.Agentbolt.t1");
        assert_eq!(protocol_key(&t), "Software\\Classes\\agentbolt-t1");
        assert_eq!(display_name(&main), "Agentbolt");
        let cmd = protocol_command(Path::new("C:\\P\\Agentbolt-t1"), "t1");
        assert!(cmd.starts_with('"') && cmd.contains("agentbolt-desk.exe\" --channel t1 --url \"%1\""), "{cmd}");
    }

    #[test]
    #[cfg(windows)] // Windows paths: elsewhere `join` adds a '/'.
    fn client_commands_name_the_installed_exe() {
        let main = Names::for_channel("main").unwrap();
        let cmds = client_commands(&main, Path::new("C:\\Users\\a b\\Programs\\Agentbolt"));
        let lines: Vec<String> = cmds.iter().map(|(_, p, a)| shell_line(p, a)).collect();
        assert_eq!(lines[0], "claude mcp add --scope user agentbolt -- \"C:\\Users\\a b\\Programs\\Agentbolt\\agentbolt.exe\" mcp");
        assert_eq!(lines[1], "codex mcp add agentbolt -- \"C:\\Users\\a b\\Programs\\Agentbolt\\agentbolt.exe\" mcp");
        assert_eq!(lines[2], "claude mcp remove --scope user agentbolt");
        assert_eq!(lines[3], "codex mcp remove agentbolt");
        let t = Names::for_channel("t1").unwrap();
        let cmds = client_commands(&t, Path::new("C:\\P"));
        assert!(shell_line(cmds[1].1, &cmds[1].2).ends_with("mcp --channel t1"));
        assert!(cmds[0].2.contains(&"agentbolt-t1".to_string()));
    }
}
