//! `agentbolt mcp` over real stdio, with no daemon running: both protocol revisions' discovery and `tools/list`
//! work, the tool list is right and small, and tool calls say plainly that Agentbolt isn't running.
//! Runs on Linux too (CI: `cargo test -p agentbolt --test mcp_stdio`), so nothing here is Windows-only.
//!
//! Each test uses its own throwaway channel, so nothing touches a real install (on Windows the server's one
//! start attempt, `schtasks /Run` for that channel's task, finds no task).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const TOOLS: [&str; 12] = [
    "desk_register",
    "desk_ask",
    "desk_answers",
    "desk_cancel",
    "desk_relay",
    "board_who",
    "board_post",
    "board_read",
    "board_request",
    "board_claim",
    "board_done",
    "work_holds",
];

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

fn throwaway_channel() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("mcptest-{:x}-{nanos:x}", std::process::id())
}

impl Server {
    fn start() -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentbolt"))
            .args(["mcp", "--owner", "mcp-stdio-test"])
            .env("AGENTBOLT_CHANNEL", throwaway_channel())
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CLAUDE_CODE_HOST_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start agentbolt mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Server {
            child,
            stdin,
            lines,
            next_id: 1,
        }
    }

    fn send(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Send a request and return its response (the whole JSON-RPC message), skipping notifications.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("a response within 60 s");
            let msg: Value =
                serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"));
            if msg.get("id") == Some(&json!(id)) {
                return msg;
            }
        }
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let msg = self.request(method, params);
        msg.get("result")
            .cloned()
            .unwrap_or_else(|| panic!("{method} failed: {msg}"))
    }

    fn initialize(&mut self, version: &str) -> Value {
        let r = self.result(
            "initialize",
            json!({"protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "mcp-stdio-test", "version": "0"}}),
        );
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        r
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        self.result("tools/call", json!({"name": tool, "arguments": args}))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn meta_2026() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "mcp-stdio-test", "version": "0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

fn check_tool_list(list: &Value) {
    let tools = list["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, TOOLS);
    for t in tools {
        assert!(
            t.get("outputSchema").is_none(),
            "{} declares an outputSchema",
            t["name"]
        );
        assert_eq!(t["inputSchema"]["type"], "object");
        assert!(
            t["inputSchema"]["properties"].get("handle").is_some(),
            "{} takes no handle",
            t["name"]
        );
        let d = t["description"].as_str().unwrap();
        assert!(!d.trim().is_empty(), "{} has no description", t["name"]);
    }
    let size = serde_json::to_string(&list["tools"]).unwrap().len();
    let descriptions: usize = tools
        .iter()
        .map(|t| t["description"].as_str().unwrap().len())
        .sum();
    eprintln!("tools/list: {size} characters, descriptions {descriptions}");
    assert!(
        size < 6000,
        "tools/list is {size} characters, over the 6,000 budget"
    );
}

#[test]
fn initialize_2025_then_tools_list() {
    let mut s = Server::start();
    let init = s.initialize("2025-06-18");
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["serverInfo"]["name"], "agentbolt");
    assert!(init["capabilities"].get("tools").is_some());
    let instructions = init["instructions"].as_str().unwrap();
    for needle in [
        "durable",
        "start of a run",
        "before you finish",
        "Silence is never agreement",
        "no authority",
    ] {
        assert!(
            instructions.contains(needle),
            "the instructions lack '{needle}'"
        );
    }
    check_tool_list(&s.result("tools/list", json!({})));
}

#[test]
fn initialize_asking_for_2026_falls_back_to_the_newest_handshake_revision() {
    let mut s = Server::start();
    let init = s.initialize("2026-07-28");
    assert_eq!(
        init["protocolVersion"], "2025-11-25",
        "2026-07-28 has no initialize; rmcp answers the newest that does"
    );
    check_tool_list(&s.result("tools/list", json!({})));
}

#[test]
fn discover_2026_then_tools_list_with_cache_hints() {
    let mut s = Server::start();
    let d = s.result("server/discover", json!({"_meta": meta_2026()}));
    let versions: Vec<&str> = d["supportedVersions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        versions.contains(&"2026-07-28") && versions.contains(&"2025-06-18"),
        "{versions:?}"
    );
    assert!(d["instructions"].as_str().unwrap().contains("durable"));
    assert!(
        d.get("ttlMs").is_some() && d.get("cacheScope").is_some(),
        "{d}"
    );
    let list = s.result("tools/list", json!({"_meta": meta_2026()}));
    assert!(
        list.get("ttlMs").is_some(),
        "2026-07-28 requires ttlMs on tools/list: {list}"
    );
    assert!(
        list.get("cacheScope").is_some(),
        "2026-07-28 requires cacheScope on tools/list: {list}"
    );
    check_tool_list(&list);
    // A 2026 call without its per-request _meta is refused by the protocol layer, not by us.
    let bad = s.request("tools/list", json!({}));
    assert!(bad.get("error").is_some(), "{bad}");
}

fn assert_not_running(r: &Value) {
    assert_eq!(r["isError"], true, "{r}");
    let text = r["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Agentbolt isn't running"), "{text}");
    assert!(
        r["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("isn't running"),
        "{r}"
    );
}

#[test]
fn tool_calls_without_a_daemon_say_it_isnt_running() {
    let mut s = Server::start();
    s.initialize("2025-06-18");
    let ask = json!({
        "kind": "choose", "title": "Ship the fix?", "options": ["Ship", "Wait"], "default": "Ship",
        "deadline": "2026-10-20T18:00:00Z", "reversible": true
    });
    assert_not_running(&s.call("desk_ask", ask));
    assert_not_running(&s.call("desk_answers", json!({"wait_ms": 100})));
    assert_not_running(&s.call("desk_register", json!({"doing": "testing"})));
    assert_not_running(&s.call("desk_cancel", json!({"id": "a1", "handle": "h1"})));
}

#[test]
fn tool_calls_without_a_daemon_work_over_2026_too() {
    let mut s = Server::start();
    s.result("server/discover", json!({"_meta": meta_2026()}));
    let r = s.result(
        "tools/call",
        json!({"name": "desk_answers", "arguments": {}, "_meta": meta_2026()}),
    );
    assert_not_running(&r);
}

#[test]
fn bad_arguments_are_explained_before_any_daemon_is_needed() {
    let mut s = Server::start();
    s.initialize("2025-06-18");
    let r = s.call(
        "desk_ask",
        json!({"kind": "choose", "title": "t", "default": "a", "reversible": true}),
    );
    assert_eq!(r["isError"], true);
    assert!(
        r["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("deadline is required"),
        "{r}"
    );
    let r = s.call("desk_relay", json!({"action": "shout"}));
    assert!(
        r["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("list or mark"),
        "{r}"
    );
    let unknown = s.request(
        "tools/call",
        json!({"name": "desk_answer", "arguments": {}}),
    );
    assert!(
        unknown.get("error").is_some(),
        "an unknown tool is a protocol error: {unknown}"
    );
}
