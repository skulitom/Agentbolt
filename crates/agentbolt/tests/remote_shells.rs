//! Execute the SSH command through real local shells with a space-containing executable path.
//! Empty input makes the relay exit before contacting or starting any daemon.
use agentbolt::remote::{Config, Shell};
use agentbolt_core::channel::Names;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[path = "../src/testutil.rs"]
mod testutil;

fn exercise(shell: Shell, executable: &Path, channel: &str, mut command: Command) {
    let config = Config {
        host: "unused.invalid".into(),
        machine: "shell-fixture".into(),
        executable: executable.to_str().unwrap().into(),
        shell,
        config_file: None,
    };
    let script = config.command(channel).unwrap();
    let names = Names::for_channel(channel).unwrap();
    assert!(!names.data_dir().unwrap().exists());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
        if shell == Shell::Cmd {
            // cmd /S removes this outer quote pair, leaving the validated executable quotes intact.
            command.raw_arg(format!("\"{script}\""));
        } else {
            command.arg(&script);
        }
    }
    #[cfg(not(windows))]
    command.arg(&script);
    let mut child = command
        .env("AGENTBOLT_CHANNEL", channel)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{shell:?} did not finish the empty relay");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{shell:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "{shell:?} added text to the relay stream"
    );
    assert!(
        output.stderr.is_empty(),
        "{shell:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !names.data_dir().unwrap().exists(),
        "empty relay must not start a daemon"
    );
}

#[test]
fn relay_launches_from_a_path_with_spaces_in_supported_shells() {
    let temp = testutil::TempDir::new();
    let dir = temp.path().join("Desk install with spaces");
    std::fs::create_dir(&dir).unwrap();
    let executable = dir.join(if cfg!(windows) {
        "agentbolt.exe"
    } else {
        "agentbolt"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_agentbolt"), &executable).unwrap();
    let channel = format!("shell-launch-{}", std::process::id());
    #[cfg(windows)]
    {
        let schtasks = agentbolt_ipc::windows_util::schtasks().unwrap();
        let system = schtasks.parent().unwrap();
        let mut cmd = Command::new(system.join("cmd.exe"));
        cmd.args(["/D", "/S", "/C"]);
        exercise(Shell::Cmd, &executable, &channel, cmd);
        let mut powershell = Command::new(system.join("WindowsPowerShell/v1.0/powershell.exe"));
        powershell.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]);
        exercise(Shell::PowerShell, &executable, &channel, powershell);
    }
    #[cfg(unix)]
    {
        let mut sh = Command::new("/bin/sh");
        sh.arg("-c");
        exercise(Shell::Sh, &executable, &channel, sh);
    }
}
