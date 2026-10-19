//! Opt-in native review of local, remote and unknown evidence origins on a throwaway channel.
#![cfg(windows)]

use agentbolt_core::{
    board::{BoardOp, PostInput},
    channel::Names,
    model::{AskInput, AskKind, Evidence, Priority},
    protocol::{AnswersResult, AskResult, McpOp, RegisterResult, RemoteRegistration},
    service::{ClientInfo, Service},
    service_impl::StoreService,
};
use agentbolt_ipc::{Client, Pipe};
use serde_json::{Value, json};
use std::{
    os::windows::process::CommandExt,
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
#[ignore = "Anode: inspect origin controls and copied references, then answer Finish origin review"]
fn evidence_controls_respect_the_originating_computer() {
    const SESSION: &str = "11111111-2222-4333-8444-555555555555";
    const CHAT: &str = "codex://threads/11111111-2222-4333-8444-555555555555";
    let channel = format!("gui-origin-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let data = names.data_dir().unwrap();
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::create_dir(&data).expect("fresh fixture channel required");
    let _data = Data(data.clone());
    let file = data.join("synthetic-evidence.md");
    std::fs::write(&file, "Synthetic evidence only. No action is authorized.\n").unwrap();
    let path = file.display().to_string();
    let service = StoreService::open(&names.db_path().unwrap())
        .unwrap()
        .with_channel(&channel)
        .with_machine("origin-fixture");
    let call = |op| service.mcp(&ClientInfo::default(), op).unwrap().0;
    let local: RegisterResult = serde_json::from_value(call(McpOp::Register {
        owner: Some("origin-local".into()),
        client: "codex".into(),
        session_ref: None,
        cwd: Some(data.display().to_string()),
        doing: None,
        provisional_key: None,
    }))
    .unwrap();
    let remote: RegisterResult = serde_json::from_value(call(McpOp::RegisterRemote {
        machine: "fixture-laptop".into(),
        registration: RemoteRegistration {
            owner: Some("origin-remote".into()),
            client: "codex".into(),
            session_ref: Some(SESSION.into()),
            cwd: Some(data.display().to_string()),
            doing: Some("Synthetic remote evidence".into()),
            provisional_key: None,
        },
    }))
    .unwrap();
    let input = AskInput {
        kind: AskKind::Choose, title: "01 Remote question".into(),
        body: "File and chat references belong to fixture-laptop, even though the same file exists here. Inspect and copy them; do not launch the synthetic chat or web link.".into(),
        evidence: vec![Evidence {label:"Remote file".into(),url:path.clone()}, Evidence {label:"Remote chat".into(),url:CHAT.into()}, Evidence {label:"Web reference".into(),url:"https://example.com/evidence".into()}],
        options: vec!["Keep".into(),"Change".into()], fields: vec![], default:json!("Keep"),
        deadline: (OffsetDateTime::now_utc()+time::Duration::days(1)).format(&time::format_description::well_known::Rfc3339).unwrap(),
        reversible: true, priority:Priority::P2, p0_reason:None, dedupe_key:None,
    };
    call(McpOp::Ask {
        handle: remote.handle.clone(),
        ask: input.clone(),
    });
    let unknown: AskResult = serde_json::from_value(call(McpOp::Ask {
        handle: remote.handle.clone(),
        ask: AskInput {
            title: "03 Unknown origin question".into(),
            body: "This older question retains a session reference but no source computer. Only copying the reference is safe.".into(),
            ..input.clone()
        },
    }))
    .unwrap();
    let finish:AskResult=serde_json::from_value(call(McpOp::Ask {
        handle:local.handle.clone(),ask:AskInput {
            title:"02 Finish origin review".into(),body:"Inspect the remote ask, Agents, and three Board messages. Paste Clipboard round trip: café — 雪 🚀 into this note from the OS clipboard. Verify Activity export, then copy the remote session last and answer Finish.".into(),
            options:vec!["Finish".into(),"Keep reviewing".into()],default:json!("Keep reviewing"),evidence:vec![],..input
        },
    })).unwrap();
    let mut legacy = String::new();
    for (handle, title) in [
        (&remote.handle, "Remote evidence"),
        (&local.handle, "Local evidence"),
        (&local.handle, "Legacy evidence"),
    ] {
        let result: Value = call(McpOp::Board {
            handle: handle.clone(),
            action: BoardOp::Post {
                input: PostInput {
                    to: Default::default(),
                    title: title.into(),
                    body: "Synthetic origin check; do not launch the chat or web reference.".into(),
                    references: vec![
                        path.clone(),
                        CHAT.into(),
                        "https://example.com/evidence".into(),
                    ],
                    thread: None,
                    dedupe_key: None,
                },
            },
        });
        if title == "Legacy evidence" {
            legacy = result["id"].as_str().unwrap().into();
        }
    }
    drop(service);
    // Simulate a message predating immutable origin tracking, before the real writer starts.
    let db = agentbolt_core::store::open_file(&names.db_path().unwrap()).unwrap();
    db.execute(
        "UPDATE asks SET asking_machine=NULL WHERE id=?1",
        [&unknown.id],
    )
    .unwrap();
    db.execute(
        "UPDATE board_messages SET sender_machine=NULL WHERE id=?1",
        [&legacy],
    )
    .unwrap();
    drop(db);
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt-desk"));
    let _daemon = launch(
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
            if let Ok(c) = Client::connect(&names, Pipe::Mcp).await {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("fixture daemon did not start")
    });
    let desk = launch(&executable, &channel, &[]);
    println!(
        "Channel {channel}; Desk pid {}. Local file {path}; remote session {SESSION}. Inspect the origin controls, then Finish.",
        desk.0.id()
    );
    let started = Instant::now();
    loop {
        let result: AnswersResult = serde_json::from_value(
            rt.block_on(client.call(&McpOp::Answers {
                handle: local.handle.clone(),
                ids: vec![finish.id.clone()],
                wait_ms: 0,
            }))
            .unwrap(),
        )
        .unwrap();
        if let Some(answer) = result.answers.first().and_then(|a| a.answer.as_ref()) {
            assert_eq!(answer.choice.as_deref(), Some("Finish"));
            assert_eq!(
                answer.note.as_deref(),
                Some("Clipboard round trip: café — 雪 🚀")
            );
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(900),
            "origin review timed out"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    // Read from a separate OS process, so egui's app-local fallback cannot pass this check.
    let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let copied = Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); Get-Clipboard -Raw",
        ])
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(copied.status.success());
    assert_eq!(String::from_utf8(copied.stdout).unwrap().trim(), SESSION);
    println!(
        "Native origin review and OS clipboard copy/paste completed; owned processes and data are being removed."
    );
}
