//! Opt-in release startup probe. Run in Anode, with all three release binaries built.
//! Each launch has its own process, an isolated channel and a real daemon; all fixtures are removed.
#![cfg(windows)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_core::protocol::McpOp;
use agentbolt_ipc::{Client, Pipe};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::HSTRING;

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

fn trace_events(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}

#[allow(
    clippy::assertions_on_constants,
    reason = "An ignored timing probe must compile in debug but refuse debug measurements"
)]
fn require_release() {
    assert!(!cfg!(debug_assertions), "measure the release build");
}

#[test]
#[ignore = "manual: release build, in Anode; verifies preload, repeated reopen, default-clock safety and daemon crash cleanup"]
fn warm_desk_reopens_fast_without_showing_unseen_asks() {
    use agentbolt_core::model::{AskInput, AskKind, Priority};
    use agentbolt_core::protocol::{AskResult, RegisterResult};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    require_release();
    let channel = format!("warm-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let data = Data(names.data_dir().unwrap());
    std::fs::create_dir_all(&data.0).unwrap();
    let trace = data.0.join("trace.json");
    let desk = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt-desk"));
    let daemon = Process(
        Command::new(desk.with_file_name("agentbolt-daemon.exe"))
            .args(["--channel", &channel])
            .env("AGENTBOLT_CHANNEL", &channel)
            .env("AGENTBOLT_STARTUP_LOG", &trace)
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
        panic!("daemon did not start");
    });
    let reg: RegisterResult = serde_json::from_value(
        rt.block_on(client.call(&McpOp::Register {
            owner: Some("warm-test".into()),
            client: "startup-test".into(),
            session_ref: None,
            cwd: None,
            doing: None,
            provisional_key: None,
        }))
        .unwrap(),
    )
    .unwrap();
    let title = HSTRING::from(format!("Agentbolt ({channel})"));
    let started = Instant::now();
    let hwnd = loop {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "warm Desk did not preload"
        );
        if let Ok(hwnd) = unsafe { FindWindowW(None, &title) } {
            assert!(
                !unsafe { IsWindowVisible(hwnd) }.as_bool(),
                "preload flashed a window"
            );
            if trace_events(&trace)
                .iter()
                .any(|e| e["stage"] == "first_frame_painted")
            {
                break hwnd;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let mut warm_pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut warm_pid));
    }
    if std::env::var("AGENTBOLT_MEASURE_IDLE").as_deref() == Ok("1") {
        idle_cpu(&desk, &daemon.0, warm_pid, hwnd);
    }
    let mut samples = Vec::new();
    for run in 0..5 {
        let ask = AskInput {
            kind: AskKind::Choose,
            title: format!("Synthetic startup check {run}"),
            body: String::new(),
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
            dedupe_key: Some(format!("startup-{run}")),
        };
        let op = McpOp::Ask {
            handle: reg.handle.clone(),
            ask,
        };
        let result: AskResult =
            serde_json::from_value(rt.block_on(client.call(&op)).unwrap()).unwrap();
        let hidden_until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < hidden_until {
            assert!(
                !unsafe { IsWindowVisible(hwnd) }.as_bool(),
                "hidden Desk appeared on its own"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let existing: AskResult =
            serde_json::from_value(rt.block_on(client.call(&op)).unwrap()).unwrap();
        assert!(
            existing.existing.unwrap().shown_at.is_none(),
            "an unseen ask was marked shown"
        );
        let before = trace_events(&trace)
            .iter()
            .filter(|e| e["stage"] == "inbox_refreshed")
            .count();
        let start = Instant::now();
        let _activation = Process(
            Command::new(&desk)
                .args(["--channel", &channel])
                .env("AGENTBOLT_CHANNEL", &channel)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        while !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "Desk failed to reopen"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let window_ms = start.elapsed().as_secs_f64() * 1000.0;
        while trace_events(&trace)
            .iter()
            .filter(|e| e["stage"] == "inbox_refreshed")
            .count()
            == before
        {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "Inbox did not refresh; foreground={:?}; trace={:?}",
                unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() },
                trace_events(&trace)
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let inbox_ms = start.elapsed().as_secs_f64() * 1000.0;
        let existing: AskResult =
            serde_json::from_value(rt.block_on(client.call(&op)).unwrap()).unwrap();
        assert!(
            existing.existing.unwrap().shown_at.is_some(),
            "visible Inbox did not mark the ask shown"
        );
        let sample = serde_json::json!({"run":run,"window_ms":window_ms,"inbox_ms":inbox_ms});
        println!("{sample}");
        samples.push(sample);
        rt.block_on(client.call(&McpOp::Cancel {
            handle: reg.handle.clone(),
            id: result.id,
        }))
        .unwrap();
        unsafe {
            PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
        }
        let closing = Instant::now();
        while unsafe { IsWindowVisible(hwnd) }.as_bool() {
            assert!(
                closing.elapsed() < Duration::from_secs(3),
                "Close did not hide the Desk"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        assert_eq!(pid, warm_pid, "the warm process must survive closing");
    }
    let report = desk.parent().unwrap().join("warm-startup-results.json");
    std::fs::write(&report, serde_json::to_vec_pretty(&samples).unwrap()).unwrap();
    drop(client);
    drop(daemon); // simulate a crash, so the kernel job must also reap the warm window
    let stopping = Instant::now();
    while unsafe { FindWindowW(None, &title) }.is_ok() {
        assert!(
            stopping.elapsed() < Duration::from_secs(3),
            "daemon crash orphaned the warm Desk"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        samples
            .iter()
            .all(|s| s["window_ms"].as_f64().unwrap() <= 300.0),
        "warm window exceeds 300 ms"
    );
    assert!(
        samples
            .iter()
            .all(|s| s["inbox_ms"].as_f64().unwrap() <= 1000.0),
        "warm Inbox exceeds 1 s"
    );
}

fn idle_cpu(
    desk: &std::path::Path,
    daemon: &Child,
    warm_pid: u32,
    hwnd: windows::Win32::Foundation::HWND,
) {
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    fn cpu(handle: &Handle) -> u64 {
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe {
            GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user).unwrap();
        }
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        ticks(kernel) + ticks(user)
    }
    let daemon_handle = Handle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, daemon.id()) }.unwrap(),
    );
    let desk_handle =
        Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, warm_pid) }.unwrap());
    let before = [cpu(&daemon_handle), cpu(&desk_handle)];
    let start = Instant::now();
    println!("Measuring daemon + hidden Desk CPU for 600 seconds...");
    while start.elapsed() < Duration::from_secs(600) {
        assert!(
            !unsafe { IsWindowVisible(hwnd) }.as_bool(),
            "idle Desk became visible"
        );
        std::thread::sleep(Duration::from_secs(1));
    }
    let wall = start.elapsed().as_secs_f64();
    let after = [cpu(&daemon_handle), cpu(&desk_handle)];
    let percent = |i: usize| (after[i] - before[i]) as f64 / 10_000_000.0 / wall * 100.0;
    let report = serde_json::json!({"duration_seconds":wall,"daemon_percent_one_core":percent(0),"desk_percent_one_core":percent(1),"combined_percent_one_core":percent(0)+percent(1)});
    println!("{report}");
    std::fs::write(
        desk.parent().unwrap().join("idle-cpu-results.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(
        percent(0) + percent(1) < 0.5,
        "idle CPU exceeds 0.5% of one core"
    );
}

