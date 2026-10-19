//! Synthetic, opt-in native UI fixture. All data and processes belong to a fresh throwaway channel.
//! Run in Anode, inspect each tab and page, then send "Keep across pages" on Inbox question 00.
#![cfg(windows)]

use agentbolt_core::board::{BoardOp, Destination, PostInput, RequestInput};
use agentbolt_core::channel::Names;
use agentbolt_core::holds::HoldOp;
use agentbolt_core::model::{AnswerVia, AskInput, AskKind, Priority};
use agentbolt_core::protocol::{AnswersResult, AskResult, DeskOp, McpOp, RegisterResult};
use agentbolt_core::service::{ClientInfo, Service};
use agentbolt_core::service_impl::StoreService;
use agentbolt_ipc::{Client, Pipe};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;

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

fn mcp(service: &StoreService, op: McpOp) -> Value {
    service.mcp(&ClientInfo::default(), op).unwrap().0
}

#[test]
#[ignore = "Anode: inspect expanded tabs/paging, then send Keep across pages on Inbox question 00"]
fn expanded_tabs_and_drafts_through_the_real_desk() {
    let channel = format!("gui-expanded-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let path = names.data_dir().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::create_dir(&path).expect("fixture channel must be fresh");
    let _data = Data(path);
    let clock = Arc::new(Mutex::new(
        OffsetDateTime::now_utc() - time::Duration::minutes(5),
    ));
    let source = clock.clone();
    let service = StoreService::open(&names.db_path().unwrap())
        .unwrap()
        .with_channel(&channel)
        .with_machine("gui-fixture")
        .with_clock(Arc::new(move || *source.lock().unwrap()));
    let mut finish = None;
    for i in 0..60 {
        *clock.lock().unwrap() += time::Duration::seconds(1);
        let register: RegisterResult = serde_json::from_value(mcp(
            &service,
            McpOp::Register {
                owner: Some(format!("fixture-{i:02}")),
                client: "GUI fixture".into(),
                session_ref: None,
                cwd: None,
                doing: Some(format!("Synthetic work {i:02}")),
                provisional_key: None,
            },
        ))
        .unwrap();
        let handle = register.handle;
        let base = AskInput {
            kind: AskKind::Choose,
            title: format!("Inbox question {i:02}"),
            body: "Synthetic question for native pagination review.".into(),
            evidence: vec![],
            options: vec!["Yes".into(), "No".into()],
            fields: vec![],
            default: json!("No"),
            deadline: (*clock.lock().unwrap() + time::Duration::days(1))
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            reversible: true,
            priority: Priority::P2,
            p0_reason: None,
            dedupe_key: None,
        };
        let open = if i == 0 {
            AskInput {
            kind: AskKind::Text, options: vec![], default: json!("Suggested reply"),
            body: "Enter Keep across pages. Navigate to another Inbox page and the other tabs. Return here and send the saved reply only when review is complete; this closes and cleans the fixture.".into(),
            ..base.clone()
        }
        } else {
            base.clone()
        };
        let ask: AskResult = serde_json::from_value(mcp(
            &service,
            McpOp::Ask {
                handle: handle.clone(),
                ask: open,
            },
        ))
        .unwrap();
        assert_eq!(ask.placed, "open");
        if i == 0 {
            finish = Some((handle.clone(), ask.id));
        }
        let archived: AskResult = serde_json::from_value(mcp(
            &service,
            McpOp::Ask {
                handle: handle.clone(),
                ask: AskInput {
                    title: format!("Archived decision {i:02}"),
                    body: "Synthetic closed decision for History search and paging.".into(),
                    ..base
                },
            },
        ))
        .unwrap();
        service
            .desk(DeskOp::Answer {
                one_click: None,
                ask_id: archived.id.clone(),
                choice: Some("Yes".into()),
                values: None,
                note: Some("Synthetic seeded answer".into()),
            })
            .unwrap();
        if i == 0 {
            for attempt in 0..55 {
                service
                    .record_delivery(
                        &archived.id,
                        "fixture",
                        attempt % 2 == 0,
                        Some(&format!("Synthetic delivery attempt {attempt:02}")),
                    )
                    .unwrap();
            }
        }
        mcp(
            &service,
            McpOp::Board {
                handle: handle.clone(),
                action: BoardOp::Profile {
                    roles: Some(vec!["builder".into(), "reviewer".into()]),
                    repo: Some("example/project".into()),
                    clear_repo: false,
                },
            },
        );
        let request = mcp(
            &service,
            McpOp::Board {
                handle: handle.clone(),
                action: BoardOp::Request {
                    input: RequestInput {
                        title: format!("Handoff {i:02}"),
                        body: "Synthetic handoff with a concrete path.".into(),
                        to_role: Some("reviewer".into()),
                        repo: Some("example/project".into()),
                        paths: vec![format!("src/item-{i:02}/**")],
                        dedupe_key: None,
                        beads_issue: None,
                    },
                },
            },
        );
        if i == 59 {
            let id = request["request"]["id"].as_str().unwrap().to_owned();
            let claim = mcp(
                &service,
                McpOp::Board {
                    handle: handle.clone(),
                    action: BoardOp::Claim { id: id.clone() },
                },
            );
            mcp(
                &service,
                McpOp::Board {
                    handle: handle.clone(),
                    action: BoardOp::Done {
                        id,
                        fence: claim["fence"].as_u64().unwrap(),
                        result: "Synthetic dependency unavailable".into(),
                        failed: true,
                    },
                },
            );
        }
        mcp(
            &service,
            McpOp::Board {
                handle: handle.clone(),
                action: BoardOp::Post {
                    input: PostInput {
                        to: Destination::Everyone,
                        title: format!("Update {i:02}"),
                        body: "Synthetic board update.".into(),
                        references: vec!["https://example.com/review".into()],
                        thread: None,
                        dedupe_key: None,
                    },
                },
            },
        );
        mcp(
            &service,
            McpOp::Holds {
                handle,
                action: HoldOp::Acquire {
                    repo: "example/project".into(),
                    paths: vec![format!("src/item-{i:02}/**")],
                    note: format!("Synthetic hold {i:02}"),
                    id: None,
                },
            },
        );
    }
    // The daemon is the only writer from this point. No settings enable scans, pushes or chat delivery.
    drop(service);
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
    let _desk = Process(
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
        "Channel {channel}, Desk pid {}. 60 questions, History entries, handoffs, messages, agents and holds. Send Keep across pages on Inbox question 00 to finish.",
        _desk.0.id()
    );
    let (handle, id) = finish.unwrap();
    let start = Instant::now();
    loop {
        let result: AnswersResult = serde_json::from_value(
            rt.block_on(client.call(&McpOp::Answers {
                handle: handle.clone(),
                ids: vec![id.clone()],
                wait_ms: 0,
            }))
            .unwrap(),
        )
        .unwrap();
        if let Some(answer) = result.answers.first().and_then(|ask| ask.answer.as_ref()) {
            assert_eq!(answer.via, AnswerVia::Desk);
            assert_eq!(answer.choice.as_deref(), Some("Keep across pages"));
            println!("Saved reply committed by the Desk and read back over MCP. Fixture complete.");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(900),
            "native review timed out"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}
