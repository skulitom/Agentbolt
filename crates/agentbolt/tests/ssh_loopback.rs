//! Opt-in real OpenSSH transport fixture. No user account, SSH settings or installed channel is changed.
//! Requires a sibling daemon binary and AGENTBOLT_SSH_TEST_PYTHON pointing to Python with Paramiko.
use agentbolt::remote::{Config, Remote, Shell};
use agentbolt_core::{
    board::BoardOp,
    channel::Names,
    model::{AskInput, AskKind, Priority},
    protocol::{DeskOp, McpOp},
    service::{ClientInfo, Service},
    service_impl::StoreService,
};
use agentbolt_ipc::{Client, Pipe};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use time::OffsetDateTime;

#[path = "../src/testutil.rs"]
mod testutil;

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        // Closing the fixture server's stdin lets it close transports and reap its relay children.
        let graceful = self.0.stdin.is_some();
        drop(self.0.stdin.take());
        let start = Instant::now();
        while graceful && start.elapsed() < Duration::from_secs(5) {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Data(PathBuf);
impl Drop for Data {
    fn drop(&mut self) {
        let path = std::fs::canonicalize(&self.0).unwrap();
        let parent = std::fs::canonicalize(self.0.parent().unwrap()).unwrap();
        assert!(path.starts_with(parent));
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ssh-loop-")
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}
fn command(exe: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(exe);
    command.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
fn register() -> McpOp {
    McpOp::Register {
        owner: Some("ssh-fixture".into()),
        client: "Encrypted loopback fixture".into(),
        session_ref: None,
        cwd: None,
        doing: None,
        provisional_key: None,
    }
}
fn ask(title: &str) -> AskInput {
    AskInput {
        kind: AskKind::Choose,
        title: title.into(),
        body: "Synthetic isolated SSH fixture".into(),
        evidence: vec![],
        options: vec!["Yes".into(), "No".into()],
        fields: vec![],
        default: json!("No"),
        deadline: (OffsetDateTime::now_utc() + time::Duration::days(1))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        reversible: true,
        priority: Priority::P2,
        p0_reason: None,
        dedupe_key: Some(title.into()),
    }
}
fn events(dir: &Path, op: &str) -> usize {
    std::fs::read_to_string(dir.join("events"))
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == op)
        .count()
}
async fn until_events(dir: &Path, op: &str, count: usize) {
    for _ in 0..200 {
        if events(dir, op) >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("expected {count} forwarded {op} calls");
}

fn mcp_host(exe: &Path, config: &Path, channel: &str, seeded_id: &Value) {
    let mut host = Process(
        command(exe)
            .args([
                "mcp",
                "--owner",
                "ssh-fixture",
                "--ssh",
                "fixture",
                "--machine",
                "loopback-b",
                "--channel",
                channel,
                "--remote-exe",
            ])
            .arg(exe)
            .arg("--ssh-config")
            .arg(config)
            .env("AGENTBOLT_CHANNEL", channel)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CLAUDE_CODE_HOST_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = host.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut request = |id, method: &str, params: Value| -> Value {
        let input = host.0.stdin.as_mut().unwrap();
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
        )
        .unwrap();
        input.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let line = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["id"] == id {
                assert!(response.get("error").is_none(), "{response}");
                return response["result"].clone();
            }
        }
    };
    let initialized = request(
        1,
        "initialize",
        json!({"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"ssh-fixture", "version":"0"}}),
    );
    assert_eq!(initialized["protocolVersion"], "2025-06-18");
    writeln!(
        host.0.stdin.as_mut().unwrap(),
        "{}",
        json!({"jsonrpc":"2.0", "method":"notifications/initialized"})
    )
    .unwrap();
    let input = host.0.stdin.as_mut().unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0", "id":2, "method":"tools/call", "params":{"name":"desk_register", "arguments":{}}})).unwrap();
    input.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let line = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        if response["id"] != 2 {
            continue;
        }
        let result = &response["result"];
        assert_ne!(result["isError"], true, "{response}");
        let structured = &result["structuredContent"];
        let text = result["content"][0]["text"].as_str().unwrap();
        let mirrored: Value = serde_json::from_str(text.split_once("\n\n").unwrap().1).unwrap();
        assert_eq!(&mirrored, structured);
        assert_eq!(structured["machine"], "remote:loopback-b");
        assert_eq!(&structured["answers"][0]["id"], seeded_id);
        break;
    }
}

#[tokio::test]
#[ignore = "opt-in: real local OpenSSH, Python/Paramiko and a built sibling daemon required"]
async fn encrypted_relay_preserves_identity_state_and_uncertain_write_semantics() {
    let python = std::env::var_os("AGENTBOLT_SSH_TEST_PYTHON")
        .expect("set AGENTBOLT_SSH_TEST_PYTHON to Python with Paramiko installed");
    let temp = testutil::TempDir::new();
    let dir = temp.path().join("SSH config with spaces");
    std::fs::create_dir(&dir).unwrap();
    let channel = format!("ssh-loop-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let data = names.data_dir().unwrap();
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::create_dir(&data).expect("fixture channel must be fresh");
    let _data = Data(data);
    // Seed a historical answer offline, then close the store before starting its sole writer.
    let service = StoreService::open(&names.db_path().unwrap())
        .unwrap()
        .with_channel(&channel);
    let seeded = service.mcp(&ClientInfo::default(), register()).unwrap().0;
    let seeded = service
        .mcp(
            &ClientInfo::default(),
            McpOp::Ask {
                handle: seeded["handle"].as_str().unwrap().into(),
                ask: ask("Already answered"),
            },
        )
        .unwrap()
        .0;
    service
        .desk(DeskOp::Answer {
            ask_id: seeded["id"].as_str().unwrap().into(),
            choice: Some("Yes".into()),
            values: None,
            note: Some("Offline synthetic seed".into()),
        })
        .unwrap();
    drop(service);
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt"));
    let daemon = exe.with_file_name(format!("agentbolt-daemon{}", std::env::consts::EXE_SUFFIX));
    let _daemon = Process(
        command(daemon)
            .args(["--channel", &channel, "--headless"])
            .env("AGENTBOLT_CHANNEL", &channel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut ready = false;
    for _ in 0..200 {
        if let Ok(mut client) = Client::connect(&names, Pipe::Mcp).await {
            assert_eq!(
                client.call(&McpOp::Status).await.unwrap()["channel"],
                channel
            );
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(ready, "fixture daemon did not start");
    let script = dir.join("server.py");
    std::fs::write(&script, SSH_SERVER).unwrap();
    let mut server = Process(
        command(python)
            .arg(&script)
            .arg(&dir)
            .arg(&exe)
            .arg(&channel)
            .env("AGENTBOLT_CHANNEL", &channel)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = server.0.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(15)).unwrap().trim(),
        "READY"
    );
    let config = |file: &str, machine: &str| Config {
        host: "fixture".into(),
        machine: machine.into(),
        executable: exe.to_str().unwrap().into(),
        shell: Shell::Sh,
        config_file: Some(dir.join(file)),
    };
    let remote = Arc::new(Remote::new(config("config", "loopback-a"), channel.clone()));
    for file in ["wrong-host", "wrong-client"] {
        let refused = Remote::new(config(file, "loopback-a"), channel.clone());
        assert!(
            refused.call(&McpOp::Status).await.is_err(),
            "{file} must fail authentication"
        );
        assert_eq!(
            events(&dir, "status"),
            0,
            "no operation may reach the relay"
        );
    }
    let registration = remote.call(&register()).await.unwrap();
    assert_eq!(registration["machine"], "remote:loopback-a");
    assert_eq!(registration["answers"][0]["id"], seeded["id"]);
    let handle = registration["handle"].as_str().unwrap().to_string();
    let before = events(&dir, "status");
    std::fs::write(dir.join("drop-next"), "status").unwrap();
    assert_eq!(
        remote.call(&McpOp::Status).await.unwrap()["channel"],
        channel
    );
    assert_eq!(
        events(&dir, "status"),
        before + 2,
        "a safe read reconnects once"
    );
    let mutation = McpOp::Ask {
        handle: handle.clone(),
        ask: ask("Uncertain write"),
    };
    std::fs::write(dir.join("drop-next"), "ask").unwrap();
    let error = remote.call(&mutation).await.unwrap_err().to_string();
    assert!(error.contains("may have committed"), "{error}");
    assert_eq!(
        events(&dir, "ask"),
        1,
        "a write must never replay automatically"
    );
    let state = remote.call(&register()).await.unwrap();
    assert_eq!(state["open_asks"].as_array().unwrap().len(), 1);
    let id = state["open_asks"][0]["id"].as_str().unwrap().to_string();
    let deduped = remote.call(&mutation).await.unwrap();
    assert_eq!(deduped["id"], id);
    assert_eq!(deduped["placed"], "existing");
    // Three actual long waits occupy three connections; work still has its reserved fourth slot.
    let mut waits = Vec::new();
    for _ in 0..3 {
        let remote = remote.clone();
        let op = McpOp::Answers {
            handle: handle.clone(),
            ids: vec![id.clone()],
            wait_ms: 10000,
        };
        waits.push(tokio::spawn(async move { remote.call(&op).await }));
    }
    until_events(&dir, "answers", 3).await;
    let count = events(&dir, "answers");
    let overflow = McpOp::Answers {
        handle: handle.clone(),
        ids: vec![id.clone()],
        wait_ms: 10000,
    };
    assert!(
        remote
            .call(&overflow)
            .await
            .unwrap_err()
            .to_string()
            .contains("Three SSH answer waits")
    );
    assert_eq!(
        events(&dir, "answers"),
        count,
        "excess wait is refused before submission"
    );
    let who = remote
        .call(&McpOp::Board {
            handle: handle.clone(),
            action: BoardOp::Who,
        })
        .await
        .unwrap();
    assert!(who.to_string().contains("remote:loopback-a"));
    remote
        .call(&McpOp::Cancel {
            handle: handle.clone(),
            id: id.clone(),
        })
        .await
        .unwrap();
    for wait in waits {
        let result = tokio::time::timeout(Duration::from_secs(12), wait)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result["cancelled"], json!([id]));
    }
    drop(remote);
    // A fresh connection with a different configured origin sees the same explicitly bound owner's state.
    let fresh = Remote::new(config("config", "loopback-b"), channel.clone());
    let resumed = fresh.call(&register()).await.unwrap();
    assert_eq!(resumed["machine"], "remote:loopback-b");
    assert_eq!(resumed["answers"][0]["id"], seeded["id"]);
    assert!(resumed["open_asks"].as_array().unwrap().is_empty());
    let state = fresh
        .call(&McpOp::Answers {
            handle: resumed["handle"].as_str().unwrap().into(),
            ids: vec![id.clone()],
            wait_ms: 0,
        })
        .await
        .unwrap();
    assert_eq!(state["cancelled"], json!([id]));
    drop(fresh);
    mcp_host(&exe, &dir.join("config"), &channel, &seeded["id"]);
    drop(server);
    assert!(
        dir.join("cleaned").exists(),
        "SSH fixture cleanup did not finish"
    );
    assert!(
        !dir.join("children-left").exists(),
        "SSH fixture left a relay process"
    );
    println!(
        "{channel}: encrypted relay, host/client authentication, origin, durable reads, safe reconnect, uncertain write and four-slot concurrency pass"
    );
}

// Test-only SSH endpoint: synthetic keys, loopback only, exact relay command allowlist, no shell.
// Embedded to keep the repository guard's Rust-only source convention. Never used by production code.
const SSH_SERVER: &str = r#"
import csv, json, logging, os, pathlib, queue, socket, subprocess, sys, threading
import paramiko
logging.getLogger('paramiko').addHandler(logging.NullHandler())
root, exe, channel = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
stop, lock = threading.Event(), threading.Lock()
transports, children = [], []
host, client, wrong = [paramiko.ECDSAKey.generate() for _ in range(3)]
client.write_private_key_file(str(root / 'client'))
wrong.write_private_key_file(str(root / 'wrong-client-key'))
for name in ['client', 'wrong-client-key']:
    os.chmod(root / name, 0o600)
    if os.name == 'nt':
        # Only these freshly generated throwaway keys: OpenSSH rejects inherited group access.
        user = subprocess.check_output(['whoami', '/user', '/fo', 'csv', '/nh'], creationflags=0x08000000).decode().strip()
        sid = next(csv.reader([user]))[1]
        subprocess.run(['icacls', str(root / name), '/inheritance:r', '/grant:r', '*' + sid + ':F'],
                       check=True, stdout=subprocess.DEVNULL, creationflags=0x08000000)
listener = socket.socket()
listener.bind(('127.0.0.1', 0))
listener.listen(8)
listener.settimeout(.25)
port = listener.getsockname()[1]
for name, key in [('known', host), ('wrong-known', wrong)]:
    (root / name).write_text('[127.0.0.1]:%d %s %s\n' % (port, key.get_name(), key.get_base64()), encoding='utf-8')
for name, known, identity in [('config', 'known', 'client'), ('wrong-host', 'wrong-known', 'client'), ('wrong-client', 'known', 'wrong-client-key')]:
    config = '''Host fixture
  HostName 127.0.0.1
  Port %d
  User fixture
  IdentityFile "%s"
  IdentitiesOnly yes
  UserKnownHostsFile "%s"
  GlobalKnownHostsFile %s
  PasswordAuthentication no
  KbdInteractiveAuthentication no
  StrictHostKeyChecking no
''' % (port, (root / identity).as_posix(), (root / known).as_posix(), 'NUL' if os.name == 'nt' else '/dev/null')
    (root / name).write_text(config, encoding='utf-8')
allowed = {'"%s" relay --channel %s --machine %s' % (exe.replace('\\', '/'), channel, machine): machine for machine in ['loopback-a', 'loopback-b']}
class Server(paramiko.ServerInterface):
    def __init__(self):
        self.ready, self.machine = threading.Event(), None
    def get_allowed_auths(self, username): return 'publickey'
    def check_auth_publickey(self, username, key):
        return paramiko.AUTH_SUCCESSFUL if username == 'fixture' and key == client else paramiko.AUTH_FAILED
    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == 'session' and chanid == 0 else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_exec_request(self, ch, cmd):
        self.machine = allowed.get(cmd.decode('utf-8', errors='strict'))
        self.ready.set()
        return self.machine is not None
def work(sock):
    transport, process = paramiko.Transport(sock), None
    with lock: transports.append(transport)
    try:
        transport.add_server_key(host)
        server = Server()
        transport.start_server(server=server)
        ch = transport.accept(10)
        if ch is None or not server.ready.wait(10) or server.machine is None: return
        with lock:
            if stop.is_set(): return
            process = subprocess.Popen([exe, 'relay', '--channel', channel, '--machine', server.machine], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, creationflags=0x08000000 if os.name == 'nt' else 0)
            children.append(process)
        replies = queue.Queue()
        def read_replies():
            for line in process.stdout: replies.put(line)
            replies.put(b'')
        threading.Thread(target=read_replies, daemon=True).start()
        incoming = ch.makefile('rb')
        while not stop.is_set() and transport.is_active():
            request = incoming.readline(1048578)
            if not request: break
            if len(request) > 1048577: raise ValueError('oversized request')
            op = json.loads(request)['op']
            with lock:
                with (root / 'events').open('a', encoding='utf-8') as log: log.write(op + '\n')
                fault = root / 'drop-next'
                drop = fault.exists() and fault.read_text(encoding='utf-8') == op
                if drop: fault.unlink()
            process.stdin.write(request)
            process.stdin.flush()
            reply = None
            while not stop.is_set() and transport.is_active():
                try:
                    reply = replies.get(timeout=.25)
                    break
                except queue.Empty: pass
            if not reply or drop: break
            ch.sendall(reply)
    except (EOFError, OSError, paramiko.SSHException): pass
    finally:
        transport.close()
        if process is not None:
            if process.poll() is None: process.kill()
            process.wait(timeout=3)
def accept():
    while not stop.is_set():
        try: sock, _ = listener.accept()
        except socket.timeout: continue
        except OSError: break
        threading.Thread(target=work, args=(sock,), daemon=True).start()
def cleanup():
    stop.set()
    listener.close()
    with lock:
        for t in transports: t.close()
        for p in children:
            if p.poll() is None: p.kill()
        for p in children: p.wait(timeout=3)
        if any(p.poll() is None for p in children): (root / 'children-left').touch()
        (root / 'cleaned').touch()
threading.Thread(target=accept, daemon=True).start()
timer = threading.Timer(90, cleanup)
timer.daemon = True
timer.start()
try:
    print('READY', flush=True)
    sys.stdin.readline()
finally:
    timer.cancel()
    cleanup()
"#;