#[test]
#[ignore = "manual: release build in the Anode desktop; cargo test -p agentbolt-desk --release --test startup -- --ignored --nocapture"]
fn cold_startup_measurement() {
    require_release();
    let channel = format!("startup-{}", std::process::id());
    let names = Names::for_channel(&channel).unwrap();
    assert!(!names.is_main());
    let _data = Data(names.data_dir().unwrap());
    let desk = PathBuf::from(env!("CARGO_BIN_EXE_agentbolt-desk"));
    let daemon = desk.with_file_name("agentbolt-daemon.exe");
    let mut daemon = Process(
        Command::new(daemon)
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
            if let Ok(mut c) = Client::connect(&names, Pipe::Mcp).await {
                c.call(&McpOp::Status).await.unwrap();
                return;
            }
            assert!(daemon.0.try_wait().unwrap().is_none(), "daemon exited");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("daemon did not start");
    });
    let title = HSTRING::from(format!("Agentbolt ({channel})"));
    let mut samples = Vec::new();
    for run in 0..5 {
        let trace = _data.0.join(format!("startup-{run}.json"));
        let start = Instant::now();
        let mut app = Process(
            Command::new(&desk)
                .args(["--channel", &channel])
                .env("AGENTBOLT_CHANNEL", &channel)
                .env("AGENTBOLT_STARTUP_LOG", &trace)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let mut visible_ms = None;
        let events = loop {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "window or Inbox never appeared"
            );
            assert!(app.0.try_wait().unwrap().is_none(), "Desk exited");
            if visible_ms.is_none()
                && let Ok(hwnd) = unsafe { FindWindowW(None, &title) }
            {
                let mut pid = 0;
                unsafe {
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                }
                if pid == app.0.id() && unsafe { IsWindowVisible(hwnd) }.as_bool() {
                    visible_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            let events: Vec<serde_json::Value> = std::fs::read_to_string(&trace)
                .unwrap_or_default()
                .lines()
                .filter_map(|s| serde_json::from_str(s).ok())
                .collect();
            if visible_ms.is_some() && events.iter().any(|e| e["stage"] == "inbox_received") {
                break events;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let inbox_ms = start.elapsed().as_secs_f64() * 1000.0;
        let sample = serde_json::json!({"run": run, "window_ms": visible_ms.unwrap(), "inbox_ms": inbox_ms, "events": events});
        println!("{sample}");
        samples.push(sample);
        drop(app);
    }
    let report = desk.parent().unwrap().join("startup-results.json");
    std::fs::write(&report, serde_json::to_vec_pretty(&samples).unwrap()).unwrap();
    println!("Report: {}", report.display());
    if std::env::var("AGENTBOLT_REQUIRE_COLD_TARGET").as_deref() == Ok("1") {
        assert!(
            samples
                .iter()
                .all(|s| s["window_ms"].as_f64().unwrap() <= 300.0),
            "window exceeds 300 ms; inspect startup-results.json"
        );
        assert!(
            samples
                .iter()
                .all(|s| s["inbox_ms"].as_f64().unwrap() <= 1000.0),
            "Inbox exceeds 1 s; inspect startup-results.json"
        );
    }
}
