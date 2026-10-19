//! `agentbolt install` / `uninstall` on a throwaway channel: the task, the HKCU keys and the install folder
//! appear, the daemon answers `status` (when a daemon build sits next to this one), and uninstall removes it all.
//!
//! It registers real per-user things (a scheduled task, HKCU keys, a folder under %LOCALAPPDATA%\Programs), so
//! it runs only when `AGENTBOLT_INSTALL_TEST=1`. It never touches the `main` channel, and a guard uninstalls and
//! then deletes leftovers directly, even when an assertion fails.
#![cfg(windows)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct TestProcess(Child);
impl Drop for TestProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Mcp {
    child: Child,
    input: std::process::ChildStdin,
    output: std::sync::mpsc::Receiver<String>,
    id: u64,
}
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Mcp {
    fn start(exe: &Path, channel: &str) -> Self {
        let mut child = Command::new(exe)
            .args(["mcp", "--channel", channel, "--owner", "installer-fixture"])
            .env("AGENTBOLT_CHANNEL", channel)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, output) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if let Ok(line) = line {
                    if tx.send(line).is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });
        let mut mcp = Self {
            child,
            input,
            output,
            id: 0,
        };
        mcp.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"installer-test","version":"0"}}));
        writeln!(
            mcp.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        mcp.input.flush().unwrap();
        mcp
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params})
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let line = self
                .output
                .recv_timeout(std::time::Duration::from_secs(15))
                .expect("MCP response");
            let value: Value = serde_json::from_str(&line).unwrap();
            if value["id"] == self.id {
                assert!(value.get("error").is_none(), "{value}");
                return value["result"].clone();
            }
        }
    }
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let started = std::time::Instant::now();
        loop {
            let result = self.request("tools/call", json!({"name":name,"arguments":args}));
            if result["isError"] == true {
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(15),
                    "MCP failed to reconnect: {result}"
                );
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            let text = result["content"][0]["text"].as_str().unwrap();
            let mirrored: Value =
                serde_json::from_str(text.rsplit_once("\n\n").unwrap().1).unwrap();
            assert_eq!(mirrored, result["structuredContent"]);
            return mirrored;
        }
    }
}

fn cli() -> PathBuf {
    // Exercise an extracted release's actual installer as well as Cargo's normal test binary.
    if std::env::var("AGENTBOLT_INSTALL_TEST").as_deref() == Ok("1")
        && let Some(exe) = std::env::var_os("AGENTBOLT_INSTALL_TEST_EXE")
    {
        let exe = PathBuf::from(exe);
        assert!(
            exe.is_absolute() && exe.is_file(),
            "test installer must be an existing absolute path"
        );
        return exe;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_agentbolt"))
}

fn run(exe: &Path, args: &[&str]) -> Output {
    Command::new(exe)
        .args(args)
        .env_remove("AGENTBOLT_CHANNEL")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn daemon_pid(n: &Names) -> Option<u32> {
    let log = std::fs::read_to_string(n.data.join("agentbolt.log")).ok()?;
    let marker = format!("starting: channel {}, pid ", n.channel);
    log.lines()
        .rev()
        .find_map(|line| line.split_once(&marker)?.1.split_once(',')?.0.parse().ok())
}

/// Kill only this fresh installation's verified daemon, then observe recovery without running /Run or MCP.
fn check_crash_restart(n: &Names, installed_cli: &Path) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
    };
    use windows::core::PWSTR;
    let pid = daemon_pid(n).expect("daemon logged its pid");
    unsafe {
        let mut daemon_session = 0;
        let mut test_session = 0;
        ProcessIdToSessionId(pid, &mut daemon_session).unwrap();
        ProcessIdToSessionId(GetCurrentProcessId(), &mut test_session).unwrap();
        // InteractiveToken chooses the user's interactive session, which can be the console when
        // this test calls /Run from Anode. It need not be the caller's desktop session.
        eprintln!(
            "task launch: test session {test_session}, daemon session {daemon_session}, channel {}",
            n.channel
        );
        let process = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
        .unwrap();
        let mut path = vec![0u16; 32768];
        let mut len = path.len() as u32;
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )
        .unwrap();
        let image = PathBuf::from(String::from_utf16(&path[..len as usize]).unwrap());
        assert_eq!(
            image.canonicalize().unwrap(),
            n.install
                .join("agentbolt-daemon.exe")
                .canonicalize()
                .unwrap()
        );
        TerminateProcess(process, 1).unwrap();
        let _ = CloseHandle(process);
    }
    let started = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if daemon_pid(n).is_some_and(|next| next != pid)
            && run(installed_cli, &["status", "--channel", &n.channel])
                .status
                .success()
        {
            eprintln!(
                "crash restart: channel {}, session task recovered in {:.1}s without a client start",
                n.channel,
                started.elapsed().as_secs_f64()
            );
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(95),
            "the task did not restart the crashed daemon within 95s"
        );
    }
}

