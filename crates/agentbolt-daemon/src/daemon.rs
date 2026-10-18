//! The daemon's async half: both pipes, the once-a-second tick, the `answers` wait, and acting on effects
//! (tray count, ntfy push, toasts, Codex delivery). The tray itself runs on the main thread (`tray.rs`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_core::model::Settings;
use agentbolt_core::protocol::{DeskOp, McpOp};
use agentbolt_core::service::{ClientInfo, Effect, Service};
use agentbolt_ipc::Handler;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::log::{error, info, warning};
use crate::{codex, ntfy};

/// The longest `answers` wait the daemon honours.
pub const MAX_WAIT_MS: u64 = 600_000;
/// How often an `answers` wait re-reads the store.
pub const POLL: Duration = Duration::from_millis(250);

/// What the daemon needs beyond the `Service` trait.
pub trait DaemonService: Service {
    fn phone_work(
        &self,
        _generation: &str,
        _op: agentbolt_core::phone::Work,
    ) -> Result<agentbolt_core::phone::ResultValue, String> {
        Err("phone transport unavailable".into())
    }
    fn hold_config(&self) -> Result<agentbolt_core::holds::Config, String> {
        Ok(Default::default())
    }
    fn save_hold_scan(&self, _scan: agentbolt_core::holds::Scan) -> Result<(), String> {
        Ok(())
    }
    fn toast_allowed(&self, _ask_id: &str) -> bool {
        false
    }
    fn notification_counts(&self) -> Result<(u32, Option<String>), String> {
        Err("notification counts unavailable".into())
    }
    fn codex_delivery_allowed(&self, _ask_id: &str, _thread: &str) -> bool {
        false
    }
    fn record_delivery(
        &self,
        ask_id: &str,
        to: &str,
        ok: bool,
        detail: Option<&str>,
    ) -> Result<(), String>;
}

/// Something for the effects task to act on.
#[derive(Debug, Clone)]
pub enum Event {
    Effect(Effect),
    /// Settings were changed (by the Desk, or by the tray's Pause item).
    Settings(Settings),
}

/// Where the tray learns what to show. A no-op when there is no tray.
pub trait Ui: Send + Sync + 'static {
    fn counts(&self, open: u32);
    fn paused(&self, paused: bool);
}

pub struct NoUi;

impl Ui for NoUi {
    fn counts(&self, _open: u32) {}
    fn paused(&self, _paused: bool) {}
}

pub struct Shared {
    pub names: Names,
    pub svc: Arc<dyn DaemonService>,
    events: mpsc::UnboundedSender<Event>,
}

impl Shared {
    pub fn new(
        names: Names,
        svc: Arc<dyn DaemonService>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<Event>) {
        let (events, rx) = mpsc::unbounded_channel();
        (Arc::new(Shared { names, svc, events }), rx)
    }

    pub fn emit(&self, effects: Vec<Effect>) {
        for effect in effects {
            let _ = self.events.send(Event::Effect(effect));
        }
    }

    pub fn settings_changed(&self, settings: Settings) {
        let _ = self.events.send(Event::Settings(settings));
    }

    /// One MCP op on a blocking thread (the store is synchronous SQLite).
    async fn mcp(&self, client: &ClientInfo, op: McpOp) -> Result<Value, String> {
        let svc = self.svc.clone();
        let client = client.clone();
        let (value, effects) = tokio::task::spawn_blocking(move || svc.mcp(&client, op))
            .await
            .map_err(|e| format!("internal error: {e}"))??;
        self.emit(effects);
        Ok(value)
    }

    async fn desk(&self, op: DeskOp) -> Result<Value, String> {
        let svc = self.svc.clone();
        let (value, effects) = tokio::task::spawn_blocking(move || svc.desk(op))
            .await
            .map_err(|e| format!("internal error: {e}"))??;
        self.emit(effects);
        Ok(value)
    }

    /// The current settings (synchronous; the tray calls it on the main thread).
    pub fn get_settings(&self) -> Result<Settings, String> {
        let (value, effects) = self.svc.desk(DeskOp::GetSettings)?;
        self.emit(effects);
        serde_json::from_value(value).map_err(|e| format!("settings: {e}"))
    }

