//! The Desk's headless modes, without a window and without a daemon.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn desk() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_agentbolt-desk"));
    c.env_remove("AGENTBOLT_CHANNEL");
    c
}

fn throwaway_channel() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("desktest-{:x}-{nanos:x}", std::process::id())
}

#[test]
fn the_test_answer_refuses_the_main_channel() {
    for args in [
        vec!["--channel", "main", "--answer", "a1", "--choice", "Ship"],
        vec!["--answer", "a1", "--choice", "Ship"],
    ] {
        let out = desk().args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("test channels only"), "{args:?}: {err}");
    }
    let out = desk()
        .env("AGENTBOLT_CHANNEL", "main")
        .args(["--answer", "a1", "--choice", "x"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn the_test_answer_without_a_daemon_says_it_isnt_running() {
    let out = desk()
        .args([
            "--channel",
            &throwaway_channel(),
            "--answer",
            "a1",
            "--choice",
            "Ship",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("isn't running"), "{err}");
}

#[test]
fn bad_arguments_exit_without_a_window() {
    let out = desk().args(["--answer", "a1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--choice"));
    let out = desk()
        .args(["--channel", "Bad Name", "--answer", "a1", "--choice", "x"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
