//! Pipe tests. Each runs on its own throwaway channel (`ipctest-<random>`), never `main`; the guard removes the
//! channel's data folder (Unix sockets live there; Windows pipes vanish with their last handle).

use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use agentbolt_core::protocol::{DeskOp, McpOp};
use serde_json::json;

use super::*;

struct TestChannel {
    names: Names,
}

impl TestChannel {
    fn new() -> Self {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        h.write_u32(std::process::id());
        let channel = format!("ipctest-{:012x}", h.finish() & 0xffff_ffff_ffff);
        let names = Names::for_channel(&channel).unwrap();
        assert!(!names.is_main());
        TestChannel { names }
    }
}

impl Drop for TestChannel {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.names.data_dir());
    }
}

#[derive(Default)]
struct Recorder {
    handled: AtomicUsize,
    refused: Mutex<Vec<(Option<u32>, Option<PathBuf>)>>,
    connected: Mutex<Vec<ClientInfo>>,
    disconnected: Mutex<Vec<u64>>,
    dropped_work: AtomicUsize,
}

struct Echo(Arc<Recorder>);

/// Counts work futures dropped before they finished.
struct DropGuard(Arc<Recorder>, bool);

impl Drop for DropGuard {
    fn drop(&mut self) {
        if !self.1 {
            self.0.dropped_work.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Handler for Echo {
    async fn handle(&self, client: &ClientInfo, op: Value) -> Result<Value, String> {
        self.0.handled.fetch_add(1, Ordering::SeqCst);
        match op["op"].as_str() {
            Some("fail") => Err("nope".into()),
            Some("sleep") => {
                let mut guard = DropGuard(self.0.clone(), false);
                tokio::time::sleep(Duration::from_millis(op["args"]["ms"].as_u64().unwrap_or(0))).await;
                guard.1 = true;
                Ok(json!("slept"))
            }
            _ => Ok(json!({ "echo": op, "pid": client.pid, "connection": client.connection })),
        }
    }
    fn connected(&self, client: &ClientInfo) {
        self.0.connected.lock().unwrap().push(client.clone());
    }
    fn disconnected(&self, client: &ClientInfo) {
        self.0.disconnected.lock().unwrap().push(client.connection);
    }
    fn refused(&self, pid: Option<u32>, image: Option<&Path>) {
        self.0.refused.lock().unwrap().push((pid, image.map(Path::to_path_buf)));
    }
}

fn me() -> Vec<PathBuf> {
    vec![std::env::current_exe().unwrap()]
}

fn start(names: &Names, pipe: Pipe, check: PeerCheck) -> Arc<Recorder> {
    let recorder = Arc::new(Recorder::default());
    let server = Server::bind_with(names, pipe, check).expect("bind");
    let handler = Arc::new(Echo(recorder.clone()));
    tokio::spawn(async move { server.serve(handler).await });
    recorder
}

async fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn round_trip_on_the_mcp_pipe() {
    let ch = TestChannel::new();
    let rec = start(&ch.names, Pipe::Mcp, PeerCheck::AnyLocalUser);
    let mut client = Client::connect_to(&ch.names, Pipe::Mcp, &me()).await.expect("connect");

    let result = client.call(&McpOp::Status).await.expect("status");
    assert_eq!(result["echo"], json!({ "op": "status" }));
    assert_eq!(result["pid"], json!(std::process::id()), "the server sees the client's real pid");
    // The handler can decode the op it was given back into the typed protocol.
    let op: McpOp = serde_json::from_value(result["echo"].clone()).unwrap();
    assert!(matches!(op, McpOp::Status));

    let op = McpOp::Heartbeat { handle: "h1".into(), doing: Some("tests".into()) };
    let result = client.call(&op).await.unwrap();
    assert_eq!(result["echo"]["args"]["handle"], "h1");

    match client.call(&json!({ "op": "fail" })).await {
        Err(IpcError::Remote(why)) => assert_eq!(why, "nope"),
        other => panic!("expected a remote error, got {other:?}"),
    }
    // Still usable after an error.
    client.call(&McpOp::Status).await.unwrap();
    assert_eq!(rec.handled.load(Ordering::SeqCst), 4);

    drop(client);
    wait_for("disconnected", || rec.disconnected.lock().unwrap().len() == 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn round_trip_on_the_desk_pipe_for_an_accepted_image() {
    let ch = TestChannel::new();
    // This test binary stands in for agentbolt-desk.exe.
    let rec = start(&ch.names, Pipe::Desk, PeerCheck::Image(me()));
    let mut client = Client::connect_to(&ch.names, Pipe::Desk, &me()).await.expect("connect");
    let result = client.call(&DeskOp::List).await.expect("list");
    assert_eq!(result["echo"], json!({ "op": "list" }));
    let token = DeskOp::AnswerToken { token: "abc".into() };
    assert_eq!(client.call(&token).await.unwrap()["echo"]["args"]["token"], "abc");
    assert!(rec.refused.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_server_on_the_same_name_fails() {
    let ch = TestChannel::new();
    let first = Server::bind(&ch.names, Pipe::Mcp).expect("first bind");
    match Server::bind(&ch.names, Pipe::Mcp) {
        Err(IpcError::AlreadyServed(_)) => {}
        Err(other) => panic!("expected AlreadyServed, got {other:?}"),
        Ok(_) => panic!("a second server bound the same pipe"),
    }
    // The other pipe of the channel is independent.
    let _desk = Server::bind(&ch.names, Pipe::Desk).expect("desk bind");
    // Once the first server is gone, the name can be bound again (a daemon restart).
    drop(first);
    Server::bind(&ch.names, Pipe::Mcp).expect("rebind after the first server closed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_refuses_a_server_that_isnt_the_daemon() {
    let ch = TestChannel::new();
    // The test binary squats the pipe name; the client expects agentbolt-daemon next to itself.
    let rec = start(&ch.names, Pipe::Mcp, PeerCheck::AnyLocalUser);
    match Client::connect(&ch.names, Pipe::Mcp).await {
        Err(IpcError::WrongServer(pipe, who)) => {
            assert_eq!(pipe, ch.names.mcp_pipe);
            let me = std::env::current_exe().unwrap();
            let stem = me.file_stem().unwrap().to_string_lossy().to_lowercase();
            assert!(who.to_lowercase().contains(&stem), "names the squatter: {who}");
        }
        Err(other) => panic!("expected WrongServer, got {other:?}"),
        Ok(_) => panic!("connected to a squatting server"),
    }
    assert_eq!(rec.handled.load(Ordering::SeqCst), 0);
}

#[cfg(any(windows, target_os = "linux"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_desk_pipe_drops_a_client_that_isnt_the_desk() {
    let ch = TestChannel::new();
    // Default check: only agentbolt-desk.exe next to the server's executable.
    let recorder = Arc::new(Recorder::default());
    let server = Server::bind(&ch.names, Pipe::Desk).expect("bind");
    let handler = Arc::new(Echo(recorder.clone()));
    tokio::spawn(async move { server.serve(handler).await });

    let mut client = Client::connect_to(&ch.names, Pipe::Desk, &me()).await.expect("the pipe itself opens");
    let answer = DeskOp::Answer { ask_id: "a1".into(), choice: Some("yes".into()), values: None, note: None };
    let err = client.call(&answer).await.expect_err("a fake Desk must not get a reply");
    assert!(matches!(err, IpcError::Protocol(_) | IpcError::Io(_)), "{err:?}");
    wait_for("refusal", || !recorder.refused.lock().unwrap().is_empty()).await;
    let refused = recorder.refused.lock().unwrap().clone();
    assert_eq!(refused[0].0, Some(std::process::id()));
    assert_eq!(recorder.handled.load(Ordering::SeqCst), 0, "nothing was read from the fake Desk");
    assert!(recorder.connected.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_daemon_is_not_running() {
    let ch = TestChannel::new();
    match Client::connect(&ch.names, Pipe::Mcp).await {
        Err(IpcError::NotRunning(channel)) => assert_eq!(channel, ch.names.channel),
        Err(other) => panic!("expected NotRunning, got {other:?}"),
        Ok(_) => panic!("connected to nothing"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_clients_at_once() {
    let ch = TestChannel::new();
    let rec = start(&ch.names, Pipe::Mcp, PeerCheck::AnyLocalUser);
    let mut tasks = Vec::new();
    for i in 0..12 {
        let names = ch.names.clone();
        tasks.push(tokio::spawn(async move {
            let mut client = Client::connect_to(&names, Pipe::Mcp, &me()).await.expect("connect");
            for _ in 0..5 {
                let r = client.call(&json!({ "op": "ping", "args": { "i": i } })).await.unwrap();
                assert_eq!(r["echo"]["args"]["i"], i);
            }
            client.call(&McpOp::Status).await.unwrap()["connection"].as_u64().unwrap()
        }));
    }
    let mut connections = Vec::new();
    for t in tasks {
        connections.push(t.await.unwrap());
    }
    connections.sort();
    connections.dedup();
    assert_eq!(connections.len(), 12, "each client has its own connection id");
    assert_eq!(rec.handled.load(Ordering::SeqCst), 12 * 6);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_leaving_mid_wait_drops_the_work() {
    let ch = TestChannel::new();
    let rec = start(&ch.names, Pipe::Mcp, PeerCheck::AnyLocalUser);
    let mut client = Client::connect_to(&ch.names, Pipe::Mcp, &me()).await.unwrap();
    let call = tokio::spawn(async move {
        let _ = client.call(&json!({ "op": "sleep", "args": { "ms": 30_000 } })).await;
    });
    wait_for("the sleep to start", || rec.handled.load(Ordering::SeqCst) == 1).await;
    call.abort();
    let started = Instant::now();
    wait_for("disconnect", || rec.disconnected.lock().unwrap().len() == 1).await;
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(rec.dropped_work.load(Ordering::SeqCst), 1, "the 30 s op was dropped, not run out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_lines_get_an_error_and_the_connection_survives() {
    let ch = TestChannel::new();
    let _rec = start(&ch.names, Pipe::Mcp, PeerCheck::AnyLocalUser);
    let mut client = Client::connect_to(&ch.names, Pipe::Mcp, &me()).await.unwrap();
    client.stream.write_all(b"not json\n{\"id\":9}\n\n").await.unwrap();
    client.stream.flush().await.unwrap();
    let first: Response =
        serde_json::from_slice(&read_line_capped(&mut client.stream, MAX_RESPONSE).await.unwrap().unwrap()).unwrap();
    assert!(!first.ok && first.id == 0 && first.error.unwrap().starts_with("bad request"));
    let second: Response =
        serde_json::from_slice(&read_line_capped(&mut client.stream, MAX_RESPONSE).await.unwrap().unwrap()).unwrap();
    assert!(!second.ok && second.id == 9 && second.error.unwrap().contains("missing op"));
    client.call(&McpOp::Status).await.expect("still usable");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_oversized_request_closes_the_connection() {
    let ch = TestChannel::new();
    let rec = start(&ch.names, Pipe::Mcp, PeerCheck::AnyLocalUser);
    let mut client = Client::connect_to(&ch.names, Pipe::Mcp, &me()).await.unwrap();
    let huge = json!({ "op": "ping", "args": "x".repeat(MAX_REQUEST + 10) });
    assert!(client.call(&huge).await.is_err());
    assert_eq!(rec.handled.load(Ordering::SeqCst), 0);
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pipe_dacl_grants_only_the_current_user() {
    use std::os::windows::io::AsRawHandle;
    let ch = TestChannel::new();
    let _rec = start(&ch.names, Pipe::Mcp, PeerCheck::AnyLocalUser);
    let client = Client::connect_to(&ch.names, Pipe::Mcp, &me()).await.unwrap();
    let sddl = win::dacl_sddl(client.stream.get_ref().as_raw_handle()).unwrap();
    let sid = win::current_user_sid().unwrap();
    assert!(sddl.starts_with("D:P"), "protected DACL, no inherited ACEs: {sddl}");
    assert_eq!(sddl.matches('(').count(), 1, "exactly one ACE: {sddl}");
    assert!(sddl.contains(&format!(";;;{sid})")), "the one ACE is the current user's: {sddl}");
    for other in [";WD)", ";AN)", ";BU)", ";BA)", ";SY)", ";AU)", ";IU)"] {
        assert!(!sddl.contains(other), "{other} must not be granted: {sddl}");
    }
}

#[test]
fn sibling_paths_cover_the_cargo_layout() {
    let paths = sibling_paths(DAEMON_EXE);
    let exe = std::env::current_exe().unwrap();
    let file = format!("{DAEMON_EXE}{}", std::env::consts::EXE_SUFFIX);
    assert_eq!(paths[0], exe.parent().unwrap().join(&file));
    // Test binaries live in target/<profile>/deps; the workspace's binaries one folder up.
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[1], exe.parent().unwrap().parent().unwrap().join(&file));
}

#[test]
fn image_checks_compare_canonical_paths() {
    let exe = std::env::current_exe().unwrap();
    let check = PeerCheck::Image(vec![exe.clone()]);
    assert!(check.accepts(Some(&exe)));
    if cfg!(windows) {
        let upper = PathBuf::from(exe.to_string_lossy().to_uppercase());
        assert!(check.accepts(Some(&upper)), "Windows paths compare case-insensitively");
        let verbatim = PathBuf::from(format!(r"\\?\{}", exe.display()));
        assert!(check.accepts(Some(&verbatim)));
    }
    assert!(!check.accepts(Some(&exe.with_file_name("agentbolt-desk.exe"))));
    assert!(!check.accepts(None), "an unknown image is refused");
    assert!(PeerCheck::AnyLocalUser.accepts(None));
}