    /// Flip `toasts_off` through the service, so the Desk and the tray agree. Returns the new value.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn toggle_pause(&self) -> Result<bool, String> {
        let mut settings = self.get_settings()?;
        settings.toasts_off = !settings.toasts_off;
        let (_, effects) = self.svc.desk(DeskOp::SetSettings {
            settings: settings.clone(),
        })?;
        self.emit(effects);
        let paused = settings.toasts_off;
        self.settings_changed(settings);
        Ok(paused)
    }

    /// The Inbox counts right now (start-up tooltip).
    pub fn counts(&self) -> Option<u32> {
        let (value, effects) = self.svc.desk(DeskOp::Status).ok()?;
        self.emit(effects);
        Some(value["open"].as_u64()? as u32)
    }
}

/// True when an `answers` result is worth returning before the wait ends: answers arrived, or nothing is left
/// that could get one (`waiting` empty).
pub fn answers_ready(value: &Value) -> bool {
    match value {
        Value::Array(list) => !list.is_empty(),
        Value::Object(map) => {
            let non_empty = |k: &str| {
                map.get(k)
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty())
            };
            let empty = |k: &str| {
                map.get(k)
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.is_empty())
            };
            non_empty("answers") || empty("waiting")
        }
        _ => true,
    }
}

/// `answers` with a wait: re-read every 250 ms until there is something or the wait (capped) runs out.
async fn answers_wait(
    shared: &Shared,
    client: &ClientInfo,
    op: McpOp,
    wait_ms: u64,
) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_millis(wait_ms.min(MAX_WAIT_MS));
    loop {
        let value = shared.mcp(client, op.clone()).await?;
        let now = Instant::now();
        if answers_ready(&value) || now >= deadline {
            return Ok(value);
        }
        tokio::time::sleep(POLL.min(deadline - now)).await;
    }
}

pub struct McpHandler(pub Arc<Shared>);

impl Handler for McpHandler {
    async fn handle(&self, client: &ClientInfo, op: Value) -> Result<Value, String> {
        let op: McpOp =
            serde_json::from_value(op).map_err(|e| format!("unknown or malformed op: {e}"))?;
        match op {
            McpOp::Answers { wait_ms, .. } if wait_ms > 0 => {
                answers_wait(&self.0, client, op, wait_ms).await
            }
            op => self.0.mcp(client, op).await,
        }
    }

    fn disconnected(&self, client: &ClientInfo) {
        self.0.svc.disconnected(client.connection);
    }

    fn listen_error(&self, e: &std::io::Error) {
        warning!("mcp pipe: a client connection failed: {e}");
    }
}

pub struct DeskHandler(pub Arc<Shared>);

fn notification_warning(names: &Names) -> Option<String> {
    #[cfg(windows)]
    {
        use crate::winutil::{HandlerKey, handler_key};
        match handler_key(names) {
            HandlerKey::Ours => None,
            HandlerKey::Missing | HandlerKey::Changed(_) => Some(format!(
                "Notification actions need repair. Run agentbolt install --channel {}. You can still answer here.",
                names.channel
            )),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = names;
        None
    }
}

impl Handler for DeskHandler {
    async fn handle(&self, _client: &ClientInfo, op: Value) -> Result<Value, String> {
        let op: DeskOp =
            serde_json::from_value(op).map_err(|e| format!("unknown or malformed op: {e}"))?;
        let new_settings = match &op {
            DeskOp::SetSettings { settings } => Some(settings.clone()),
            _ => None,
        };
        let health = matches!(op, DeskOp::List | DeskOp::Status);
        let mut value = self.0.desk(op).await?;
        if health {
            let names = self.0.names.clone();
            let warning = tokio::task::spawn_blocking(move || notification_warning(&names))
                .await
                .map_err(|e| format!("notification health check: {e}"))?;
            if let Some(map) = value.as_object_mut() {
                map.insert("notification_warning".into(), warning.into());
            }
        }
        if let Some(settings) = new_settings {
            self.0.settings_changed(settings);
        }
        Ok(value)
    }

    fn connected(&self, client: &ClientInfo) {
        info!(
            "desk connected (pid {:?}, connection {})",
            client.pid, client.connection
        );
    }

    fn disconnected(&self, client: &ClientInfo) {
        self.0.svc.disconnected(client.connection);
    }

    fn refused(&self, pid: Option<u32>, image: Option<&std::path::Path>) {
        warning!(
            "desk pipe: dropped a client that isn't the installed agentbolt-desk (pid {pid:?}, image {})",
            image
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "unknown".into())
        );
    }

    fn listen_error(&self, e: &std::io::Error) {
        warning!("desk pipe: a client connection failed: {e}");
    }
}

