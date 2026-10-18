//! Opt-in live Desk fixture. Run in Anode, then use the visible Desk to answer the three asks together.
//! The test owns its processes and data and removes them on success or assertion failure.
#![cfg(windows)]

use agentbolt_core::channel::Names;
use agentbolt_core::model::{AnswerVia, AskInput, AskKind, Field, Priority};
use agentbolt_core::protocol::{AnswersResult, AskResult, McpOp, RegisterResult};
use agentbolt_ipc::{Client, Pipe};
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
        "Live fixture: channel {channel}, Desk pid {}. Select all, Review, Yes / Edited reply / quantity 7, Send answers.",
        _desk.0.id()
    );
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
