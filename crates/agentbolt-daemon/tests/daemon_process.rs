//! The real `agentbolt-daemon.exe`, end to end, on a throwaway channel.
//!
//! The daemon is copied into a temp folder together with a copy of this test binary named `agentbolt-desk.exe`,
//! so the Desk pipe's image check can be passed by a "Desk" that is really this test (run as a child, see
//! `fake_desk_child`) and failed by this test process itself. The daemon runs `--headless` (no tray, no toasts)
//! with `LOCALAPPDATA` pointed into the temp folder, so nothing touches a real install, and it is killed and the
//! folder removed at the end. Windows-only: it exercises named pipes, the mutex and image paths.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_core::model::{AskInput, AskKind, Priority};
use agentbolt_core::protocol::{DeskOp, McpOp};
use agentbolt_ipc::{Client, IpcError, Pipe};
use serde_json::json;

const FAKE_DESK_ASK: &str = "AGENTBOLT_TEST_FAKE_DESK_ASK";
const FAKE_DESK_CHANNEL: &str = "AGENTBOLT_TEST_FAKE_DESK_CHANNEL";

fn random_hex() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    format!("{:012x}", h.finish() & 0xffff_ffff_ffff)
}

struct Setup {
    names: Names,
    dir: PathBuf,
    daemons: Vec<Child>,
}

impl Setup {
    fn new() -> Setup {
        let names = Names::for_channel(&format!("ipctest-{}", random_hex())).unwrap();
        assert!(!names.is_main());
        let dir = std::env::temp_dir().join(format!("agentbolt-{}", names.channel));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_agentbolt-daemon"), dir.join("agentbolt-daemon.exe")).unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), dir.join("agentbolt-desk.exe")).unwrap();
        Setup { names, dir, daemons: Vec::new() }
    }

    fn daemon_exe(&self) -> PathBuf {
        self.dir.join("agentbolt-daemon.exe")
    }

    fn spawn_daemon(&self) -> Child {
        Command::new(self.daemon_exe())
            .args(["--channel", &self.names.channel, "--headless"])
            .env("LOCALAPPDATA", self.dir.join("data"))
            .env_remove("AGENTBOLT_CHANNEL")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the daemon")
    }

    fn log(&self) -> String {
        let path = self.dir.join("data").join("Agentbolt").join(&self.names.channel).join("agentbolt.log");
        std::fs::read_to_string(path).unwrap_or_default()
    }

    async fn connect(&self, pipe: Pipe) -> Result<Client, IpcError> {
        Client::connect_to(&self.names, pipe, &[self.daemon_exe()]).await
    }

    async fn connect_when_up(&self) -> Client {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.connect(Pipe::Mcp).await {
                Ok(client) => return client,
                Err(IpcError::NotRunning(_)) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                Err(e) => panic!("the daemon didn't come up: {e}\nlog:\n{}", self.log()),
            }
        }
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        for child in &mut self.daemons {
            let _ = child.kill();
            let _ = child.wait();
        }
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.dir).is_ok() || !self.dir.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn wait_exit(child: &mut Child, within: Duration) -> Option<i32> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return status.code();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn choose_ask() -> AskInput {
    let deadline = time::OffsetDateTime::now_utc() + time::Duration::hours(6);
    AskInput {
        kind: AskKind::Choose,
        title: "Pick a colour for the test".into(),
        body: "Throwaway ask from the daemon process test.".into(),
        evidence: vec![],
        options: vec!["Red".into(), "Blue".into()],
        fields: vec![],
        default: json!("Red"),
        deadline: deadline.format(&time::format_description::well_known::Rfc3339).unwrap(),
        reversible: true,
        priority: Priority::P1,
        p0_reason: None,
        dedupe_key: None,
    }
}