/// Call `Service::tick` about once a second.
pub async fn tick_loop(shared: Arc<Shared>) {
    let mut every = tokio::time::interval(Duration::from_secs(1));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        every.tick().await;
        let svc = shared.svc.clone();
        match tokio::task::spawn_blocking(move || svc.tick(time::OffsetDateTime::now_utc())).await {
            Ok(effects) => shared.emit(effects),
            Err(e) => error!("tick panicked: {e}"),
        }
    }
}

/// The buttons a toast may carry: none unless the channel's `agentbolt://` handler key points to the Desk next to
/// the daemon (a missing or changed key would send a button's token to whatever program it names).
pub fn toast_buttons(buttons: Vec<(String, String)>, handler_ok: bool) -> Vec<(String, String)> {
    if handler_ok { buttons } else { Vec::new() }
}

/// Act on effects: the tray count, the ntfy push (at most one a minute), toasts, Codex delivery.
/// `toasts` = false in headless mode: toast effects are logged and dropped.
pub async fn effects_loop(
    shared: Arc<Shared>,
    mut rx: mpsc::UnboundedReceiver<Event>,
    ui: Arc<dyn Ui>,
    mut paused: bool,
    toasts: bool,
) {
    let mut push = ntfy::PushState::default();
    let delivery = crate::delivery::Pool::new(shared.clone())
        .map_err(|e| {
            error!("couldn't start Codex delivery workers: {e}");
        })
        .ok();
    let mut every = tokio::time::interval(Duration::from_secs(1));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else { return };
                match event {
                    Event::Effect(Effect::InboxChanged { open, notifiable, next_deadline }) => {
                        ui.counts(open);
                        push.on_counts(ntfy::Counts { open: notifiable, next_deadline });
                    }
                    Event::Effect(Effect::Toast { ask_id, tag, suppress_popup, title, body, buttons }) => {
                        if !toasts {
                            info!("toast for ask {ask_id} skipped: headless");
                        } else if paused {
                            info!("toast for ask {ask_id} skipped: notifications are paused");
                        } else {
                            let shared = shared.clone();
                            // Preserve replacement order: an older count must not overwrite a newer one.
                            let _ = tokio::task::spawn_blocking(move || {
                                if shared.svc.toast_allowed(&ask_id) {
                                    show_toast(&shared.names, &ask_id, &tag, suppress_popup, &title, &body, &buttons);
                                }
                            }).await;
                        }
                    }
                    Event::Effect(Effect::CodexQueue { ask_id, thread, text }) => {
                        let job = crate::delivery::Job { ask_id, thread, text };
                        let submitted = match &delivery { Some(pool) => pool.submit(job), None => Err(job) };
                        if let Err(job) = submitted {
                            crate::delivery::failed(&shared, &job, "Codex delivery queue unavailable or full; retrieve this durable answer with desk_answers");
                        }
                    }
                    Event::Settings(settings) => {
                        paused = settings.toasts_off;
                        ui.paused(paused);
                    }
                }
            }
            _ = every.tick() => {
                if push.due(Instant::now()).is_some() {
                    let shared = shared.clone();
                    tokio::task::spawn_blocking(move || send_push(&shared));
                }
            }
        }
    }
}

fn send_push(shared: &Shared) {
    let url = match shared.get_settings() {
        Ok(settings) => settings.ntfy_topic_url,
        Err(e) => {
            warning!("ntfy: couldn't read settings: {e}");
            return;
        }
    };
    let Some(url) = url.filter(|u| !u.trim().is_empty()) else {
        return;
    };
    let counts = match shared.svc.notification_counts() {
        Ok((open, next_deadline)) if open > 0 => ntfy::Counts {
            open,
            next_deadline,
        },
        Ok(_) => return,
        Err(e) => {
            warning!("ntfy: couldn't read counts: {e}");
            return;
        }
    };
    let body = ntfy::body(&counts, time::OffsetDateTime::now_utc());
    match ntfy::post(&url, &body) {
        Ok(()) => info!("ntfy: pushed counts ({} unmuted open)", counts.open),
        Err(e) => warning!("{e}"),
    }
}

