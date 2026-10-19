//! Opt-in native interaction evidence, using only a fresh throwaway channel and owned processes.
#![cfg(windows)]

use agentbolt_core::{
    channel::Names,
    metrics::Report,
    model::{AnswerVia, AskInput, AskKind, Field, Priority},
    protocol::{AnswersResult, AskResult, DeskOp, McpOp, RegisterResult},
    service::{ClientInfo, Service},
    service_impl::StoreService,
};
use agentbolt_ipc::{Client, Pipe};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
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
fn launch(path: &Path, channel: &str, extra: &[&str]) -> Process {
    Process(
        Command::new(path)
            .args(["--channel", channel])
            .args(extra)
            .env("AGENTBOLT_CHANNEL", channel)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

#[test]
#[ignore = "Anode: answer seven synthetic questions, inspect Activity, then Finish review"]
fn native_answer_interactions_and_restored_draft_are_measured() {
    let channel = format!("gui-interaction-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let data = names.data_dir().unwrap();
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::create_dir(&data).expect("fresh fixture channel required");
    let _data = Data(data.clone());
    let service = StoreService::open(&names.db_path().unwrap())
        .unwrap()
        .with_channel(&channel)
        .with_machine("interaction-fixture");
    let mut questions = Vec::new();
    for (index, title) in [
        "01 Direct choice",
        "02 Restore edited reply",
        "03 Untouched form",
        "04 Untouched reply",
        "05 Unnecessary question",
        "06 Clear edited note",
        "07 Finish review",
    ]
    .into_iter()
    .enumerate()
    {
        let register: RegisterResult = serde_json::from_value(
            service
                .mcp(
                    &ClientInfo::default(),
                    McpOp::Register {
                        owner: Some(format!("interaction-{index}")),
                        client: "Native interaction fixture".into(),
                        session_ref: None,
                        cwd: None,
                        doing: None,
                        provisional_key: None,
                    },
                )
                .unwrap()
                .0,
        )
        .unwrap();
        let mut input = AskInput {
            kind: AskKind::Choose,
            title: title.into(),
            body: "Synthetic native interaction check. Answer No without adding a note.".into(),
            evidence: vec![],
            options: vec!["Yes".into(), "No".into()],
            fields: vec![],
            default: json!("No"),
            deadline: (OffsetDateTime::now_utc()
                + time::Duration::days(1)
                + time::Duration::seconds(index as i64))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
            reversible: true,
            priority: Priority::P2,
            p0_reason: None,
            dedupe_key: None,
        };
        match index {
            1 | 3 => {
                input.kind=AskKind::Text;
                input.options.clear();
                input.default=json!("Suggested reply");
                input.body=if index==1 {
                    "Replace the reply with Kept across restart. The fixture will restart only its Desk after the edit is saved. Then send the restored reply."
                } else { "Send Suggested reply without editing it or the note." }.into();
            }
            2 => {
                input.kind=AskKind::Form;
                input.options.clear();
                input.fields=vec![Field{id:"count".into(),label:"Count".into(),field_type:"number".into(),required:true,options:None}];
                input.default=json!({"count":3});
                input.body="Send the prefilled count 3 without editing it or the note.".into();
            }
            4 => input.body="Click Not needed without adding a note.".into(),
            5 => input.body="Enter Temporary note, clear it completely, then answer No. Editing still counts after text is cleared.".into(),
            6 => {
                input.options=vec!["Finish".into(),"Keep reviewing".into()];
                input.default=json!("Keep reviewing");
                input.body="After answering 01–06, inspect Activity for 4 / 7 (57%). Then click Finish here to verify persisted evidence and clean up the fixture.".into();
            }
            _ => (),
        }
        let ask: AskResult = serde_json::from_value(
            service
                .mcp(
                    &ClientInfo::default(),
                    McpOp::Ask {
                        handle: register.handle.clone(),
                        ask: input,
                    },
                )
                .unwrap()
                .0,
        )
        .unwrap();
        assert_eq!(ask.placed, "open");
        questions.push((register.handle, ask.id));
    }
    drop(service);
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt-desk"));
    let daemon = launch(
        &executable.with_file_name("agentbolt-daemon.exe"),
        &channel,
        &["--headless"],
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut client = rt.block_on(async {
        for _ in 0..100 {
            if let Ok(client) = Client::connect(&names, Pipe::Mcp).await {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("fixture daemon did not start")
    });
    let mut desk = launch(&executable, &channel, &[]);
    println!(
        "Channel {channel}; Desk pid {}. Answer 01–06, inspect Activity, then Finish 07.",
        desk.0.id()
    );
    let mut restarted = false;
    let mut answers = BTreeMap::new();
    let started = Instant::now();
    loop {
        if !restarted {
            for entry in std::fs::read_dir(&data).unwrap().flatten() {
                if !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("desk-drafts-")
                {
                    continue;
                }
                let state: Value = match std::fs::read(entry.path())
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                {
                    Some(state) => state,
                    None => continue,
                };
                let draft = &state["state"]["drafts"][&questions[1].1];
                if draft["text"] == "Kept across restart" {
                    assert_eq!(
                        draft["one_click"], false,
                        "native edit must record interaction before restart"
                    );
                    drop(desk);
                    desk = launch(&executable, &channel, &[]);
                    restarted = true;
                    println!(
                        "Saved edited reply verified; restarted owned Desk as pid {}. Send the restored reply.",
                        desk.0.id()
                    );
                    break;
                }
            }
        }
        for (index, (handle, id)) in questions.iter().enumerate() {
            if answers.contains_key(&index) {
                continue;
            }
            let result: AnswersResult = serde_json::from_value(
                rt.block_on(client.call(&McpOp::Answers {
                    handle: handle.clone(),
                    ids: vec![id.clone()],
                    wait_ms: 0,
                }))
                .unwrap(),
            )
            .unwrap();
            if let Some(answer) = result.answers.first().and_then(|ask| ask.answer.clone()) {
                println!("Question {:02} answered via {:?}.", index + 1, answer.via);
                answers.insert(index, answer);
            }
        }
        if answers.contains_key(&6) {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(900),
            "native interaction review timed out"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    assert!(
        restarted,
        "edited draft must be restored in a new Desk process"
    );
    assert_eq!(
        answers.len(),
        7,
        "finish only after all native interactions"
    );
    assert_eq!(answers[&0].choice.as_deref(), Some("No"));
    assert_eq!(answers[&1].choice.as_deref(), Some("Kept across restart"));
    assert_eq!(answers[&2].values, Some(json!({"count":3})));
    assert_eq!(answers[&3].choice.as_deref(), Some("Suggested reply"));
    assert_eq!(answers[&4].via, AnswerVia::NotNeeded);
    assert_eq!(answers[&5].choice.as_deref(), Some("No"));
    assert_eq!(answers[&5].note, None);
    assert_eq!(answers[&6].choice.as_deref(), Some("Finish"));
    drop(desk);
    drop(client);
    drop(daemon);
    // Read the persisted result only after the sole writer has exited. No answer is written here.
    let service = StoreService::open(&names.db_path().unwrap())
        .unwrap()
        .with_channel(&channel);
    let report: Report = serde_json::from_value(service.desk(DeskOp::Metrics).unwrap().0).unwrap();
    let direct = report.current.one_click.unwrap();
    assert_eq!(direct.shown, 7);
    assert_eq!(direct.one_click_answers, 5);
    assert_eq!(direct.other_human_answers, 2);
    assert_eq!(direct.unmeasured_human_answers, 0);
    assert_eq!(direct.share, Some(5.0 / 7.0));
    println!(
        "Persisted native evidence: 5 direct, 2 edited, 7 shown, no unmeasured answers. Fixture complete."
    );
}