/// With LOCALAPPDATA unset the daemon has no data folder: it stops (exit 1, the task's retries show it) and never
/// falls back to a store and log under the current folder.
#[test]
fn without_localappdata_the_daemon_stops_and_writes_nothing() {
    let setup = Setup::new();
    let mut child = Command::new(setup.daemon_exe())
        .args(["--channel", &setup.names.channel, "--headless"])
        .env_remove("LOCALAPPDATA")
        .env_remove("AGENTBOLT_CHANNEL")
        .current_dir(&setup.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the daemon");
    let code = wait_exit(&mut child, Duration::from_secs(10));
    if code.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert_eq!(code, Some(1), "the daemon must stop without a data folder");
    assert!(!setup.dir.join("Agentbolt").exists(), "it made a data folder under the current folder");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_serves_refuses_and_waits() {
    let mut setup = Setup::new();
    let first = setup.spawn_daemon();
    setup.daemons.push(first);
    let mut mcp = setup.connect_when_up().await;

    // Status over the MCP pipe, from the real store.
    let status = mcp.call(&McpOp::Status).await.expect("status");
    assert_eq!(status["channel"], json!(setup.names.channel), "{status}");

    // A second daemon on the channel exits 0 at once (the Global\ mutex), and the first keeps serving.
    let mut second = setup.spawn_daemon();
    assert_eq!(wait_exit(&mut second, Duration::from_secs(15)), Some(0), "log:\n{}", setup.log());
    mcp.call(&McpOp::Status).await.expect("the first daemon still serves");

    // A client whose expected daemon is elsewhere (cargo's copy) refuses this one: the image check is real.
    match Client::connect(&setup.names, Pipe::Mcp).await {
        Err(IpcError::WrongServer(..)) => {}
        Err(e) => panic!("expected WrongServer, got {e}"),
        Ok(_) => panic!("accepted a daemon from another folder"),
    }

    // This test process isn't agentbolt-desk.exe: the Desk pipe drops it before reading.
    let mut fake = setup.connect(Pipe::Desk).await.expect("the pipe opens");
    assert!(fake.call(&DeskOp::Status).await.is_err(), "a fake Desk got a reply");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !setup.log().contains("dropped a client") {
        assert!(Instant::now() < deadline, "no refusal logged:\n{}", setup.log());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Register, ask, and wait for the answer while the "installed Desk" (a copy of this binary) answers it.
    let reg = McpOp::Register {
        owner: Some("ipctest-owner".into()),
        client: "daemon-process-test".into(),
        session_ref: None,
        cwd: None,
        doing: Some("testing".into()),
        provisional_key: None,
    };
    let handle = mcp.call(&reg).await.expect("register")["handle"].as_str().unwrap().to_string();
    let asked = mcp.call(&McpOp::Ask { handle: handle.clone(), ask: choose_ask() }).await.expect("ask");
    let ask_id = asked["id"].as_str().unwrap().to_string();

    // Nothing answered yet: a short wait runs out and reports silence as silence.
    let started = Instant::now();
    let none = mcp.call(&McpOp::Answers { handle: handle.clone(), ids: vec![ask_id.clone()], wait_ms: 800 }).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(750), "it waited");
    assert_eq!(none["answers"], json!([]), "{none}");

    let waiter = {
        let names = setup.names.clone();
        let daemon = setup.daemon_exe();
        let (handle, ask_id) = (handle.clone(), ask_id.clone());
        tokio::spawn(async move {
            let mut client = Client::connect_to(&names, Pipe::Mcp, &[daemon]).await.unwrap();
            let started = Instant::now();
            let result = client.call(&McpOp::Answers { handle, ids: vec![ask_id], wait_ms: 60_000 }).await;
            (result, started.elapsed())
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    let desk = Command::new(setup.dir.join("agentbolt-desk.exe"))
        .args(["fake_desk_child", "--exact", "--test-threads=1", "--nocapture"])
        .env(FAKE_DESK_ASK, &ask_id)
        .env(FAKE_DESK_CHANNEL, &setup.names.channel)
        .output()
        .expect("run the fake desk");
    assert!(desk.status.success(), "fake desk failed:\n{}", String::from_utf8_lossy(&desk.stdout));

    let (result, took) = waiter.await.unwrap();
    let result = result.expect("answers");
    assert_eq!(result["answers"][0]["id"], json!(ask_id), "{result}");
    assert_eq!(result["answers"][0]["answer"]["choice"], json!("Blue"), "{result}");
    assert!(took < Duration::from_secs(20), "returned when the answer landed, not at 60 s: {took:?}");
    assert!(!setup.log().contains("ERROR"), "log:\n{}", setup.log());
    println!("daemon log:\n{}", setup.log());
}

/// Runs only as a child of the test above, as `agentbolt-desk.exe` next to the copied daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fake_desk_child() {
    let (Ok(ask_id), Ok(channel)) = (std::env::var(FAKE_DESK_ASK), std::env::var(FAKE_DESK_CHANNEL)) else {
        return;
    };
    let names = Names::for_channel(&channel).unwrap();
    let me = std::env::current_exe().unwrap();
    assert_eq!(me.file_name().unwrap(), Path::new("agentbolt-desk.exe").as_os_str());
    // The default client check: the daemon must be next to this exe.
    let mut desk = Client::connect(&names, Pipe::Desk).await.expect("connect as the desk");
    desk.call(&DeskOp::List).await.expect("list");
    let answer = DeskOp::Answer { ask_id, choice: Some("Blue".into()), values: None, note: None };
    let result = desk.call(&answer).await.expect("answer");
    println!("answered: {result}");
}

/// Manual check of the tray and a real toast (run against a daemon in an Anode seat, never `main`): posts one
/// reversible choose ask to the daemon of `AGENTBOLT_SMOKE_CHANNEL` served by `AGENTBOLT_SMOKE_DAEMON`.
/// `cargo test -p agentbolt-daemon --test daemon_process -- --ignored manual_toast_smoke`
#[ignore = "manual: needs a running daemon on a throwaway channel"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_toast_smoke() {
    let channel = std::env::var("AGENTBOLT_SMOKE_CHANNEL").expect("AGENTBOLT_SMOKE_CHANNEL");
    let daemon = PathBuf::from(std::env::var("AGENTBOLT_SMOKE_DAEMON").expect("AGENTBOLT_SMOKE_DAEMON"));
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main(), "never on main");
    let mut mcp = Client::connect_to(&names, Pipe::Mcp, &[daemon]).await.expect("connect");
    let reg = McpOp::Register {
        owner: Some("ipctest-smoke".into()),
        client: "manual-toast-smoke".into(),
        provisional_key: None,
        session_ref: None,
        cwd: None,
        doing: None,
    };
    let handle = mcp.call(&reg).await.unwrap()["handle"].as_str().unwrap().to_string();
    let asked = mcp.call(&McpOp::Ask { handle, ask: choose_ask() }).await.unwrap();
    println!("asked: {asked}");
}