fn show_toast(
    names: &Names,
    ask_id: &str,
    tag: &str,
    suppress_popup: bool,
    title: &str,
    body: &str,
    buttons: &[(String, String)],
) {
    #[cfg(windows)]
    {
        // Check at emission, not at daemon startup: never put fresh tokens into a changed handler.
        let handler_ok = crate::winutil::handler_key(names) == crate::winutil::HandlerKey::Ours;
        let buttons = toast_buttons(buttons.to_vec(), handler_ok);
        match crate::toast::show(names, tag, suppress_popup, title, body, &buttons) {
            Ok(()) => info!("toast shown for ask {ask_id} ({} buttons)", buttons.len()),
            Err(e) => warning!("ask {ask_id}: {e}"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (names, tag, suppress_popup, title, body, buttons);
        info!("toast for ask {ask_id} skipped: toasts are Windows-only");
    }
}

pub fn deliver_to_codex(shared: &Shared, ask_id: &str, thread: &str, text: &str) {
    if !shared.svc.codex_delivery_allowed(ask_id, thread) {
        let _ = shared.svc.record_delivery(
            ask_id,
            thread,
            false,
            Some("Codex wake permission is off or the original host thread is unverified"),
        );
        return;
    }
    deliver_with(shared, codex::find(), ask_id, thread, text);
}

fn deliver_with(
    shared: &Shared,
    exe: Option<codex::CodexExe>,
    ask_id: &str,
    thread: &str,
    text: &str,
) {
    let (ok, detail) = match exe {
        Some(exe) => codex::run(&exe, thread, text, codex::WAIT),
        None => (false, "codex isn't on PATH".to_string()),
    };
    if ok {
        info!("ask {ask_id}: queued into codex thread {thread}");
    } else {
        warning!("ask {ask_id}: codex delivery failed: {detail}");
    }
    if let Err(e) = shared
        .svc
        .record_delivery(ask_id, thread, ok, Some(&detail))
    {
        error!("ask {ask_id}: couldn't record the codex delivery: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeService, channel};
    use agentbolt_ipc::{Client, PeerCheck, Pipe, Server};
    use serde_json::json;

    fn me() -> Vec<std::path::PathBuf> {
        vec![std::env::current_exe().unwrap()]
    }

    async fn start(fake: Arc<FakeService>) -> (Names, Arc<Shared>, mpsc::UnboundedReceiver<Event>) {
        let names = channel();
        let (shared, rx) = Shared::new(names.clone(), fake);
        let mcp = Server::bind_with(&names, Pipe::Mcp, PeerCheck::AnyLocalUser).unwrap();
        let desk = Server::bind_with(&names, Pipe::Desk, PeerCheck::Image(me())).unwrap();
        tokio::spawn(mcp.serve(Arc::new(McpHandler(shared.clone()))));
        tokio::spawn(desk.serve(Arc::new(DeskHandler(shared.clone()))));
        (names, shared, rx)
    }

    fn answers(handle: &str, wait_ms: u64) -> McpOp {
        McpOp::Answers {
            handle: handle.into(),
            ids: vec![],
            wait_ms,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answers_wait_returns_early_when_an_answer_appears() {
        let fake = Arc::new(FakeService::default());
        fake.set_waiting(&["a1"]);
        let (names, _shared, _rx) = start(fake.clone()).await;
        let mut client = Client::connect_to(&names, Pipe::Mcp, &me()).await.unwrap();

        let answerer = {
            let fake = fake.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(700)).await;
                fake.answer("a1");
            })
        };
        let started = Instant::now();
        let result = client.call(&answers("h1", 20_000)).await.unwrap();
        let took = started.elapsed();
        answerer.await.unwrap();
        assert_eq!(result["answers"][0]["id"], "a1");
        assert!(
            took >= Duration::from_millis(650),
            "it waited for the answer: {took:?}"
        );
        assert!(
            took < Duration::from_secs(3),
            "it returned as soon as the answer appeared: {took:?}"
        );
        let polls = fake.answers_calls();
        assert!(
            (3..=8).contains(&polls),
            "polled about every 250 ms: {polls}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answers_wait_runs_out_without_an_answer() {
        let fake = Arc::new(FakeService::default());
        fake.set_waiting(&["a1"]);
        let (names, _shared, _rx) = start(fake.clone()).await;
        let mut client = Client::connect_to(&names, Pipe::Mcp, &me()).await.unwrap();
        let started = Instant::now();
        let result = client.call(&answers("h1", 600)).await.unwrap();
        let took = started.elapsed();
        assert_eq!(result["answers"], json!([]), "silence is never an answer");
        assert!(
            took >= Duration::from_millis(600) && took < Duration::from_millis(1500),
            "{took:?}"
        );

        // No wait, or nothing left waiting: one read, straight back.
        let started = Instant::now();
        client.call(&answers("h1", 0)).await.unwrap();
        fake.set_waiting(&[]);
        client.call(&answers("h1", 60_000)).await.unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn toasts_lose_their_buttons_when_the_handler_key_is_not_ours() {
        let buttons = vec![
            ("Ship".to_string(), "aa.bb".to_string()),
            ("Wait".to_string(), "cc.dd".to_string()),
        ];
        assert_eq!(toast_buttons(buttons.clone(), true), buttons);
        assert!(toast_buttons(buttons, false).is_empty());
    }

    #[test]
    fn answers_ready_reads_both_shapes() {
        assert!(answers_ready(
            &json!({ "answers": [{ "id": "a" }], "waiting": ["b"] })
        ));
        assert!(!answers_ready(&json!({ "answers": [], "waiting": ["b"] })));
        assert!(
            answers_ready(&json!({ "answers": [], "waiting": [] })),
            "nothing left to wait for"
        );
        assert!(!answers_ready(&json!({ "answers": [] })));
        assert!(answers_ready(&json!([{ "id": "a" }])));
        assert!(!answers_ready(&json!([])));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn effects_reach_the_tray_and_desk_settings_pause_toasts() {
        #[derive(Default)]
        struct Seen(std::sync::Mutex<Vec<String>>);
        impl Ui for Seen {
            fn counts(&self, open: u32) {
                self.0.lock().unwrap().push(format!("counts {open}"));
            }
            fn paused(&self, paused: bool) {
                self.0.lock().unwrap().push(format!("paused {paused}"));
            }
        }
        let fake = Arc::new(FakeService::default());
        fake.set_waiting(&["a1"]);
        let (names, shared, rx) = start(fake.clone()).await;
        let seen = Arc::new(Seen::default());
        tokio::spawn(effects_loop(shared.clone(), rx, seen.clone(), false, false));

        // An ask (the fake emits InboxChanged), then the Desk turns toasts off.
        let mut mcp = Client::connect_to(&names, Pipe::Mcp, &me()).await.unwrap();
        mcp.call(&McpOp::Status).await.unwrap();
        let mut desk = Client::connect_to(&names, Pipe::Desk, &me()).await.unwrap();
        let settings = Settings {
            toasts_off: true,

            ..Settings::default()
        };
        desk.call(&DeskOp::SetSettings { settings }).await.unwrap();
        crate::testutil::wait_for("tray updates", || seen.0.lock().unwrap().len() >= 2).await;
        assert_eq!(
            *seen.0.lock().unwrap(),
            vec!["counts 1".to_string(), "paused true".to_string()]
        );

        // The tray's Pause item flips it back through the service.
        assert!(!shared.toggle_pause().unwrap());
        assert!(!fake.settings().toasts_off);
        drop(desk);
        drop(mcp);
        crate::testutil::wait_for("disconnects reach the service", || fake.disconnects() == 2)
            .await;
    }

    #[test]
    fn a_codex_delivery_is_recorded_whatever_happens() {
        let fake = Arc::new(FakeService::default());
        let (shared, _rx) = Shared::new(channel(), fake.clone());
        // Never the real codex in a test: no exe, then one that can't start.
        deliver_with(&shared, None, "a1", "thread-1", "Approve");
        let missing = codex::CodexExe {
            path: "Z:/nowhere/codex.exe".into(),
            env: vec![],
        };
        deliver_with(&shared, Some(missing), "a2", "thread-2", "Decline");
        let d = fake.deliveries();
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[0].0.as_str(), d[0].1.as_str(), d[0].2),
            ("a1", "thread-1", false)
        );
        assert_eq!(d[0].3.as_deref(), Some("codex isn't on PATH"));
        assert_eq!(
            (d[1].0.as_str(), d[1].1.as_str(), d[1].2),
            ("a2", "thread-2", false)
        );
        assert!(d[1].3.as_deref().unwrap().starts_with("could not start"));
    }
}
