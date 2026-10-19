//! Opt-in live Desk fixture. Run in Anode, then use the visible Desk to answer the three asks together.
//! The test owns its processes and data and removes them on success or assertion failure.
#![cfg(windows)]

use agentbolt_core::channel::Names;
use agentbolt_core::model::{AnswerVia, AskInput, AskKind, Field, Priority};
use agentbolt_core::protocol::{AnswersResult, AskResult, McpOp, RegisterResult};
use agentbolt_ipc::{Client, Pipe};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Data(PathBuf);
impl Drop for Data {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires Anode: starts two real Desk windows; no manual input needed"]
fn lifecycle_handshake_preserves_offline_drafts_and_closes_warm_windows() {
    use agentbolt_ipc::desk_control::{Op, Prepared, endpoint};
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible};
    use windows::core::HSTRING;
    let channel = format!("gui-upgrade-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let data = Data(names.data_dir().unwrap());
    assert!(!data.0.exists());
    std::fs::create_dir_all(&data.0).unwrap();
    let mut session = 0;
    unsafe {
        ProcessIdToSessionId(std::process::id(), &mut session).unwrap();
    }
    let saved = data.0.join(format!("desk-drafts-{session}.json"));
    let draft = json!({"choice":"Yes", "note":"Saved while offline", "text":"Edited reply", "text_filled":true,
        "fields":{"quantity":{"Text":"7"}}});
    std::fs::write(
        &saved,
        serde_json::to_vec(&json!({"version":1,"state":{
            "drafts":{"offline-ask":draft},"selected":"offline-ask","batch":["offline-ask"]
        }}))
        .unwrap(),
    )
    .unwrap();
    let desk = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt-desk"));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let transaction = "a".repeat(32);
    let other = "b".repeat(32);
    for warm in [false, true] {
        let mut cmd = Command::new(&desk);
        cmd.args(["--channel", &channel])
            .env("AGENTBOLT_CHANNEL", &channel);
        if warm {
            cmd.arg("--warm");
        }
        let mut child = Process(cmd.spawn().unwrap());
        let control_names = endpoint(&names, child.0.id());
        rt.block_on(async {
            let start = Instant::now();
            let mut control = loop {
                if let Ok(c) =
                    Client::connect_to(&control_names, Pipe::Mcp, std::slice::from_ref(&desk)).await
                {
                    break c;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(20),
                    "no lifecycle listener"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
            if !warm {
                loop {
                    if unsafe {
                        FindWindowW(None, &HSTRING::from(format!("Agentbolt ({channel})")))
                    }
                    .is_ok_and(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
                    {
                        break;
                    }
                    assert!(
                        start.elapsed() < Duration::from_secs(20),
                        "Desk did not become visible"
                    );
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            assert!(
                control
                    .call(&Op::Close {
                        transaction: transaction.clone()
                    })
                    .await
                    .is_err()
            );
            let prepared: Prepared = serde_json::from_value(
                control
                    .call(&Op::Prepare {
                        transaction: transaction.clone(),
                    })
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(prepared.pid, child.0.id());
            assert_eq!(prepared.session, session);
            assert_eq!(prepared.warm, warm);
            assert_eq!(prepared.visible, !warm);
            assert!(
                control
                    .call(&Op::Close {
                        transaction: other.clone()
                    })
                    .await
                    .is_err()
            );
            assert_eq!(
                control
                    .call(&Op::Cancel {
                        transaction: other.clone()
                    })
                    .await
                    .unwrap()["resumed"],
                false
            );
            assert_eq!(
                control
                    .call(&Op::Cancel {
                        transaction: transaction.clone()
                    })
                    .await
                    .unwrap()["resumed"],
                true
            );
            assert!(
                control
                    .call(&Op::Close {
                        transaction: transaction.clone()
                    })
                    .await
                    .is_err()
            );
            control
                .call(&Op::Prepare {
                    transaction: transaction.clone(),
                })
                .await
                .unwrap();
            control
                .call(&Op::Close {
                    transaction: transaction.clone(),
                })
                .await
                .unwrap();
        });
        let start = Instant::now();
        loop {
            if let Some(exit) = child.0.try_wait().unwrap() {
                assert!(exit.success());
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "prepared Desk did not close"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let restored: Value = serde_json::from_slice(&std::fs::read(&saved).unwrap()).unwrap();
        assert_eq!(restored["state"]["drafts"]["offline-ask"], draft);
        assert_eq!(restored["state"]["batch"], json!(["offline-ask"]));
        println!(
            "Lifecycle pause, cancellation, native close and retained drafts passed: warm={warm}, channel {channel}"
        );
    }
}

struct Mcp {
    _process: Process,
    input: std::process::ChildStdin,
    output: std::sync::mpsc::Receiver<String>,
    id: u64,
}

impl Mcp {
    fn start(exe: &std::path::Path, channel: &str) -> Self {
        let mut child = Command::new(exe)
            .args(["mcp", "--channel", channel])
            .env("AGENTBOLT_CHANNEL", channel)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CLAUDE_CODE_HOST_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, output) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut mcp = Self {
            _process: Process(child),
            input,
            output,
            id: 0,
        };
        mcp.request("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"gui-owner-test","version":"0"}}));
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
                .recv_timeout(Duration::from_secs(10))
                .expect("MCP response");
            let msg: Value = serde_json::from_str(&line).unwrap();
            if msg["id"] == self.id {
                assert!(msg.get("error").is_none(), "{msg}");
                return msg["result"].clone();
            }
        }
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let result = self.request("tools/call", json!({"name":name,"arguments":args}));
        assert_ne!(result["isError"], true, "{result}");
        let text = result["content"][0]["text"].as_str().unwrap();
        let mirrored: Value = serde_json::from_str(text.rsplit_once("\n\n").unwrap().1).unwrap();
        assert_eq!(
            mirrored, result["structuredContent"],
            "both MCP branches must contain the complete result"
        );
        mirrored
    }
}

#[test]
#[ignore = "requires isolated app bundle; no manual approval or GUI interaction"]
fn owner_names_apply_immediately_through_mcp() {
    let channel = format!("gui-owner-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let data = Data(names.data_dir().unwrap());
    assert!(!data.0.exists(), "fixture channel must be fresh");
    let desk = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt-desk"));
    let _daemon = Process(
        Command::new(desk.with_file_name("agentbolt-daemon.exe"))
            .args(["--channel", &channel, "--headless"])
            .env("AGENTBOLT_CHANNEL", &channel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        for _ in 0..100 {
            if Client::connect(&names, Pipe::Mcp).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("fixture daemon did not start");
    });
    let mut mcp = Mcp::start(&desk.with_file_name("agentbolt.exe"), &channel);
    let reg = mcp.tool(
        "desk_register",
        json!({"doing":"Automatic name verification", "requested_owner":"gui-named-agent"}),
    );
    assert_eq!(reg["provisional"], false);
    assert_eq!(reg["owner"], "gui-named-agent");
    assert!(reg["requested_owner"].is_null());
    let handle = reg["handle"].as_str().unwrap().to_string();
    let ask = json!({"handle":handle,"kind":"choose","title":"Automatic agent fixture","body":"Synthetic test: leave this ask unanswered.",
        "options":["Yes","No"],"default":"No","deadline":(time::OffsetDateTime::now_utc()+time::Duration::days(1)).format(&time::format_description::well_known::Rfc3339).unwrap(),"reversible":true,"priority":"P2"});
    let first = mcp.tool("desk_ask", ask.clone());
    let renewed = mcp.tool(
        "desk_register",
        json!({"handle":handle,"requested_owner":"gui-named-agent"}),
    );
    assert_eq!(renewed["owner"], "gui-named-agent");
    assert_eq!(renewed["provisional"], false);
    assert!(renewed["requested_owner"].is_null());
    let again = mcp.tool("desk_register", json!({}));
    assert_eq!(again["owner"], "gui-named-agent");
    assert_eq!(again["open_asks"][0]["id"], first["id"]);
    assert_eq!(mcp.tool("desk_ask", ask)["id"], first["id"]);
    println!(
        "Automatic naming, re-registration, retained ask and dedupe verified in both MCP result branches."
    );
}

#[test]
#[ignore = "manual: Anode; select all three asks, set Yes, Edited reply, quantity 7, then Send answers"]
fn batch_review_through_the_real_desk() {
    let channel = format!("gui-batch-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let data = Data(names.data_dir().unwrap());
    assert!(!data.0.exists(), "fixture channel must be fresh");
    let desk = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt-desk"));
    let _daemon = Process(
        Command::new(desk.with_file_name("agentbolt-daemon.exe"))
            .args(["--channel", &channel, "--headless"])
            .env("AGENTBOLT_CHANNEL", &channel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut client = rt.block_on(async {
        for _ in 0..100 {
            if let Ok(c) = Client::connect(&names, Pipe::Mcp).await {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("fixture daemon did not start");
    });
    let reg: RegisterResult = serde_json::from_value(
        rt.block_on(client.call(&McpOp::Register {
            owner: Some("batch-gui-test".into()),
            client: "GUI verification".into(),
            session_ref: None,
            cwd: None,
            doing: None,
            provisional_key: None,
        }))
        .unwrap(),
    )
    .unwrap();
    let base = AskInput {
        kind: AskKind::Choose,
        title: "Batch choice".into(),
        body: "Synthetic verification: choose Yes in the batch review.".into(),
        evidence: vec![],
        options: vec!["Yes".into(), "No".into()],
        fields: vec![],
        default: serde_json::json!("No"),
        deadline: (time::OffsetDateTime::now_utc() + time::Duration::days(1))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        reversible: true,
        priority: Priority::P2,
        p0_reason: None,
        dedupe_key: None,
    };
    let text = AskInput {
        kind: AskKind::Text,
        title: "Batch reply".into(),
        body: "Synthetic verification: enter Edited reply.".into(),
        options: vec![],
        default: serde_json::json!("Suggested reply"),
        ..base.clone()
    };
    let form = AskInput {
        kind: AskKind::Form,
        title: "Batch form".into(),
        body: "Synthetic verification: set quantity to 7. Empty quantity must disable sending."
            .into(),
        options: vec![],
        fields: vec![Field {
            id: "quantity".into(),
            label: "Quantity".into(),
            field_type: "number".into(),
            required: true,
            options: None,
        }],
        default: serde_json::json!({"quantity":5}),
        ..base.clone()
    };
    let mut ids = Vec::new();
    for ask in [base, text, form] {
        let result: AskResult = serde_json::from_value(
            rt.block_on(client.call(&McpOp::Ask {
                handle: reg.handle.clone(),
                ask,
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(result.placed, "open");
        ids.push(result.id);
    }
    let mut live_desk = Process(
        Command::new(&desk)
            .args(["--channel", &channel])
            .env("AGENTBOLT_CHANNEL", &channel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    println!(
        "Live fixture: channel {channel}, Desk pid {}. Select all, Review, Yes / Edited reply / quantity 7, Send answers.",
        live_desk.0.id()
    );
    if std::env::var("AGENTBOLT_DRAFT_RESTART_TEST").as_deref() == Ok("1") {
        println!(
            "Restart check: make those edits but DO NOT SEND. Add note 'Survives restart' to Batch choice. The test will restart the Desk once all edits are saved."
        );
        let start = Instant::now();
        loop {
            let saved = std::fs::read_dir(&data.0)
                .unwrap()
                .flatten()
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with("desk-drafts-")
                        && e.path().extension().is_some_and(|e| e == "json")
                })
                .any(|entry| {
                    let Ok(bytes) = std::fs::read(entry.path()) else {
                        return false;
                    };
                    let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
                        return false;
                    };
                    let d = &v["state"]["drafts"];
                    d[&ids[0]]["choice"] == "Yes"
                        && d[&ids[0]]["note"] == "Survives restart"
                        && d[&ids[1]]["text"] == "Edited reply"
                        && d[&ids[2]]["fields"]["quantity"]["Text"] == "7"
                });
            if saved {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(480),
                "waiting for saved edits timed out"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        live_desk.0.kill().unwrap();
        live_desk.0.wait().unwrap();
        live_desk.0 = Command::new(&desk)
            .args(["--channel", &channel])
            .env("AGENTBOLT_CHANNEL", &channel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        println!(
            "Desk restarted (pid {}). Review the three restored selections and edits, then Send answers.",
            live_desk.0.id()
        );
    }
    let started = Instant::now();
    let answers = loop {
        let result: AnswersResult = serde_json::from_value(
            rt.block_on(client.call(&McpOp::Answers {
                handle: reg.handle.clone(),
                ids: ids.clone(),
                wait_ms: 0,
            }))
            .unwrap(),
        )
        .unwrap();
        if result.answers.len() == 3 {
            break result.answers;
        }
        assert!(
            result.answers.is_empty(),
            "batch must commit all three answers together"
        );
        assert!(
            started.elapsed() < Duration::from_secs(480),
            "live GUI verification timed out"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let answer = |id: &str| {
        answers
            .iter()
            .find(|a| a.id == id)
            .unwrap()
            .answer
            .as_ref()
            .unwrap()
    };
    assert_eq!(answer(&ids[0]).choice.as_deref(), Some("Yes"));
    assert_eq!(answer(&ids[1]).choice.as_deref(), Some("Edited reply"));
    if std::env::var("AGENTBOLT_DRAFT_RESTART_TEST").as_deref() == Ok("1") {
        assert_eq!(answer(&ids[0]).note.as_deref(), Some("Survives restart"));
    }
    assert_eq!(
        answer(&ids[2]).values,
        Some(serde_json::json!({"quantity":7}))
    );
    assert!(
        answers
            .iter()
            .all(|a| a.answer.as_ref().unwrap().via == AnswerVia::Desk)
    );
    println!("All three edits arrived atomically with Desk authority.");
    drop(client);
}
