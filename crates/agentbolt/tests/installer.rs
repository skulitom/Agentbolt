//! `agentbolt install` / `uninstall` on a throwaway channel: the task, the HKCU keys and the install folder
//! appear, the daemon answers `status` (when a daemon build sits next to this one), and uninstall removes it all.
//!
//! It registers real per-user things (a scheduled task, HKCU keys, a folder under %LOCALAPPDATA%\Programs), so
//! it runs only when `AGENTBOLT_INSTALL_TEST=1`. It never touches the `main` channel, and a guard uninstalls and
//! then deletes leftovers directly, even when an assertion fails.
#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn cli() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agentbolt"))
}

fn run(exe: &Path, args: &[&str]) -> Output {
    Command::new(exe).args(args).env_remove("AGENTBOLT_CHANNEL").creation_flags(CREATE_NO_WINDOW).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn schtasks(args: &[&str]) -> Output {
    Command::new("schtasks.exe").args(args).creation_flags(CREATE_NO_WINDOW).output().unwrap()
}

fn reg_query(key: &str, value: Option<&str>) -> Option<String> {
    let mut c = Command::new("reg.exe");
    c.args(["query", &format!("HKCU\\{key}")]);
    match value {
        Some(v) => c.args(["/v", v]),
        None => c.arg("/ve"),
    };
    let o = c.creation_flags(CREATE_NO_WINDOW).output().unwrap();
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

struct Names {
    channel: String,
    task: String,
    protocol_key: String,
    aumid_key: String,
    install: PathBuf,
    data: PathBuf,
    shortcut: PathBuf,
}

impl Names {
    fn new() -> Names {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
        let channel = format!("insttest-{:x}{nanos:x}", std::process::id());
        assert_ne!(channel, "main");
        let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap());
        Names {
            task: format!("Agentbolt ({channel})"),
            protocol_key: format!("Software\\Classes\\agentbolt-{channel}"),
            aumid_key: format!("Software\\Classes\\AppUserModelId\\Skulitom.Agentbolt.{channel}"),
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
        for key in [&n.protocol_key, &n.aumid_key] {
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
    if ["agentbolt-daemon.exe", "agentbolt-desk.exe"].iter().all(|f| build.join(f).is_file()) {
        return (build, true);
    }
    let tmp = std::env::temp_dir().join(format!("agentbolt-{}-src", n.channel));
    std::fs::create_dir_all(&tmp).unwrap();
    for f in ["agentbolt.exe", "agentbolt-daemon.exe", "agentbolt-desk.exe"] {
        std::fs::copy(cli(), tmp.join(f)).unwrap();
    }
    (tmp, false)
}

#[test]
fn install_then_uninstall_on_a_throwaway_channel() {
    if std::env::var("AGENTBOLT_INSTALL_TEST").as_deref() != Ok("1") {
        eprintln!("skipped: set AGENTBOLT_INSTALL_TEST=1 to register and remove a throwaway install");
        return;
    }
    let n = Names::new();
    let _cleanup = Cleanup(&n);
    let (from, real) = source(&n);
    let from_s = from.display().to_string();
    let mut args = vec!["install", "--channel", &n.channel, "--from", &from_s];
    if !real {
        args.push("--no-start");
    }
    let o = run(&cli(), &args);
    assert!(o.status.success(), "install failed: {}", text(&o));

    // The task, as Task Scheduler stored it.
    let q = schtasks(&["/Query", "/TN", &n.task, "/XML"]);
    assert!(q.status.success(), "the task wasn't created: {}", text(&q));
    let xml = String::from_utf8_lossy(&q.stdout).to_string();
    for needle in ["PT0S", "InteractiveToken", "IgnoreNew", "<Count>3</Count>", "PT1M", "LogonTrigger"] {
        assert!(xml.contains(needle), "the stored task lacks {needle}");
    }
    assert!(xml.contains(&format!("--channel {}", n.channel)));

    // The registrations and the programs.
    let cmd = reg_query(&format!("{}\\shell\\open\\command", n.protocol_key), None).expect("the protocol handler");
    assert!(cmd.contains("agentbolt-desk.exe") && cmd.contains(&format!("--channel {}", n.channel)) && cmd.contains("%1"), "{cmd}");
    assert!(reg_query(&n.protocol_key, Some("URL Protocol")).is_some());
    assert!(reg_query(&n.aumid_key, Some("DisplayName")).expect("the AUMID").contains("Agentbolt"));
    for f in ["agentbolt.exe", "agentbolt-daemon.exe", "agentbolt-desk.exe"] {
        assert!(n.install.join(f).is_file(), "{f} wasn't copied");
    }

    // The Start-menu shortcut carries the AUMID (toasts from an unpackaged app need it).
    assert!(n.shortcut.is_file(), "no shortcut at {}", n.shortcut.display());
    let (dir, file) = (n.shortcut.parent().unwrap().display().to_string(), n.shortcut.file_name().unwrap().to_string_lossy().to_string());
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
    let lines: Vec<String> = String::from_utf8_lossy(&ps.stdout).lines().map(|l| l.trim().to_string()).collect();
    assert_eq!(lines.first().map(String::as_str), Some(format!("Skulitom.Agentbolt.{}", n.channel).as_str()), "{lines:?}");
    assert_eq!(lines.get(1).map(String::as_str), Some(format!("--channel {}", n.channel).as_str()), "{lines:?}");
    assert!(lines.get(2).is_some_and(|t| t.ends_with("agentbolt-desk.exe")), "{lines:?}");

    if real {
        // install waited for this already; ask again through the installed copy.
        let s = run(&n.install.join("agentbolt.exe"), &["status", "--channel", &n.channel]);
        assert!(s.status.success(), "status: {}", text(&s));
        assert!(text(&s).contains(&format!("running on channel {}", n.channel)), "{}", text(&s));
    }

    let o = run(&cli(), &["uninstall", "--channel", &n.channel, "--purge"]);
    assert!(o.status.success(), "uninstall failed: {}", text(&o));
    assert!(!schtasks(&["/Query", "/TN", &n.task]).status.success(), "the task is still there");
    assert!(reg_query(&n.protocol_key, Some("URL Protocol")).is_none(), "the protocol key is still there");
    assert!(reg_query(&n.aumid_key, Some("DisplayName")).is_none(), "the AUMID key is still there");
    assert!(!n.install.exists(), "the install folder is still there");
    assert!(!n.data.exists(), "the data folder is still there");
    assert!(!n.shortcut.exists(), "the shortcut is still there");
    if !real {
        let _ = std::fs::remove_dir_all(&from);
    }
}