fn schtasks(args: &[&str]) -> Output {
    Command::new("schtasks.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap()
}

fn reg_query(key: &str, value: Option<&str>) -> Option<String> {
    let mut c = Command::new("reg.exe");
    c.args(["query", &format!("HKCU\\{key}")]);
    match value {
        Some(v) => c.args(["/v", v]),
        None => c.arg("/ve"),
    };
    let o = c.creation_flags(CREATE_NO_WINDOW).output().unwrap();
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

struct Names {
    channel: String,
    task: String,
    protocol_key: String,
    aumid_key: String,
    uninstall_key: String,
    install: PathBuf,
    data: PathBuf,
    shortcut: PathBuf,
}

impl Names {
    fn new() -> Names {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let channel = format!("insttest-{:x}{nanos:x}", std::process::id());
        assert_ne!(channel, "main");
        let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap());
        Names {
            task: agentbolt_ipc::windows_task::name(
                &agentbolt_core::channel::Names::for_channel(&channel).unwrap(),
            )
            .unwrap(),
            protocol_key: format!("Software\\Classes\\agentbolt-{channel}"),
            aumid_key: format!("Software\\Classes\\AppUserModelId\\Skulitom.Agentbolt.{channel}"),
            uninstall_key: agentbolt::install::uninstall_key(
                &agentbolt_core::channel::Names::for_channel(&channel).unwrap(),
            ),
            install: local.join("Programs").join(format!("Agentbolt-{channel}")),
            data: local.join("Agentbolt").join(&channel),
            shortcut: PathBuf::from(std::env::var("APPDATA").unwrap())
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join(format!("Agentbolt ({channel}).lnk")),
            channel,
        }
    }
}

/// Uninstalls on drop, then removes anything left by hand.
struct Cleanup<'a>(&'a Names);

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let n = self.0;
        let _ = run(&cli(), &["uninstall", "--channel", &n.channel, "--purge"]);
        let _ = schtasks(&["/End", "/TN", &n.task]);
        let _ = schtasks(&["/Delete", "/TN", &n.task, "/F"]);
        // Every fixture uses a unique test channel, including any legacy-name decoys it registers.
        let legacy = format!("Agentbolt ({})", n.channel);
        let _ = schtasks(&["/End", "/TN", &legacy]);
        let _ = schtasks(&["/Delete", "/TN", &legacy, "/F"]);
        for key in [&n.protocol_key, &n.aumid_key, &n.uninstall_key] {
            let _ = Command::new("reg.exe")
                .args(["delete", &format!("HKCU\\{key}"), "/f"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
        let _ = std::fs::remove_file(&n.shortcut);
        for dir in [&n.install, &n.data] {
            for _ in 0..20 {
                if !dir.exists() || std::fs::remove_dir_all(dir).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
    }
}

/// The folder to install from: this build's folder when it holds all three programs, otherwise a temporary
/// folder with stand-ins (then the task isn't started).
fn source(n: &Names) -> (PathBuf, bool) {
    let build = cli().parent().unwrap().to_path_buf();
    if ["agentbolt-daemon.exe", "agentbolt-desk.exe"]
        .iter()
        .all(|f| build.join(f).is_file())
    {
        return (build, true);
    }
    let tmp = std::env::temp_dir().join(format!("agentbolt-{}-src", n.channel));
    std::fs::create_dir_all(&tmp).unwrap();
    for f in [
        "agentbolt.exe",
        "agentbolt-daemon.exe",
        "agentbolt-desk.exe",
    ] {
        std::fs::copy(cli(), tmp.join(f)).unwrap();
    }
    (tmp, false)
}

/// Without LOCALAPPDATA or APPDATA, install and uninstall stop with a clear error before touching anything, and
/// never fall back to the current folder (where `uninstall --purge` would have deleted `.\Programs\...`).
/// Nothing is registered, so this runs everywhere.
#[test]
fn missing_folders_fail_clearly_and_delete_nothing() {
    let n = Names::new();
    let cwd = std::env::temp_dir().join(format!("agentbolt-{}-cwd", n.channel));
    let decoys = [
        cwd.join("Programs")
            .join(format!("Agentbolt-{}", n.channel)),
        cwd.join("Agentbolt").join(&n.channel),
    ];
    for d in &decoys {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("keep.txt"), "decoy").unwrap();
    }
    let from = cwd.join("from");
    std::fs::create_dir_all(&from).unwrap();
    for f in [
        "agentbolt.exe",
        "agentbolt-daemon.exe",
        "agentbolt-desk.exe",
    ] {
        std::fs::write(from.join(f), "stand-in").unwrap();
    }
    let from_s = from.display().to_string();
    let attempts: [(&str, Vec<&str>); 4] = [
        (
            "LOCALAPPDATA",
            vec!["uninstall", "--channel", &n.channel, "--purge"],
        ),
        (
            "APPDATA",
            vec!["uninstall", "--channel", &n.channel, "--purge"],
        ),
        (
            "LOCALAPPDATA",
            vec![
                "install",
                "--channel",
                &n.channel,
                "--from",
                &from_s,
                "--no-start",
            ],
        ),
        (
            "APPDATA",
            vec![
                "install",
                "--channel",
                &n.channel,
                "--from",
                &from_s,
                "--no-start",
            ],
        ),
    ];
    for (var, args) in attempts {
        let o = Command::new(cli())
            .args(&args)
            .env_remove("AGENTBOLT_CHANNEL")
            .env_remove(var)
            .current_dir(&cwd)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(1),
            "{var} unset, {args:?}: {}",
            text(&o)
        );
        assert!(
            text(&o).contains(&format!("{var} isn't set")),
            "{var} unset, {args:?}: {}",
            text(&o)
        );
        for d in &decoys {
            assert!(d.join("keep.txt").is_file(), "{} was touched", d.display());
        }
    }
    assert!(
        !n.install.exists(),
        "install with APPDATA unset created {}",
        n.install.display()
    );
    assert!(
        !schtasks(&["/Query", "/TN", &n.task]).status.success(),
        "a task was created"
    );
    std::fs::remove_dir_all(&cwd).unwrap();
}

#[test]
fn install_then_uninstall_on_a_throwaway_channel() {
    if std::env::var("AGENTBOLT_INSTALL_TEST").as_deref() != Ok("1") {
        eprintln!(
            "skipped: set AGENTBOLT_INSTALL_TEST=1 to register and remove a throwaway install"
        );
        return;
    }
    let n = Names::new();
    let _cleanup = Cleanup(&n);
    let (from, real) = source(&n);
    let desk_upgrade = real && std::env::var("AGENTBOLT_DESK_UPGRADE_TEST").as_deref() == Ok("1");
    let legacy = if real && std::env::var("AGENTBOLT_SCHEMA_UPGRADE_TEST").as_deref() == Ok("1") {
        assert!(
            desk_upgrade,
            "schema fixture requires the isolated Desk upgrade path"
        );
        Some(seed_expanded_schema13(&n))
    } else {
        None
    };
    let from_s = from.display().to_string();
    let mut args = vec!["install", "--channel", &n.channel, "--from", &from_s];
    if !real || desk_upgrade {
        args.push("--no-start");
    }
    let o = run(&cli(), &args);
    assert!(o.status.success(), "install failed: {}", text(&o));

    // The task, as Task Scheduler stored it.
    let q = schtasks(&["/Query", "/TN", &n.task, "/XML"]);
    assert!(q.status.success(), "the task wasn't created: {}", text(&q));
    let xml = String::from_utf8_lossy(&q.stdout).to_string();
    for needle in [
        "PT0S",
        "InteractiveToken",
        "IgnoreNew",
        "<Count>3</Count>",
        "PT1M",
        "LogonTrigger",
    ] {
        assert!(xml.contains(needle), "the stored task lacks {needle}");
    }
    assert!(xml.contains(&format!("--channel {}", n.channel)));

    // The registrations and the programs.
    assert_eq!(
        agentbolt::install::reg::get_string(&n.uninstall_key, Some("DisplayVersion")).as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    let uninstall = format!(
        "\"{}\" uninstall --channel {}",
        n.install.join("agentbolt.exe").display(),
        n.channel
    );
    for name in ["UninstallString", "QuietUninstallString"] {
        assert_eq!(
            agentbolt::install::reg::get_string(&n.uninstall_key, Some(name)),
            Some(uninstall.clone())
        );
    }
    let cmd = reg_query(&format!("{}\\shell\\open\\command", n.protocol_key), None)
        .expect("the protocol handler");
    assert!(
        cmd.contains("agentbolt-desk.exe")
            && cmd.contains(&format!("--channel {}", n.channel))
            && cmd.contains("%1"),
        "{cmd}"
    );
    assert!(reg_query(&n.protocol_key, Some("URL Protocol")).is_some());
    assert!(
        reg_query(&n.aumid_key, Some("DisplayName"))
            .expect("the AUMID")
            .contains("Agentbolt")
    );
    for f in [
        "agentbolt.exe",
        "agentbolt-daemon.exe",
        "agentbolt-desk.exe",
    ] {
        assert!(n.install.join(f).is_file(), "{f} wasn't copied");
    }

    // The Start-menu shortcut carries the AUMID (toasts from an unpackaged app need it).
    assert!(
        n.shortcut.is_file(),
        "no shortcut at {}",
        n.shortcut.display()
    );
    let (dir, file) = (
        n.shortcut.parent().unwrap().display().to_string(),
        n.shortcut
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    );
    let script = format!(
        "$i = (New-Object -ComObject Shell.Application).NameSpace('{dir}').ParseName('{file}'); \
         $i.ExtendedProperty('System.AppUserModel.ID'); \
         $l = (New-Object -ComObject WScript.Shell).CreateShortcut('{lnk}'); $l.Arguments; $l.TargetPath",
        lnk = n.shortcut.display()
    );
    let ps = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    let lines: Vec<String> = String::from_utf8_lossy(&ps.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .collect();
    assert_eq!(
        lines.first().map(String::as_str),
        Some(format!("Skulitom.Agentbolt.{}", n.channel).as_str()),
        "{lines:?}"
    );
    assert_eq!(
        lines.get(1).map(String::as_str),
        Some(format!("--channel {}", n.channel).as_str()),
        "{lines:?}"
    );
    assert!(
        lines
            .get(2)
            .is_some_and(|t| t.ends_with("agentbolt-desk.exe")),
        "{lines:?}"
    );

    let installed_cli = n.install.join("agentbolt.exe");
    let mut test_session = 0;
    unsafe {
        windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(
            std::process::id(),
            &mut test_session,
        )
        .unwrap();
    }
    let daemon_session = std::env::var("AGENTBOLT_TEST_DAEMON_SESSION")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(test_session);
    let core_names = agentbolt_core::channel::Names::for_channel(&n.channel).unwrap();
    if desk_upgrade {
        agentbolt::install::start_and_wait_in_session(
            &core_names,
            std::time::Duration::from_secs(20),
            Some(daemon_session),
        )
        .unwrap();
        args.retain(|a| *a != "--no-start");
    }
    if real {
        // install waited for this already; ask again through the installed copy.
        let s = run(&installed_cli, &["status", "--channel", &n.channel]);
        assert!(s.status.success(), "status: {}", text(&s));
        assert!(
            text(&s).contains(&format!("running on channel {}", n.channel)),
            "{}",
            text(&s)
        );
        if std::env::var("AGENTBOLT_RESTART_TEST").as_deref() == Ok("1") {
            check_crash_restart(&n, &installed_cli);
        }
    }

    // An upgrade while an agent session runs `agentbolt mcp` from the install folder (always, on Artem's PC): the
    // running exe can't be overwritten, so install renames it aside, and the daemon comes back up.
    if let Ok(legacy_cli) = std::env::var("AGENTBOLT_LEGACY_MCP_EXE") {
        std::fs::copy(legacy_cli, &installed_cli).unwrap();
    }
    let mut session = Mcp::start(&installed_cli, &n.channel);
    let mut ask_id = None;
    let mut draft_path = None;
    let mut _desk_launcher = None;
    if real {
        let registered = session.tool("desk_register", json!({}));
        let asked = session.tool("desk_ask", json!({"handle":registered["handle"],"kind":"choose", "title":"Upgrade keeps this ask", "body":"Synthetic installer fixture; leave this unanswered until after upgrade.", "options":["Yes","No"],"default":"No","deadline":(time::OffsetDateTime::now_utc()+time::Duration::days(1)).format(&time::format_description::well_known::Rfc3339).unwrap(),"reversible":true,"priority":"P2"}));
        ask_id = Some(asked["id"].as_str().unwrap().to_string());
    }
    if desk_upgrade {
        // Stop this fixture before writing its seed drafts; the next real Desk loads them at startup.
        let processes = agentbolt_ipc::windows_util::running_images(&[
            n.install.join("agentbolt-daemon.exe"),
            n.install.join("agentbolt-desk.exe"),
        ])
        .unwrap();
        assert!(schtasks(&["/End", "/TN", &n.task]).status.success());
        let started = std::time::Instant::now();
        while processes.iter().any(|p| !p.exited().unwrap()) {
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let id = ask_id.as_ref().unwrap();
        let path = n.data.join(format!("desk-drafts-{test_session}.json"));
        std::fs::write(&path, serde_json::to_vec(&json!({"version":1,"state":{"drafts":{id:{"choice":"Yes","note":"Survives installer upgrade","text":"","text_filled":false,"fields":null}},"selected":id,"batch":[id]}})).unwrap()).unwrap();
        draft_path = Some(path);
        agentbolt::install::start_and_wait_in_session(
            &core_names,
            std::time::Duration::from_secs(20),
            Some(daemon_session),
        )
        .unwrap();
        _desk_launcher = Some(TestProcess(
            Command::new(n.install.join("agentbolt-desk.exe"))
                .args(["--channel", &n.channel])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .unwrap(),
        ));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        loop {
            let mut visible = false;
            for p in
                agentbolt_ipc::windows_util::running_images(&[n.install.join("agentbolt-desk.exe")])
                    .unwrap()
            {
                if p.session != test_session {
                    continue;
                }
                visible = rt
                    .block_on(async {
                        let mut c = agentbolt_ipc::Client::connect_to(
                            &agentbolt_ipc::desk_control::endpoint(&core_names, p.pid),
                            agentbolt_ipc::Pipe::Mcp,
                            &[n.install.join("agentbolt-desk.exe")],
                        )
                        .await
                        .ok()?;
                        Some(
                            c.call(&agentbolt_ipc::desk_control::Op::Inspect)
                                .await
                                .ok()?["visible"]
                                == true,
                        )
                    })
                    .unwrap_or(false);
                if visible {
                    break;
                }
            }
            if visible {
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(20),
                "fixture Desk did not open"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    if real && std::env::var("AGENTBOLT_COPY_FAILURE_TEST").as_deref() == Ok("1") {
        use std::os::windows::fs::OpenOptionsExt;
        let broken = std::env::temp_dir().join(format!("agentbolt-{}-broken-source", n.channel));
        std::fs::create_dir_all(&broken).unwrap();
        let mut original = Vec::new();
        for exe in agentbolt::install::EXES {
            std::fs::copy(from.join(exe), broken.join(exe)).unwrap();
            original.push((exe, std::fs::read(n.install.join(exe)).unwrap()));
        }
        let processes = agentbolt_ipc::windows_util::running_images(&[
            n.install.join("agentbolt-daemon.exe"),
            n.install.join("agentbolt-desk.exe"),
        ])
        .unwrap();
        // The third source exists but cannot be read: fail after earlier files have been replaced.
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(broken.join("agentbolt-desk.exe"))
            .unwrap();
        let failed = run(
            &cli(),
            &[
                "install",
                "--channel",
                &n.channel,
                "--from",
                &broken.display().to_string(),
            ],
        );
        drop(locked);
        std::fs::remove_dir_all(&broken).unwrap();
        assert!(!failed.status.success(), "copy failure was ignored");
        assert!(
            text(&failed).contains("old programs were restored"),
            "{}",
            text(&failed)
        );
        for (exe, bytes) in original {
            assert!(
                std::fs::read(n.install.join(exe)).unwrap() == bytes,
                "{exe} not restored"
            );
        }
        assert!(
            processes.iter().all(|p| !p.exited().unwrap()),
            "copy failure stopped a prepared process"
        );
        assert!(
            run(&installed_cli, &["status", "--channel", &n.channel])
                .status
                .success()
        );
        eprintln!(
            "Third-file copy failure restored all original bytes and kept existing processes alive"
        );
    }
    let o = run(&cli(), &args);
    assert!(
        o.status.success(),
        "the reinstall with a session open failed: {}",
        text(&o)
    );
    for f in [
        "agentbolt.exe",
        "agentbolt-daemon.exe",
        "agentbolt-desk.exe",
    ] {
        assert!(
            n.install.join(f).is_file(),
            "{f} is missing after the reinstall"
        );
    }
    let old: Vec<String> = std::fs::read_dir(&n.install)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| f.contains(".old-"))
        .collect();
    assert!(
        old.iter().any(|f| f.starts_with("agentbolt.exe.old-")),
        "the running exe was renamed aside: {old:?}"
    );
    if real {
        let s = run(&installed_cli, &["status", "--channel", &n.channel]);
        assert!(
            s.status.success(),
            "the daemon isn't back after the reinstall: {}",
            text(&s)
        );
        let registered = session.tool("desk_register", json!({}));
        let id = ask_id.as_ref().unwrap();
        assert!(
            registered["open_asks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["id"] == *id)
        );
        if let Some(path) = &draft_path {
            let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(saved["state"]["drafts"][id]["choice"], "Yes");
            assert_eq!(
                saved["state"]["drafts"][id]["note"],
                "Survives installer upgrade"
            );
        }
        let answer = run(
            &n.install.join("agentbolt-desk.exe"),
            &["--channel", &n.channel, "--answer", id, "--choice", "Yes"],
        );
        assert!(
            answer.status.success(),
            "new Desk answer failed: {}",
            text(&answer)
        );
        let answers = session.tool(
            "desk_answers",
            json!({"handle":registered["handle"],"ids":[id]}),
        );
        assert_eq!(answers["answers"][0]["answer"]["choice"], "Yes");
        if let Some(legacy) = &legacy {
            let messages = session.tool("board_read", json!({"after":0}));
            let message = messages["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["id"] == legacy.message)
                .expect("legacy Board message survived");
            assert!(message["sender_machine"].is_null());
            assert_eq!(message["body"], "Preserve this older handoff");
            assert_eq!(message["references"], json!(["C:/synthetic/evidence.md"]));
            verify_expanded_schema_upgrade(&n, legacy);
        }
        eprintln!(
            "MCP reconnect, retained ask and new Desk answer verified; visible Desk test={desk_upgrade}, Desk session={test_session}, daemon session={daemon_session}, channel {}",
            n.channel
        );
    }

    // Uninstall with the session still open: the running exe is parked in %TEMP%, the folder still goes.
    let o = run(&cli(), &["uninstall", "--channel", &n.channel, "--purge"]);
    let _ = session.child.kill();
    let _ = session.child.wait();
    assert!(o.status.success(), "uninstall failed: {}", text(&o));
    assert!(
        text(&o).contains("claude mcp remove"),
        "uninstall prints the client commands: {}",
        text(&o)
    );
    let parked: Vec<PathBuf> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|f| f.to_string_lossy().starts_with("agentbolt-uninstalled-"))
        })
        .collect();
    for p in &parked {
        let _ = std::fs::remove_file(p);
    }
    assert!(
        !schtasks(&["/Query", "/TN", &n.task]).status.success(),
        "the task is still there"
    );
    assert!(
        reg_query(&n.protocol_key, Some("URL Protocol")).is_none(),
        "the protocol key is still there"
    );
    assert!(
        reg_query(&n.aumid_key, Some("DisplayName")).is_none(),
        "the AUMID key is still there"
    );
    assert!(!n.install.exists(), "the install folder is still there");
    assert!(
        reg_query(&n.uninstall_key, Some("DisplayName")).is_none(),
        "the Installed Apps entry is still there"
    );
    assert!(!n.data.exists(), "the data folder is still there");
    assert!(!n.shortcut.exists(), "the shortcut is still there");
    if !real {
        let _ = std::fs::remove_dir_all(&from);
    }
}

struct LegacyExpanded {
    answer: String,
    message: String,
    baseline: String,
}

/// A schema-13-shaped snapshot, prepared before any installed process starts. It is deliberately not
/// described as an older executable run: the installed daemon must perform migrations 14 and 15.
fn seed_expanded_schema13(n: &Names) -> LegacyExpanded {
    use agentbolt_core::{
        board::{BoardOp, PostInput},
        model::{AskInput, AskKind, Priority},
        protocol::{AskResult, DeskOp, McpOp, RegisterResult},
        service::{ClientInfo, Service},
        service_impl::StoreService,
    };
    assert!(!n.data.exists(), "legacy fixture data must be fresh");
    let path = n.data.join("agentbolt.db");
    let before = time::OffsetDateTime::now_utc() - time::Duration::minutes(5);
    let service = StoreService::open(&path)
        .unwrap()
        .with_channel(&n.channel)
        .with_machine("legacy-fixture")
        .with_clock(std::sync::Arc::new(move || before));
    let call = |op| service.mcp(&ClientInfo::default(), op).unwrap().0;
    let owner: RegisterResult = serde_json::from_value(call(McpOp::Register {
        owner: Some("schema-migration-fixture".into()),
        client: "Schema fixture".into(),
        session_ref: None,
        cwd: None,
        doing: None,
        provisional_key: None,
    }))
    .unwrap();
    let ask: AskResult = serde_json::from_value(call(McpOp::Ask {
        handle: owner.handle.clone(),
        ask: AskInput {
            kind: AskKind::Choose,
            title: "Older measured question".into(),
            body: "Synthetic schema migration history".into(),
            evidence: vec![],
            options: vec!["Keep".into(), "Change".into()],
            fields: vec![],
            default: json!("Keep"),
            deadline: (before + time::Duration::days(1))
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            reversible: true,
            priority: Priority::P2,
            p0_reason: None,
            dedupe_key: None,
        },
    }))
    .unwrap();
    service.desk(DeskOp::List).unwrap();
    service
        .desk(DeskOp::Answer {
            ask_id: ask.id.clone(),
            choice: Some("Keep".into()),
            values: None,
            note: Some("Synthetic saved answer".into()),
            one_click: None,
        })
        .unwrap();
    let post = call(McpOp::Board {
        handle: owner.handle,
        action: BoardOp::Post {
            input: PostInput {
                to: Default::default(),
                title: "Older Board reference".into(),
                body: "Preserve this older handoff".into(),
                references: vec!["C:/synthetic/evidence.md".into()],
                thread: None,
                dedupe_key: None,
            },
        },
    });
    service
        .desk(DeskOp::StartBaseline {
            label: "Synthetic old baseline".into(),
        })
        .unwrap();
    drop(service);
    let db = agentbolt_core::store::open_file(&path).unwrap();
    let old: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='adoption_baseline'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut baseline: Value = serde_json::from_str(&old).unwrap();
    baseline["period"]
        .as_object_mut()
        .unwrap()
        .remove("one_click");
    let baseline = baseline.to_string();
    db.execute(
        "UPDATE meta SET value=?1 WHERE key='adoption_baseline'",
        [&baseline],
    )
    .unwrap();
    db.execute_batch("ALTER TABLE board_messages DROP COLUMN sender_machine; ALTER TABLE answers DROP COLUMN one_click; PRAGMA user_version=13;").unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        13
    );
    eprintln!(
        "Prepared schema 13 fixture with older answer, Board reference and baseline on {}",
        n.channel
    );
    LegacyExpanded {
        answer: ask.id,
        message: post["id"].as_str().unwrap().into(),
        baseline,
    }
}

fn verify_expanded_schema_upgrade(n: &Names, legacy: &LegacyExpanded) {
    // Stop only this installation's verified images before inspecting its sole-writer database.
    let processes = agentbolt_ipc::windows_util::running_images(&[
        n.install.join("agentbolt-daemon.exe"),
        n.install.join("agentbolt-desk.exe"),
    ])
    .unwrap();
    assert!(schtasks(&["/End", "/TN", &n.task]).status.success());
    let start = std::time::Instant::now();
    while processes.iter().any(|p| !p.exited().unwrap()) {
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let path = n.data.join("agentbolt.db");
    // Open with SQLite, not store::open_file: the verifier must not itself apply the migrations.
    let db =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        15
    );
    let (choice, direct): (String, Option<bool>) = db
        .query_row(
            "SELECT choice,one_click FROM answers WHERE ask_id=?1",
            [&legacy.answer],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(choice, "Keep");
    assert_eq!(direct, None);
    let origin: Option<String> = db
        .query_row(
            "SELECT sender_machine FROM board_messages WHERE id=?1",
            [&legacy.message],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(origin, None);
    let baseline: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='adoption_baseline'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        baseline, legacy.baseline,
        "the saved baseline must remain byte-for-byte unchanged"
    );
    let report = agentbolt_core::metrics::report(
        &db,
        (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64,
    )
    .unwrap();
    assert!(report.baseline.unwrap().period.one_click.is_none());
    let direct = report.current.one_click.unwrap();
    assert!(direct.unmeasured_human_answers >= 1);
    assert!(direct.share.is_none());
    eprintln!(
        "Installed schema 13 -> 15 migration and subsequent reinstall preserved answer/Board history, unknown evidence and exact baseline on {}",
        n.channel
    );
}

fn register_task_xml(n: &Names, name: &str, xml: &str) {
    let path = std::env::temp_dir().join(format!("agentbolt-{}-task-fixture.xml", n.channel));
    let mut bytes = vec![0xff, 0xfe];
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(&path, bytes).unwrap();
    let result = schtasks(&[
        "/Create",
        "/TN",
        name,
        "/XML",
        &path.display().to_string(),
        "/F",
    ]);
    std::fs::remove_file(path).unwrap();
    assert!(result.status.success(), "{}", text(&result));
}

#[test]
fn scheduler_migrates_only_a_verified_legacy_task_and_refuses_scoped_decoys() {
    if std::env::var("AGENTBOLT_INSTALL_TEST").as_deref() != Ok("1") {
        eprintln!("skipped: set AGENTBOLT_INSTALL_TEST=1 for Task Scheduler fixtures");
        return;
    }
    let n = Names::new();
    let _cleanup = Cleanup(&n);
    let (from, real) = source(&n);
    let core_names = agentbolt_core::channel::Names::for_channel(&n.channel).unwrap();
    let args = [
        "install",
        "--channel",
        &n.channel,
        "--from",
        &from.display().to_string(),
        "--no-start",
    ];
    let initial = run(&cli(), &args);
    assert!(initial.status.success(), "{}", text(&initial));
    let xml = String::from_utf8(schtasks(&["/Query", "/TN", &n.task, "/XML"]).stdout).unwrap();
    let legacy = core_names.task.clone();
    register_task_xml(&n, &legacy, &xml);
    assert!(
        schtasks(&["/Delete", "/TN", &n.task, "/F"])
            .status
            .success()
    );
    let before = agentbolt_ipc::windows_task::registrations(&core_names).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].name, legacy);
    let migrated = run(&cli(), &args);
    assert!(migrated.status.success(), "{}", text(&migrated));
    assert!(text(&migrated).contains("Migrated the verified historical task"));
    assert!(!schtasks(&["/Query", "/TN", &legacy]).status.success());
    assert_eq!(
        agentbolt_ipc::windows_task::registrations(&core_names).unwrap()[0].name,
        n.task
    );

    if real {
        use std::os::windows::fs::OpenOptionsExt;
        let blocked = std::env::temp_dir().join(format!("agentbolt-{}-delayed-copy", n.channel));
        std::fs::create_dir_all(&blocked).unwrap();
        for exe in agentbolt::install::EXES {
            std::fs::copy(from.join(exe), blocked.join(exe)).unwrap();
        }
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(blocked.join("agentbolt-desk.exe"))
            .unwrap();
        let installer = Command::new(cli())
            .args([
                "install",
                "--channel",
                &n.channel,
                "--from",
                &blocked.display().to_string(),
                "--no-start",
            ])
            .env("AGENTBOLT_CHANNEL", &n.channel)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        while !std::fs::read_dir(&n.install).unwrap().flatten().any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("agentbolt-desk.exe.old-")
        }) {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "installer did not reach copying"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // Launch the newly copied worker/supervisor directly, as Task Scheduler or an old host could.
        // Both must fail transiently before opening a store. This is independent of CLI coordination.
        for supervise in [false, true] {
            let mut cmd = Command::new(n.install.join("agentbolt-daemon.exe"));
            cmd.args(["--channel", &n.channel, "--headless"]);
            if supervise {
                cmd.arg("--supervise");
            }
            let mut process = TestProcess(cmd.creation_flags(CREATE_NO_WINDOW).spawn().unwrap());
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                if let Some(status) = process.0.try_wait().unwrap() {
                    assert_eq!(
                        status.code(),
                        Some(1),
                        "maintenance must be a transient startup failure"
                    );
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "a new daemon entered an uncommitted installation"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        assert!(
            !core_names.db_path().unwrap().exists(),
            "uncommitted daemon opened the database"
        );
        // Change the task after preflight. Rollback must not overwrite this unrelated definition.
        register_task_xml(&n, &n.task, &xml.replace("--supervise", "--unrelated"));
        let changed = schtasks(&["/Query", "/TN", &n.task, "/XML"]).stdout;
        drop(locked);
        let result = installer.wait_with_output().unwrap();
        assert!(!result.status.success(), "changed task was accepted");
        assert!(
            text(&result).contains("left untouched"),
            "{}",
            text(&result)
        );
        assert_eq!(
            schtasks(&["/Query", "/TN", &n.task, "/XML"]).stdout,
            changed,
            "rollback replaced an unrelated task"
        );
        for exe in agentbolt::install::EXES {
            assert!(
                std::fs::read(from.join(exe)).unwrap()
                    == std::fs::read(n.install.join(exe)).unwrap(),
                "rollback failed for {exe}"
            );
        }
        std::fs::remove_dir_all(blocked).unwrap();
        register_task_xml(&n, &n.task, &xml);
        eprintln!(
            "Uncommitted worker/supervisor refused before database creation; rollback preserved a concurrently changed task"
        );
    }

    // Same label and account do not grant ownership: require exactly our action, folder and arguments.
    for decoy in [
        xml.replace("agentbolt-daemon.exe", "unrelated.exe"),
        xml.replace("--supervise", "--unrelated"),
        xml.replace("</WorkingDirectory>", "-unrelated</WorkingDirectory>"),
    ] {
        register_task_xml(&n, &legacy, &decoy);
        let stored = schtasks(&["/Query", "/TN", &legacy, "/XML"]).stdout;
        let upgraded = run(&cli(), &args);
        assert!(upgraded.status.success(), "{}", text(&upgraded));
        assert_eq!(
            schtasks(&["/Query", "/TN", &legacy, "/XML"]).stdout,
            stored,
            "unrelated historical task changed"
        );
        let ours = agentbolt_ipc::windows_task::registrations(&core_names).unwrap();
        assert_eq!(ours.len(), 1);
        assert_eq!(ours[0].name, n.task);
    }
    let good_bytes = std::fs::read(n.install.join("agentbolt.exe")).unwrap();
    register_task_xml(
        &n,
        &n.task,
        &xml.replace("agentbolt-daemon.exe", "unrelated.exe"),
    );
    let collision = schtasks(&["/Query", "/TN", &n.task, "/XML"]).stdout;
    for operation in ["install", "uninstall", "start"] {
        let result = if operation == "install" {
            run(&cli(), &args)
        } else {
            run(&cli(), &[operation, "--channel", &n.channel])
        };
        assert!(
            !result.status.success(),
            "{operation} accepted an unrelated scoped task"
        );
        assert!(
            text(&result).contains("left untouched"),
            "{}",
            text(&result)
        );
        assert_eq!(
            schtasks(&["/Query", "/TN", &n.task, "/XML"]).stdout,
            collision
        );
        assert_eq!(
            std::fs::read(n.install.join("agentbolt.exe")).unwrap(),
            good_bytes
        );
    }
    // Restore the fixture's own valid task so the normal uninstall cleans up its files and registrations.
    register_task_xml(&n, &n.task, &xml);
    let foreign = schtasks(&["/Query", "/TN", &legacy, "/XML"]).stdout;
    let removed = run(&cli(), &["uninstall", "--channel", &n.channel, "--purge"]);
    assert!(removed.status.success(), "{}", text(&removed));
    assert_eq!(
        schtasks(&["/Query", "/TN", &legacy, "/XML"]).stdout,
        foreign
    );
    assert!(!n.install.exists() && !n.data.exists() && !n.shortcut.exists());
    assert!(
        schtasks(&["/Delete", "/TN", &legacy, "/F"])
            .status
            .success()
    );
    if !real {
        std::fs::remove_dir_all(from).unwrap();
    }
    eprintln!(
        "Verified legacy migration, unrelated-task preservation and scoped-name refusal on {}",
        n.channel
    );
}
