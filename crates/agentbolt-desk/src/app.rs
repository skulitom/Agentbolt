//! The Desk window (eframe/egui): Inbox, Agents and Settings. It follows the system's light or dark
//! theme, polls the daemon every 1.5 s while open, and hides on close when running warm. It lists the Inbox (which marks
//! asks shown) only while the window is focused and not minimized; otherwise it refreshes the counts only.
//!
//! Ask text is untrusted: bodies are plain selectable text, never HTML or markdown, and links pass
//! `view::classify` before anything opens.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_core::model::{Agent, AnswerVia, Ask, AskKind, AskStatus, Owner, Priority, Settings};
use agentbolt_core::protocol::{DeskAnswer, MAX_BATCH_ANSWERS};
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;

use crate::drafts::{Draft, Snapshot as DraftSnapshot, Store as DraftStore};
use crate::os;
use crate::theme;
use crate::view::{self, FieldInput, Link};
use crate::worker::{self, Req, Resp, Worker};

const POLL: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Inbox,
    History,
    Board,
    Activity,
    Holds,
    Agents,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BoardSection {
    Handoffs,
    Messages,
    Agents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SettingsSection {
    Notifications,
    Phone,
    Owners,
}

enum Conn {
    Unknown,
    Up,
    Down(String),
}

struct Notice {
    text: String,
    error: bool,
    at: Instant,
}

#[derive(Default)]
struct SettingsDraft {
    ntfy: String,
    toasts_off: bool,
}

#[derive(Default)]
struct DirectoryState {
    query: agentbolt_core::protocol::DirectoryQuery,
    search: String,
    back: Vec<Option<String>>,
    next: Option<String>,
    more: bool,
    total: u32,
}

impl DirectoryState {
    fn controls(&mut self, ui: &mut egui::Ui, label: &str, busy: bool) -> bool {
        let mut changed = false;
        ui.push_id(label, |ui| {
            ui.horizontal_wrapped(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(format!("Search {label}"))
                        .desired_width(ui.available_width().min(320.0)),
                );
                if ui.add_enabled(!busy, egui::Button::new("Search")).clicked()
                    || (!busy
                        && field.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    self.query.search = self.search.trim().into();
                    self.query.after = None;
                    self.back.clear();
                    changed = true;
                }
                if !self.search.is_empty()
                    && ui.add_enabled(!busy, egui::Button::new("Clear")).clicked()
                {
                    self.search.clear();
                    self.query.search.clear();
                    self.query.after = None;
                    self.back.clear();
                    changed = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                if self.more || !self.back.is_empty() {
                    if ui
                        .add_enabled(
                            !busy && !self.back.is_empty(),
                            egui::Button::new("Previous"),
                        )
                        .clicked()
                    {
                        self.query.after = self.back.pop().flatten();
                        changed = true;
                    }
                    if ui
                        .add_enabled(!busy && self.more, egui::Button::new("Next"))
                        .clicked()
                    {
                        self.back.push(self.query.after.clone());
                        self.query.after = self.next.clone();
                        changed = true;
                    }
                }
                let noun = if self.total == 1 {
                    label.strip_suffix('s').unwrap_or(label)
                } else {
                    label
                };
                ui.weak(if self.more || !self.back.is_empty() {
                    format!("{} {noun} · Page {}", self.total, self.back.len() + 1)
                } else {
                    format!("{} {noun}", self.total)
                });
                if busy {
                    ui.spinner();
                }
            });
        });
        ui.add_space(8.0);
        changed
    }
}

struct DeliveryView {
    query: agentbolt_core::protocol::DeliveryQuery,
    back: Vec<Option<u64>>,
    page: agentbolt_core::protocol::DeliveryPage,
    busy: bool,
}

pub struct DeskApp {
    upgrade: crate::upgrade::Gate,
    names: Names,
    worker: Worker,
    tab: Tab,
    settings_section: SettingsSection,
    inbox: Vec<Ask>,
    inbox_after: Option<agentbolt_core::protocol::InboxCursor>,
    inbox_back: Vec<Option<agentbolt_core::protocol::InboxCursor>>,
    inbox_next: Option<agentbolt_core::protocol::InboxCursor>,
    inbox_more: bool,
    /// Open asks by the last count (a List, or a Status while the window wasn't seen).
    open_count: Option<u32>,
    recent: Vec<Ask>,
    history: agentbolt_core::protocol::HistoryResult,
    history_query: agentbolt_core::protocol::HistoryQuery,
    history_search: String,
    history_back: Vec<Option<String>>,
    history_selected: Option<String>,
    history_inflight: bool,
    selected_inbox: Option<String>,
    /// Past questions are opened only by an explicit click, never by Inbox selection recovery.
    selected_recent: Option<String>,
    /// None forces the disclosure closed on first display, ignoring persisted egui expansion.
    recent_open: Option<bool>,
    batch_selection: HashSet<String>,
    batch_mode: bool,
    review_batch: bool,
    drafts: HashMap<String, Draft>,
    draft_store: DraftStore,
    draft_error: Option<String>,
    draft_retry: Option<Instant>,
    draft_check_after: Option<String>,
    draft_check_inflight: bool,
    last_draft_check: Option<Instant>,
    delivery_view: Option<DeliveryView>,
    sending: HashSet<String>,
    agents: Vec<Agent>,
    agents_directory: DirectoryState,
    board: agentbolt_core::board::Snapshot,
    board_query: agentbolt_core::board::DeskQuery,
    board_search: String,
    board_section: BoardSection,
    board_request_back: Vec<Option<agentbolt_core::board::RequestCursor>>,
    board_message_back: Vec<Option<u64>>,
    board_agent_back: Vec<u64>,
    board_inflight: bool,
    last_board: Option<Instant>,
    lease_drafts: HashMap<String, u32>,
    metrics: Option<agentbolt_core::metrics::Report>,
    metrics_inflight: bool,
    last_metrics: Option<Instant>,
    baseline_label: String,
    verdict_draft: String,
    holds: agentbolt_core::holds::Snapshot,
    holds_after: Option<String>,
    configure_holds: bool,
    holds_back: Vec<Option<String>>,
    holds_inflight: bool,
    last_holds: Option<Instant>,
    hold_draft: Option<agentbolt_core::holds::Config>,
    hold_excludes: String,
    focus_file: String,
    new_hold_repo: String,
    new_hold_path: String,
    owners: Vec<Owner>,
    owners_directory: DirectoryState,
    settings: Option<Settings>,
    settings_draft: SettingsDraft,
    phone: Option<agentbolt_core::phone::Status>,
    phone_draft: Option<agentbolt_core::phone::Settings>,
    phone_daemon_token: String,
    phone_reply_token: String,
    notice: Option<Notice>,
    conn: Conn,
    notification_warning: Option<String>,
    list_inflight: bool,
    last_list: Option<Instant>,
    agents_inflight: bool,
    last_agents: Option<Instant>,
    owners_inflight: bool,
    last_owners: Option<Instant>,
    /// Whether the window was seen (focused, not minimized) on the last frame: seeing it again lists at once.
    was_seen: bool,
    /// Whether the selected ask's local evidence paths exist, checked once per selection.
    paths: view::PathCache,
}

/// Open the window and run until it closes.
pub fn run(names: Names, warm: bool) -> anyhow::Result<()> {
    os::set_app_identity(&names).map_err(anyhow::Error::msg)?;
    let title = if names.is_main() {
        "Agentbolt".to_string()
    } else {
        format!("Agentbolt ({})", names.channel)
    };
    let Some(instance) =
        crate::os::first_window(&names, &title, !warm).map_err(anyhow::Error::msg)?
    else {
        return Ok(());
    };
    crate::os::trace_window_shown(&title);
    let instance = std::sync::Arc::new(std::sync::Mutex::new(Some(instance)));
    let _instance_lifetime = instance.clone();
    let restore_visible =
        agentbolt_ipc::desk_control::take_reopen(&names, crate::drafts::session_id());
    let data = names.data_dir().map_err(anyhow::Error::msg)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_icon(egui::IconData {
                rgba: agentbolt_core::icon::rgba_at(64),
                width: 64,
                height: 64,
            })
            .with_active(!warm || restore_visible)
            .with_inner_size([1040.0, 620.0])
            .with_min_inner_size([640.0, 420.0]),
        centered: true,
        persistence_path: Some(
            data.join(format!("desk-window-{}.ron", crate::drafts::session_id())),
        ),
        ..Default::default()
    };
    crate::startup::mark("native_start");
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            crate::resident::prepare(cc, warm, restore_visible)?;
            crate::startup::mark("renderer_ready");
            let ctx = cc.egui_ctx.clone();
            let worker = worker::spawn(names.clone(), move || ctx.request_repaint());
            let app = DeskApp::new(names.clone(), worker);
            crate::upgrade::serve(
                names,
                app.upgrade.clone(),
                app.worker.tx.clone(),
                cc.egui_ctx.clone(),
                warm,
                instance,
            );
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("the Desk window failed: {e}"))
}

impl DeskApp {
    fn new(names: Names, worker: Worker) -> Self {
        let (draft_store, state) = DraftStore::load(
            &names
                .data_dir()
                .expect("data path checked before opening the window"),
        );
        let draft_error = draft_store.load_error().map(str::to_owned);
        Self {
            upgrade: crate::upgrade::gate(draft_error.clone()),
            names,
            worker,
            tab: Tab::Inbox,
            settings_section: SettingsSection::Notifications,
            inbox: Vec::new(),
            inbox_after: None,
            inbox_back: Vec::new(),
            inbox_next: None,
            inbox_more: false,
            open_count: None,
            recent: Vec::new(),
            history: Default::default(),
            history_query: Default::default(),
            history_search: String::new(),
            history_back: Vec::new(),
            history_selected: None,
            history_inflight: false,
            selected_inbox: state.selected,
            selected_recent: None,
            recent_open: None,
            batch_mode: !state.batch.is_empty(),
            batch_selection: state.batch,
            review_batch: false,
            drafts: state.drafts,
            draft_store,
            draft_error,
            draft_retry: None,
            draft_check_after: None,
            draft_check_inflight: false,
            last_draft_check: None,
            delivery_view: None,
            sending: HashSet::new(),
            agents: Vec::new(),
            agents_directory: Default::default(),
            board: Default::default(),
            board_query: Default::default(),
            board_search: String::new(),
            board_section: BoardSection::Handoffs,
            board_request_back: Vec::new(),
            board_message_back: Vec::new(),
            board_agent_back: Vec::new(),
            board_inflight: false,
            last_board: None,
            lease_drafts: HashMap::new(),
            metrics: None,
            metrics_inflight: false,
            last_metrics: None,
            baseline_label: "Before adoption".into(),
            verdict_draft: String::new(),
            holds: Default::default(),
            holds_inflight: false,
            holds_after: None,
            configure_holds: false,
            holds_back: Vec::new(),
            last_holds: None,
            hold_draft: None,
            hold_excludes: String::new(),
            focus_file: String::new(),
            new_hold_repo: String::new(),
            new_hold_path: String::new(),
            owners: Vec::new(),
            owners_directory: Default::default(),
            settings: None,
            settings_draft: SettingsDraft::default(),
            phone: None,
            phone_draft: None,
            phone_daemon_token: String::new(),
            phone_reply_token: String::new(),
            notice: None,
            conn: Conn::Unknown,
            notification_warning: None,
            list_inflight: false,
            last_list: None,
            agents_inflight: false,
            last_agents: None,
            owners_inflight: false,
            last_owners: None,
            was_seen: false,
            paths: view::PathCache::default(),
        }
    }

    fn send(&self, req: Req) {
        let _ = self.worker.tx.send(req);
    }

    fn note(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Some(Notice {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    fn drain(&mut self) {
        while let Ok(resp) = self.worker.rx.try_recv() {
            match resp {
                Resp::History { query, result } => {
                    if query != self.history_query {
                        continue;
                    }
                    self.history_inflight = false;
                    match result {
                        Ok(page) => {
                            if self
                                .history_selected
                                .as_ref()
                                .is_none_or(|id| !page.asks.iter().any(|a| &a.id == id))
                            {
                                self.history_selected = page.asks.first().map(|a| a.id.clone());
                            }
                            self.history = page;
                        }
                        Err(error) => self.note(format!("Couldn't load history: {error}"), true),
                    }
                }
                Resp::List { after, result } => {
                    self.list_inflight = false;
                    if after != self.inbox_after {
                        continue;
                    }
                    let mut r = match result {
                        Ok(r) => r,
                        Err(error) => {
                            self.conn = Conn::Down(error);
                            continue;
                        }
                    };
                    self.inbox_next = r.next;
                    self.inbox_more = r.more;
                    self.notification_warning = r.notification_warning.take();
                    crate::startup::mark("inbox_received");
                    crate::startup::record("inbox_refreshed");
                    self.list_inflight = false;
                    self.conn = Conn::Up;
                    view::sort_asks(&mut r.inbox);
                    self.inbox = r.inbox;
                    self.open_count = Some(r.total);
                    self.recent = r.recent;
                    let ids: HashSet<&str> = self.inbox.iter().map(|a| a.id.as_str()).collect();
                    self.batch_selection.retain(|id| ids.contains(id.as_str()));
                    // Another page can hold an unsent draft. Retire only questions known to be closed.
                    let closed: HashSet<&str> = self.recent.iter().map(|a| a.id.as_str()).collect();
                    self.drafts.retain(|id, _| !closed.contains(id.as_str()));
                    if self
                        .selected_inbox
                        .as_deref()
                        .is_none_or(|s| !ids.contains(s))
                    {
                        self.selected_inbox = self.inbox.first().map(|a| a.id.clone());
                    }
                    if self
                        .selected_recent
                        .as_deref()
                        .is_some_and(|id| !closed.contains(id))
                    {
                        self.selected_recent = None;
                    }
                    if self.inbox.is_empty() && self.batch_selection.is_empty() {
                        self.batch_mode = false;
                        self.review_batch = false;
                    }
                }
                Resp::DraftStates { ids, result } => {
                    self.draft_check_inflight = false;
                    if let Ok(status) = result {
                        self.draft_check_after = ids.last().cloned();
                        for id in status.closed {
                            self.drafts.remove(&id);
                            self.batch_selection.remove(&id);
                        }
                    }
                }
                Resp::DeliveryHistory { query, result } => {
                    if let Some(view) = self.delivery_view.as_mut().filter(|v| v.query == query) {
                        view.busy = false;
                        match result {
                            Ok(page) => view.page = page,
                            Err(error) => {
                                self.note(format!("Couldn't load delivery history: {error}"), true)
                            }
                        }
                    }
                }
                Resp::Status(s) => {
                    self.notification_warning = s.notification_warning;
                    self.list_inflight = false;
                    self.conn = Conn::Up;
                    self.open_count = Some(s.open);
                }
                Resp::BatchAnswered { ids, result } => {
                    for id in &ids {
                        self.sending.remove(id);
                    }
                    match result {
                        Ok(r) => {
                            for answer in &r.answers {
                                self.drafts.remove(&answer.ask_id);
                                self.batch_selection.remove(&answer.ask_id);
                                self.inbox.retain(|a| a.id != answer.ask_id);
                            }
                            self.review_batch = false;
                            self.batch_mode = false;
                            self.note(format!("Sent {} answers", r.answers.len()), false);
                        }
                        Err(e) => self.note(format!("Batch wasn't confirmed: {e}. Check recently answered before trying again."), true),
                    }
                    self.last_list = None;
                }
                Resp::Answered { ask_id, result } => {
                    self.sending.remove(&ask_id);
                    match result {
                        Ok(r) => {
                            let title = self.title_of(&ask_id);
                            let said = r
                                .answer
                                .choice
                                .clone()
                                .or_else(|| r.answer.values.as_ref().map(Value::to_string));
                            let message = if r.answer.via == AnswerVia::NotNeeded {
                                format!("Marked \"{title}\" Not needed")
                            } else {
                                format!("Answered \"{title}\": {}", said.unwrap_or_default())
                            };
                            self.note(message, false);
                            self.inbox.retain(|a| a.id != ask_id);
                            self.drafts.remove(&ask_id);
                            self.last_list = None;
                        }
                        Err(e) => self.note(format!("That answer didn't go through: {e}"), true),
                    }
                }
                Resp::Agents { query, result } => {
                    self.agents_inflight = false;
                    if query != self.agents_directory.query {
                        continue;
                    }
                    match result {
                        Ok(page) => {
                            self.agents = page.items;
                            self.agents_directory.next = page.next;
                            self.agents_directory.more = page.more;
                            self.agents_directory.total = page.total;
                        }
                        Err(error) => self.note(format!("Couldn't load agents: {error}"), true),
                    }
                }
                Resp::Board { query, result } => {
                    self.board_inflight = false;
                    if query != self.board_query {
                        continue;
                    }
                    match result {
                        Ok(page) => self.board = page,
                        Err(error) => self.note(format!("Couldn't load the Board: {error}"), true),
                    }
                }
                Resp::Metrics(r) => {
                    self.metrics_inflight = false;
                    self.metrics = Some(*r);
                }
                Resp::Holds { after, result } => {
                    self.holds_inflight = false;
                    if after != self.holds_after {
                        continue;
                    }
                    let r = match result {
                        Ok(r) => r,
                        Err(error) => {
                            self.note(format!("Couldn't load work holds: {error}"), true);
                            continue;
                        }
                    };
                    if self.hold_draft.is_none() {
                        self.hold_excludes = r.config.excluded_paths.join("\n");
                        self.hold_draft = Some(r.config.clone());
                    }
                    self.holds = r;
                }
                Resp::HoldsSaved(r) => {
                    match r {
                        Ok(()) => {
                            self.note("Working-copy allowlist saved", false);
                            self.hold_draft = None;
                            self.holds_after = None;
                            self.holds_back.clear();
                        }
                        Err(e) => self.note(e, true),
                    }
                    self.last_holds = None;
                }
                Resp::FocusImported(r) => match r {
                    Ok(config) => {
                        self.hold_draft = Some(config);
                        self.note("Eligible working copies imported. Review the list and exclusions, then Save to enable scanning.",false);
                    }
                    Err(e) => self.note(e, true),
                },
                Resp::MetricsSaved(r) => {
                    match r {
                        Ok(()) => self.note("Activity record saved", false),
                        Err(e) => self.note(e, true),
                    }
                    self.last_metrics = None;
                }
                Resp::LeaseSaved(r) => {
                    match r {
                        Ok(()) => self.note("Inactivity ceiling saved", false),
                        Err(e) => self.note(e, true),
                    }
                    self.last_board = None;
                }
                Resp::Owners { query, result } => {
                    self.owners_inflight = false;
                    if query != self.owners_directory.query {
                        continue;
                    }
                    match result {
                        Ok(page) => {
                            self.owners = page.items;
                            self.owners_directory.next = page.next;
                            self.owners_directory.more = page.more;
                            self.owners_directory.total = page.total;
                        }
                        Err(error) => self.note(format!("Couldn't load owners: {error}"), true),
                    }
                }
                Resp::OwnerSaved(r) => {
                    match r {
                        Ok(name) => self.note(format!("Saved owner {name}"), false),
                        Err(e) => self.note(format!("Couldn't save the owner: {e}"), true),
                    }
                    self.last_owners = None;
                    self.last_agents = None;
                    self.last_list = None;
                }
                Resp::Settings(s) => {
                    self.settings_draft = SettingsDraft {
                        ntfy: s.ntfy_topic_url.clone().unwrap_or_default(),
                        toasts_off: s.toasts_off,
                    };
                    self.settings = Some(s);
                }
                Resp::Phone(status) => {
                    if self.phone_draft.is_none() {
                        self.phone_draft = Some(status.settings.clone());
                    }
                    self.phone = Some(status);
                }
                Resp::PhoneSaved(result) => match result {
                    Ok(()) => {
                        self.phone_daemon_token.clear();
                        self.phone_reply_token.clear();
                        self.phone_draft = None;
                        self.note("Phone settings saved", false);
                        self.send(Req::Phone);
                    }
                    Err(error) => self.note(format!("Couldn't save phone settings: {error}"), true),
                },
                Resp::SettingsSaved(r) => match r {
                    Ok(()) => {
                        self.note("Settings saved", false);
                        self.send(Req::GetSettings);
                    }
                    Err(e) => self.note(format!("Couldn't save the settings: {e}"), true),
                },
                Resp::Started(r) => match r {
                    Ok(()) => self.note("Starting Agentbolt...", false),
                    Err(e) => self.note(e, true),
                },
                Resp::Failed {
                    what,
                    error,
                    not_running,
                } => {
                    match what {
                        "the Inbox" => self.list_inflight = false,
                        "the agents" => self.agents_inflight = false,
                        "the Board" => self.board_inflight = false,
                        "activity" => self.metrics_inflight = false,
                        "work holds" => self.holds_inflight = false,
                        "the owners" => self.owners_inflight = false,
                        _ => {}
                    }
                    if not_running || what == "the Inbox" {
                        self.conn = Conn::Down(error);
                    } else {
                        self.note(format!("Couldn't load {what}: {error}"), true);
                    }
                }
            }
        }
    }

    fn title_of(&self, id: &str) -> String {
        self.inbox
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.input.title.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(i.focused));
        // Listing the Inbox marks its asks shown (their default clocks start), so list only while he can see the
        // window; otherwise refresh the counts. Coming back to the window lists at once.
        let visibility = view::poll_kind(focused && crate::resident::visible(), minimized);
        if !crate::resident::visible() {
            self.recent_open = None;
            self.selected_recent = None;
        }
        let seen = visibility == view::Poll::List;
        // Another tab can be visible without showing a new question in the Inbox.
        let kind = if seen && self.tab == Tab::Inbox {
            view::Poll::List
        } else {
            view::Poll::Status
        };
        if seen && !self.was_seen {
            self.last_list = None;
        }
        self.was_seen = seen;
        if !self.list_inflight && self.last_list.is_none_or(|t| t.elapsed() >= POLL) {
            self.list_inflight = true;
            self.last_list = Some(Instant::now());
            self.send(match kind {
                view::Poll::List => Req::List(self.inbox_after.clone()),
                view::Poll::Status => Req::Status,
            });
        }
        if self.tab == Tab::Agents
            && seen
            && !self.agents_inflight
            && self.last_agents.is_none_or(|t| t.elapsed() >= POLL)
        {
            self.agents_inflight = true;
            self.last_agents = Some(Instant::now());
            self.send(Req::Agents(self.agents_directory.query.clone()));
        }
        if self.tab == Tab::Board
            && seen
            && !self.board_inflight
            && self.last_board.is_none_or(|t| t.elapsed() >= POLL)
        {
            self.board_inflight = true;
            self.last_board = Some(Instant::now());
            self.send(Req::Board(self.board_query.clone()));
        }
        if seen
            && self.tab == Tab::Settings
            && !self.owners_inflight
            && self
                .last_owners
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
        {
            self.owners_inflight = true;
            self.last_owners = Some(Instant::now());
            self.send(Req::Owners(self.owners_directory.query.clone()));
        }
        if self.tab == Tab::Activity
            && seen
            && !self.metrics_inflight
            && self
                .last_metrics
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(30))
        {
            self.metrics_inflight = true;
            self.last_metrics = Some(Instant::now());
            self.send(Req::Metrics);
        }
        if self.tab == Tab::Holds
            && seen
            && !self.holds_inflight
            && self
                .last_holds
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
        {
            self.holds_inflight = true;
            self.last_holds = Some(Instant::now());
            self.send(Req::Holds(self.holds_after.clone()));
        }
        if seen
            && !self.drafts.is_empty()
            && !self.draft_check_inflight
            && self
                .last_draft_check
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
        {
            let mut ids: Vec<_> = self.drafts.keys().cloned().collect();
            ids.sort();
            let start = self
                .draft_check_after
                .as_ref()
                .map_or(0, |after| ids.partition_point(|id| id <= after));
            let start = if start == ids.len() { 0 } else { start };
            let ids = ids.into_iter().skip(start).take(100).collect();
            self.draft_check_inflight = true;
            self.last_draft_check = Some(Instant::now());
            self.send(Req::DraftStates(ids));
        }
        ctx.request_repaint_after(POLL);
    }

    /// Save only on a change. The atomic file is synced before remembering that state as saved.
    fn save_drafts(&mut self) -> Result<(), String> {
        self.draft_store.save(DraftSnapshot {
            drafts: self.drafts.clone(),
            selected: self.selected_inbox.clone(),
            batch: self.batch_selection.clone(),
        })
    }

    fn switch_to(&mut self, tab: Tab) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        match tab {
            Tab::Inbox => {
                self.last_list = None;
                self.recent_open = None;
                self.selected_recent = None;
            }
            Tab::History => self.load_history(),
            Tab::Settings => {
                self.send(Req::GetSettings);
                self.send(Req::Phone);
                self.last_owners = None;
            }
            Tab::Agents => self.last_agents = None,
            Tab::Board => self.last_board = None,
            Tab::Activity => self.last_metrics = None,
            Tab::Holds => self.last_holds = None,
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::brand(ui);
            ui.label(RichText::new("Agentbolt").size(19.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match &self.conn {
                    Conn::Up => {
                        ui.weak("Connected");
                    }
                    Conn::Unknown => {
                        ui.weak("Connecting…");
                        ui.spinner();
                    }
                    Conn::Down(_) => {
                        if ui.button("Start Agentbolt").clicked() {
                            self.send(Req::StartDaemon);
                        }
                        ui.colored_label(ui.visuals().error_fg_color, "Disconnected");
                    }
                }
                if !self.names.is_main() {
                    ui.weak(&self.names.channel);
                }
            });
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let pending = self
                .owners
                .iter()
                .filter(|o| o.requested_owner.is_some() || o.binding == "pending")
                .count();
            for (tab, label) in [
                (
                    Tab::Inbox,
                    format!(
                        "Inbox ({})",
                        self.open_count.unwrap_or(self.inbox.len() as u32)
                    ),
                ),
                (Tab::Agents, "Agents".into()),
                (Tab::Board, "Board".into()),
                (
                    Tab::Settings,
                    if pending == 0 {
                        "Settings".into()
                    } else {
                        format!("Settings · {pending} to review")
                    },
                ),
            ] {
                if ui
                    .add(egui::Button::selectable(self.tab == tab, label))
                    .clicked()
                {
                    self.switch_to(tab);
                }
            }
            let more_label = match self.tab {
                Tab::History => "More: History",
                Tab::Holds => "More: Work holds",
                Tab::Activity => "More: Activity",
                _ => "More",
            };
            ui.menu_button(more_label, |ui| {
                for (tab, label) in [
                    (Tab::History, "History"),
                    (Tab::Holds, "Work holds"),
                    (Tab::Activity, "Activity"),
                ] {
                    if ui.selectable_label(self.tab == tab, label).clicked() {
                        self.switch_to(tab);
                        ui.close();
                    }
                }
            });
        });
    }

    fn ask_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong(
                if self.inbox.is_empty() && self.inbox_after.is_none() && !self.inbox_more {
                    "Nothing waiting"
                } else if self.inbox.is_empty() {
                    "No questions on this page"
                } else {
                    "Waiting for you"
                },
            );
            if !self.batch_mode
                && self.inbox.len() > 1
                && ui.small_button("Select several").clicked()
            {
                self.batch_mode = true;
                self.selected_recent = None;
            }
        });
        if self.inbox_more || self.inbox_after.is_some() || !self.inbox_back.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.weak(format!("Page {}", self.inbox_back.len() + 1));
                if ui
                    .add_enabled(
                        !self.list_inflight && !self.inbox_back.is_empty(),
                        egui::Button::new("Previous"),
                    )
                    .clicked()
                {
                    self.inbox_after = self.inbox_back.pop().flatten();
                    self.last_list = None;
                    self.batch_selection.clear();
                    self.selected_recent = None;
                }
                if ui
                    .add_enabled(
                        !self.list_inflight && self.inbox_more,
                        egui::Button::new("Next"),
                    )
                    .clicked()
                {
                    self.inbox_back.push(self.inbox_after.clone());
                    self.inbox_after = self.inbox_next.clone();
                    self.last_list = None;
                    self.batch_selection.clear();
                    self.selected_recent = None;
                }
                if ui.button("First page").clicked() {
                    self.inbox_after = None;
                    self.inbox_back.clear();
                    self.last_list = None;
                    self.batch_selection.clear();
                    self.selected_recent = None;
                }
            });
        }
        if self.batch_mode {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.batch_selection.is_empty(),
                        egui::Button::new(format!("Review {} answers", self.batch_selection.len())),
                    )
                    .clicked()
                {
                    self.review_batch = true;
                    self.selected_recent = None;
                }
                if ui.small_button("Cancel selection").clicked() {
                    self.batch_selection.clear();
                    self.selected_recent = None;
                    self.batch_mode = false;
                    self.review_batch = false;
                }
            });
        }
        let asks = self.inbox.clone();
        let now = time::OffsetDateTime::now_utc();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for a in &asks {
                    let selected = &mut self.selected_inbox;
                    let is_sel = selected.as_deref() == Some(a.id.as_str())
                        && self.selected_recent.is_none()
                        && !self.review_batch;
                    let (due, overdue) = view::relative_deadline(&a.input.deadline, now);
                    let frame = theme::surface(ui).inner_margin(10).fill(if is_sel {
                        ui.visuals().selection.bg_fill
                    } else {
                        ui.visuals().extreme_bg_color
                    });
                    let resp = frame
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if self.batch_mode {
                                ui.horizontal(|ui| {
                                    let mut checked = self.batch_selection.contains(&a.id);
                                    let allowed =
                                        checked || self.batch_selection.len() < MAX_BATCH_ANSWERS;
                                    let checkbox = ui.add_enabled(
                                        allowed,
                                        egui::Checkbox::new(&mut checked, "Select"),
                                    );
                                    checkbox.widget_info(|| {
                                        egui::WidgetInfo::selected(
                                            egui::accesskit::Role::CheckBox,
                                            allowed,
                                            checked,
                                            format!("Select {} for batch answer", a.input.title),
                                        )
                                    });
                                    if checkbox.changed() {
                                        if checked {
                                            self.batch_selection.insert(a.id.clone());
                                        } else {
                                            self.batch_selection.remove(&a.id);
                                        }
                                    }
                                });
                            }
                            ui.add(egui::Label::new(RichText::new(&a.input.title).strong()).wrap());
                            ui.horizontal_wrapped(|ui| {
                                if a.input.priority != Priority::P2 {
                                    priority_tag(ui, a.input.priority);
                                }
                                ui.weak(&a.owner);
                                if a.muted {
                                    ui.weak("Muted");
                                }
                                ui.weak("·");
                                let c = if overdue {
                                    ui.visuals().error_fg_color
                                } else {
                                    ui.visuals().weak_text_color()
                                };
                                ui.colored_label(c, due);
                            });
                        })
                        .response;
                    let resp = resp.interact(egui::Sense::click());
                    resp.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::accesskit::Role::Button,
                            ui.is_enabled(),
                            is_sel,
                            &a.input.title,
                        )
                    });
                    if is_sel {
                        ui.painter().rect_stroke(
                            resp.rect,
                            4.0,
                            ui.visuals().selection.stroke,
                            egui::StrokeKind::Inside,
                        );
                    }
                    if resp.clicked() {
                        *selected = Some(a.id.clone());
                        self.selected_recent = None;
                        self.review_batch = false;
                    }
                    ui.add_space(2.0);
                }
                if !self.recent.is_empty() {
                    ui.add_space(12.0);
                    ui.separator();
                    let response = egui::CollapsingHeader::new(format!(
                        "Past questions ({})",
                        self.recent.len()
                    ))
                    .id_salt("past-questions")
                    .default_open(false)
                    .open(self.recent_open.is_none().then_some(false))
                    .show_accessible(ui, |ui| {
                        for ask in &self.recent {
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        self.selected_recent.as_deref() == Some(ask.id.as_str()),
                                        RichText::new(format!(
                                            "{} · {}",
                                            view::closed_status(ask),
                                            ask.input.title
                                        ))
                                        .weak(),
                                    )
                                    .wrap(),
                                )
                                .clicked()
                            {
                                self.selected_recent = Some(ask.id.clone());
                                self.review_batch = false;
                            }
                        }
                        if ui.small_button("View all history").clicked() {
                            self.switch_to(Tab::History);
                        }
                    });
                    let open = egui::collapsing_header::CollapsingState::load(
                        ui.ctx(),
                        response.header_response.id,
                    )
                    .is_some_and(|state| state.is_open());
                    self.recent_open = Some(open);
                    if !open {
                        self.selected_recent = None;
                    }
                    ui.weak("No reply needed");
                }
            });
    }

    /// The selected ask's details and its answer controls. Returns an answer to send.
    fn details(&mut self, ui: &mut egui::Ui, ask: &Ask) -> Option<Req> {
        let now = time::OffsetDateTime::now_utc();
        let mut req = None;
        let active = matches!(ask.status, AskStatus::Open | AskStatus::Digest);
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("details", &ask.id)).show(ui, |ui| {
            if !active { ui.weak(format!("{} · No reply needed", view::closed_status(ask))); }
            ui.add(egui::Label::new(RichText::new(&ask.input.title).heading().strong()).wrap());
            ui.horizontal_wrapped(|ui| {
                if active && ask.input.priority != Priority::P2 { priority_tag(ui, ask.input.priority); }
                ui.label(format!("from {}", ask.owner));
                if let Some(machine) = ask.asking_machine.as_deref().and_then(|m| m.strip_prefix("remote:")) {
                    ui.label(format!("on {machine}"));
                }
                if active {
                let (due, overdue) = view::relative_deadline(&ask.input.deadline, now);
                let c = if overdue { ui.visuals().error_fg_color } else { ui.visuals().text_color() };
                ui.colored_label(c, due).on_hover_text(&ask.input.deadline);
                let tag = view::reversible_tag(ask);
                let c = if ask.input.reversible { ui.visuals().weak_text_color() } else { ui.visuals().warn_fg_color };
                ui.colored_label(c, tag);
                }
            });
            if active && let Some(why) = &ask.input.p0_reason {
                ui.label(format!("Why it's urgent: {why}"));
            }
            ui.separator();
            if ask.input.body.trim().is_empty() {
                ui.weak("No details given.");
            } else {
                ui.add(egui::Label::new(&ask.input.body).selectable(true).wrap());
            }
            if !ask.input.evidence.is_empty() {
                ui.add_space(6.0);
                egui::CollapsingHeader::new(format!("Supporting links ({})", ask.input.evidence.len()))
                    .id_salt(("evidence", &ask.id)).show_accessible(ui, |ui| {
                let exists = self.paths.for_ask(ask, |p| p.exists());
                for e in &ask.input.evidence {
                    evidence_row(ui, &e.label, &e.url, exists, ask.asking_machine.as_deref());
                }
                if ask.asking_machine.is_none() && ask.input.evidence.iter().any(|e| !matches!(view::classify(&e.url), Link::Web(_))) {
                    ui.weak("Source computer unavailable. Copy file and chat references to open them on the originating computer.");
                }
                });
            }
            ui.separator();
            if !active {
                if let Some(answer) = &ask.answer {
                    let label = match answer.via {
                        AnswerVia::NotNeeded => "Marked Not needed",
                        AnswerVia::Defaulted => "Default applied — no approval from Artem",
                        AnswerVia::Desk => "Answered in the Desk",
                        AnswerVia::Toast => "Answered from a notification",
                        AnswerVia::Phone => "Answered from the phone",
                    };
                    ui.strong(label);
                    ui.weak(&answer.answered_at);
                    if let Some(choice) = &answer.choice { ui.add(egui::Label::new(choice).selectable(true).wrap()); }
                    if let Some(values) = &answer.values { ui.add(egui::Label::new(values.to_string()).selectable(true).wrap()); }
                    if let Some(note) = &answer.note { ui.label("Note"); ui.add(egui::Label::new(note).selectable(true).wrap()); }
                    self.deliveries(ui, answer);

                } else { ui.strong("Withdrawn by the agent"); }
                self.question_source(ui, ask);
                return;
            }
            let sending = self.sending.contains(&ask.id);
            let draft = self.drafts.entry(ask.id.clone()).or_insert_with(Draft::fresh);
            ui.strong("Your answer");
            if matches!(ask.input.kind, AskKind::Choose | AskKind::Approve | AskKind::Inform) {
                ui.weak("Choose an option to send your answer immediately.");
            }
            ui.add_enabled_ui(!sending, |ui| {
                // The note comes first: an option button answers at once.
                egui::CollapsingHeader::new(if draft.note.is_empty() { "Add a note" } else { "Note included with your answer" })
                    .id_salt(("answer-note", &ask.id))
                    .default_open(!draft.note.is_empty())
                    .show_accessible(ui, |ui| {
                    if ui.add(
                        egui::TextEdit::singleline(&mut draft.note)
                            .hint_text("optional, sent with your answer")
                            .desired_width(f32::INFINITY),
                    ).changed() { draft.edited(); }
                });
                ui.add_space(4.0);
                req = answer_controls(ui, ask, draft);
                ui.add_space(6.0);
                ui.menu_button("Other actions", |ui| {
                    if ui.button("Mark as not needed").on_hover_text("Tell the agent this question was unnecessary. No option is approved.").clicked() {
                        req = Some(Req::NotNeeded { ask_id: ask.id.clone(), note: None, one_click: None });
                        ui.close();
                    }
                });
            });
            if sending {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Sending...");
                });
            }
            if !ask.input.reversible {
                ui.weak("This can't be undone, so it never takes its default: only your answer moves it.");
            }
            ui.add_space(8.0);
            self.question_source(ui, ask);
        });
        let note = self
            .drafts
            .get(&ask.id)
            .map(|d| d.note.trim().to_string())
            .filter(|n| !n.is_empty());
        let one_click = self.drafts.get(&ask.id).and_then(|draft| draft.one_click);
        req.map(|r| match r {
            Req::Answer {
                ask_id,
                choice,
                values,
                ..
            } => Req::Answer {
                ask_id,
                choice,
                values,
                note,
                one_click,
            },
            Req::NotNeeded { ask_id, .. } => Req::NotNeeded {
                ask_id,
                note,
                one_click,
            },
            other => other,
        })
    }

    fn question_source(&mut self, ui: &mut egui::Ui, ask: &Ask) {
        egui::CollapsingHeader::new("Source and notifications")
            .id_salt(("source", &ask.id))
            .show_accessible(ui, |ui| {
                if let Some(target) = chat_button(
                    ui,
                    ask.asking_client.as_deref().unwrap_or_default(),
                    ask.asking_session.as_deref(),
                    ask.asking_machine.as_deref(),
                    ask.asking_cwd.as_deref(),
                ) && let Err(e) = os::open_chat(&target, ask.asking_cwd.as_deref())
                {
                    self.note(e, true);
                }
                if let Some(req) = mute_control(ui, &ask.owner, ask.muted) {
                    self.send(req);
                }
            });
    }

    fn load_history(&mut self) {
        self.history_inflight = true;
        self.send(Req::History(self.history_query.clone()));
    }

    fn history_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("History");
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.history_search)
                    .hint_text("Search question, owner or ID")
                    .desired_width(260.0),
            );
            if ui
                .add_enabled(!self.history_inflight, egui::Button::new("Search"))
                .clicked()
                || (!self.history_inflight
                    && field.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                self.history_query = agentbolt_core::protocol::HistoryQuery {
                    before: None,
                    search: self.history_search.trim().into(),
                };
                self.history_back.clear();
                self.load_history();
            }
            if !self.history_search.is_empty()
                && ui
                    .add_enabled(!self.history_inflight, egui::Button::new("Clear"))
                    .clicked()
            {
                self.history_search.clear();
                self.history_query = Default::default();
                self.history_back.clear();
                self.load_history();
            }
        });
        ui.horizontal(|ui| {
            if self.history.more || !self.history_back.is_empty() {
                if ui
                    .add_enabled(
                        !self.history_inflight && !self.history_back.is_empty(),
                        egui::Button::new("Newer"),
                    )
                    .clicked()
                {
                    self.history_query.before = self.history_back.pop().flatten();
                    self.load_history();
                }
                if ui
                    .add_enabled(
                        !self.history_inflight && self.history.more,
                        egui::Button::new("Older"),
                    )
                    .clicked()
                {
                    self.history_back.push(self.history_query.before.clone());
                    self.history_query.before = self.history.next.clone();
                    self.load_history();
                }
            }
            ui.weak(if self.history.more || !self.history_back.is_empty() {
                format!(
                    "{} past questions · page {}",
                    self.history.total,
                    self.history_back.len() + 1
                )
            } else {
                format!("{} past questions", self.history.total)
            });
            if self.history_inflight {
                ui.spinner();
            }
        });
        ui.separator();
        egui::Panel::left("history-list")
            .resizable(true)
            .default_size(300.0)
            .min_size(200.0)
            .max_size((ui.available_width() * 0.42).clamp(200.0, 420.0))
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 4)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("history-rows")
                    .show(ui, |ui| {
                        if self.history.asks.is_empty() && !self.history_inflight {
                            ui.weak("No matching answered or withdrawn questions.");
                        }
                        for ask in &self.history.asks {
                            if ui
                                .selectable_label(
                                    self.history_selected.as_deref() == Some(&ask.id),
                                    format!(
                                        "{}\n{} · {}",
                                        ask.input.title,
                                        view::closed_status(ask),
                                        ask.owner
                                    ),
                                )
                                .clicked()
                            {
                                self.history_selected = Some(ask.id.clone());
                            }
                            ui.separator();
                        }
                    });
            });
        let ask = self
            .history
            .asks
            .iter()
            .find(|a| Some(&a.id) == self.history_selected.as_ref())
            .cloned();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 4)))
            .show(ui, |ui| {
                if let Some(ask) = ask {
                    if let Some(req) = self.details(ui, &ask) {
                        self.send(req);
                    }
                } else {
                    theme::empty(
                        ui,
                        "Past decisions",
                        "Select a question to see its answer and delivery history.",
                    );
                }
            });
    }

    fn deliveries(&mut self, ui: &mut egui::Ui, answer: &agentbolt_core::model::Answer) {
        // The page is chronological. A later successful retry for a target clears its warning.
        let mut targets = HashSet::new();
        if answer
            .deliveries
            .iter()
            .rev()
            .any(|delivery| targets.insert(&delivery.to) && !delivery.ok)
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "An answer delivery failed. See delivery details.",
            );
        }
        egui::CollapsingHeader::new("Delivery details")
            .id_salt(("answer-deliveries", &answer.ask_id))
            .show_accessible(ui, |ui| self.delivery_details(ui, answer));
    }

    fn delivery_details(&mut self, ui: &mut egui::Ui, answer: &agentbolt_core::model::Answer) {
        let expanded = self
            .delivery_view
            .as_ref()
            .is_some_and(|v| v.query.ask_id == answer.ask_id);
        let mut request = None;
        let mut close = false;
        if expanded {
            let view = self
                .delivery_view
                .as_mut()
                .expect("expanded delivery history");
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!(
                    "Delivery history · {} records · page {}",
                    view.page.total,
                    view.back.len() + 1
                ));
                if ui
                    .add_enabled(
                        !view.busy && !view.back.is_empty(),
                        egui::Button::new("Newer deliveries"),
                    )
                    .clicked()
                {
                    view.query.before = view.back.pop().flatten();
                    request = Some(view.query.clone());
                }
                if ui
                    .add_enabled(
                        !view.busy && view.page.more,
                        egui::Button::new("Older deliveries"),
                    )
                    .clicked()
                {
                    view.back.push(view.query.before);
                    view.query.before = view.page.next;
                    request = Some(view.query.clone());
                }
                if ui
                    .add_enabled(!view.busy, egui::Button::new("Latest / refresh"))
                    .clicked()
                {
                    view.query.before = None;
                    view.back.clear();
                    request = Some(view.query.clone());
                }
                close = ui.small_button("Collapse history").clicked();
            });
            if view.busy {
                ui.weak("Loading delivery records...");
            }
            egui::ScrollArea::vertical()
                .id_salt(("delivery-history", &answer.ask_id, view.query.before))
                .max_height(280.0)
                .show(ui, |ui| delivery_rows(ui, &view.page.deliveries));
        } else {
            if answer.deliveries.is_empty() {
                ui.weak("Available for the agent to read; no delivery recorded yet.");
            }
            if answer.delivery_total > 0 && ui.button("Browse delivery history").clicked() {
                let query = agentbolt_core::protocol::DeliveryQuery {
                    ask_id: answer.ask_id.clone(),
                    before: None,
                };
                self.delivery_view = Some(DeliveryView {
                    query: query.clone(),
                    back: Vec::new(),
                    page: Default::default(),
                    busy: false,
                });
                request = Some(query);
            }
            let recent = &answer.deliveries[answer.deliveries.len().saturating_sub(3)..];
            if answer.delivery_total as usize > recent.len() {
                ui.weak(format!(
                    "Latest {} of {} delivery records",
                    recent.len(),
                    answer.delivery_total
                ));
            }
            delivery_rows(ui, recent);
        }
        if close {
            self.delivery_view = None;
        } else if let Some(query) = request {
            if let Some(view) = &mut self.delivery_view {
                view.busy = true;
            }
            self.send(Req::DeliveryHistory(query));
        }
    }

    fn ask_pane(&mut self, ui: &mut egui::Ui) {
        if self.inbox.is_empty() && self.recent.is_empty() && self.inbox_after.is_none() {
            match self.conn {
                Conn::Unknown => theme::empty(
                    ui,
                    "Connecting to your Desk",
                    "Your questions will appear here when Agentbolt connects.",
                ),
                Conn::Down(_) => theme::empty(
                    ui,
                    "Your Desk is offline",
                    "Start Agentbolt above to load your questions. Your saved drafts stay here.",
                ),
                Conn::Up => theme::empty(ui, "You’re all caught up", view::EMPTY_INBOX),
            }
            return;
        }
        egui::Panel::left("inbox-list")
            .resizable(true)
            .default_size(300.0)
            .min_size(200.0)
            .max_size((ui.available_width() * 0.42).clamp(200.0, 420.0))
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 4)))
            .show(ui, |ui| self.ask_list(ui));
        let ask = self
            .selected_recent
            .as_ref()
            .and_then(|id| self.recent.iter().find(|a| &a.id == id))
            .or_else(|| {
                self.selected_inbox
                    .as_ref()
                    .and_then(|id| self.inbox.iter().find(|a| &a.id == id))
            })
            .cloned();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 4)))
            .show(ui, |ui| {
                if self.review_batch {
                    self.batch_review(ui);
                    return;
                }
                match ask {
                    Some(ask) => {
                        if let Some(req) = self.details(ui, &ask) {
                            self.sending.insert(ask.id.clone());
                            self.send(req);
                        }
                    }
                    None => {
                        if self.inbox.is_empty() {
                            match self.conn {
                                Conn::Unknown => theme::empty(ui, "Connecting to your Desk", "Your questions will appear when Agentbolt connects."),
                                Conn::Down(_) => theme::empty(ui, "Your Desk is offline", "Start Agentbolt above to load your questions. Your saved drafts stay here."),
                                Conn::Up if self.inbox_after.is_none() && !self.inbox_more => theme::empty(ui, "You’re all caught up", "Nothing needs your reply. Past questions are tucked away on the left."),
                                Conn::Up => theme::empty(ui, "No questions on this page", "Go to the previous or first page to see waiting questions."),
                            }
                        } else {
                            theme::empty(ui, "Choose a question", "Select a waiting question to review it and reply.");
                        }
                    }
                }
            });
    }

    fn batch_review(&mut self, ui: &mut egui::Ui) {
        ui.heading("Review answers");
        ui.label(
            "Review each question and answer below. Nothing is sent until you press Send answers.",
        );
        if ui.button("Back to individual answers").clicked() {
            self.review_batch = false;
        }
        let asks: Vec<_> = self
            .inbox
            .iter()
            .filter(|a| self.batch_selection.contains(&a.id))
            .cloned()
            .collect();
        let busy = asks.iter().any(|a| self.sending.contains(&a.id));
        let mut ready = !asks.is_empty() && !busy;
        let mut answers = Vec::new();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                for ask in &asks {
                    ui.push_id(&ask.id, |ui| {
                        ui.separator();
                        ui.heading(&ask.input.title);
                        ui.weak(format!(
                            "From {} · {}",
                            ask.owner,
                            view::relative_deadline(
                                &ask.input.deadline,
                                time::OffsetDateTime::now_utc()
                            )
                            .0
                        ));
                        if !ask.input.reversible {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                "This decision cannot be undone.",
                            );
                        }
                        ui.label(&ask.input.body);
                        for e in &ask.input.evidence {
                            ui.label(format!("{}: {}", e.label, e.url));
                        }
                        let draft = self
                            .drafts
                            .entry(ask.id.clone())
                            .or_insert_with(Draft::fresh);
                        draft.edited(); // Reviewing a selected batch is not a direct one-click response.
                        let (choice, values) = match ask.input.kind {
                            AskKind::Choose | AskKind::Approve => {
                                let choice = draft.choice.get_or_insert_with(|| {
                                    ask.input.default.as_str().unwrap_or_default().to_owned()
                                });
                                for option in view::options_for(ask) {
                                    ui.radio_value(
                                        choice,
                                        option.clone(),
                                        view::option_label(ask, &option),
                                    );
                                }
                                (Some(choice.clone()), None)
                            }
                            AskKind::Inform => {
                                ui.label("Answer: Acknowledged");
                                (None, None)
                            }
                            AskKind::Text => {
                                view::fill_reply_once(
                                    &mut draft.text,
                                    &mut draft.text_filled,
                                    ask.input.default.as_str(),
                                );
                                ui.add(
                                    egui::TextEdit::multiline(&mut draft.text)
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY),
                                );
                                (Some(draft.text.clone()), None)
                            }
                            AskKind::Form => match form_controls(ui, ask, draft) {
                                Ok(values) => (None, Some(values)),
                                Err(e) => {
                                    ui.colored_label(ui.visuals().warn_fg_color, e);
                                    ready = false;
                                    (None, None)
                                }
                            },
                        };
                        ui.horizontal(|ui| {
                            ui.label("Note");
                            ui.text_edit_singleline(&mut draft.note);
                        });
                        if !(ask.input.kind == AskKind::Form && values.is_none())
                            && let Err(e) = agentbolt_core::rules::check_answer(
                                &ask.input,
                                choice.clone(),
                                values.clone(),
                            )
                        {
                            ui.colored_label(ui.visuals().warn_fg_color, e);
                            ready = false;
                        }
                        answers.push(DeskAnswer {
                            ask_id: ask.id.clone(),
                            choice,
                            values,
                            note: Some(draft.note.clone()),
                        });
                    });
                }
            });
            ui.separator();
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(format!("Send answers ({})", answers.len())),
                )
                .clicked()
            {
                self.sending
                    .extend(answers.iter().map(|a| a.ask_id.clone()));
                self.send(Req::AnswerBatch(answers));
            }
        });
    }

    fn agents_tab(&mut self, ui: &mut egui::Ui) {
        theme::page_heading(
            ui,
            "Agents",
            "See who is connected, what they’re working on and when they last checked in.",
        );
        if self
            .agents_directory
            .controls(ui, "agents", self.agents_inflight)
        {
            self.last_agents = None;
        }
        if self.agents.is_empty() {
            theme::empty(
                ui,
                if self.agents_directory.query.search.is_empty() {
                    "No agents yet"
                } else {
                    "No matching agents"
                },
                if self.agents_directory.query.search.is_empty() {
                    view::EMPTY_AGENTS
                } else {
                    "Try a different name, client or project."
                },
            );
            return;
        }
        let now = time::OffsetDateTime::now_utc();
        let mut open = None;
        let mut mute = None;
        let mut review_owners = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for a in &self.agents {
                    ui.push_id(&a.handle, |ui| {
                        theme::surface(ui).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.add(egui::Label::new(RichText::new(&a.owner).strong()).wrap());
                                ui.weak(&a.client);
                                if a.provisional {
                                    ui.weak("Provisional name");
                                }
                                if a.muted {
                                    ui.weak("Muted");
                                }
                            });
                            if let Some(proposed) = &a.requested_owner {
                                ui.colored_label(
                                    ui.visuals().warn_fg_color,
                                    format!("Name needs review: {proposed}"),
                                );
                                if ui.button("Review name").clicked() {
                                    review_owners = true;
                                }
                            }
                            if let Some(doing) = a.doing.as_deref().filter(|s| !s.is_empty()) {
                                ui.add(egui::Label::new(doing).wrap());
                            }
                            egui::CollapsingHeader::new("Details and actions")
                                .id_salt(("agent-details", &a.handle))
                                .show_accessible(ui, |ui| {
                                    ui.weak(format!("Last seen {}", view::ago(&a.last_seen, now)))
                                        .on_hover_text(&a.last_seen);
                                    if let Some(target) = chat_button(
                                        ui,
                                        &a.client,
                                        a.session_ref.as_deref(),
                                        Some(&a.machine),
                                        a.cwd.as_deref(),
                                    ) {
                                        open = Some((target, a.cwd.clone()));
                                    }
                                    if let Some(req) = mute_control(ui, &a.owner, a.muted) {
                                        mute = Some(req);
                                    }
                                });
                        });
                        ui.add_space(4.0);
                    });
                }
            });
        if let Some(req) = mute {
            self.send(req);
        }
        if review_owners {
            self.settings_section = SettingsSection::Owners;
            self.owners_directory = DirectoryState::default();
            self.last_owners = None;
            self.switch_to(Tab::Settings);
        }
        if let Some((target, cwd)) = open
            && let Err(e) = os::open_chat(&target, cwd.as_deref())
        {
            self.note(e, true);
        }
    }

    fn board_tab(&mut self, ui: &mut egui::Ui) {
        let mut open = None;
        let mut save = None;
        let mut reload = false;
        theme::page_heading(
            ui,
            "Agent Board",
            "Messages and handoffs between agents. These do not grant your approval.",
        );
        ui.horizontal_wrapped(|ui| {
            for (section, label, count) in [
                (
                    BoardSection::Handoffs,
                    "Handoffs",
                    self.board.total_requests,
                ),
                (
                    BoardSection::Messages,
                    "Messages",
                    self.board.total_messages,
                ),
                (
                    BoardSection::Agents,
                    "Who is working",
                    self.board.total_agents,
                ),
            ] {
                ui.selectable_value(
                    &mut self.board_section,
                    section,
                    format!("{label} ({count})"),
                );
            }
        });
        if self.board_section != BoardSection::Agents {
            ui.horizontal_wrapped(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.board_search)
                        .hint_text("Find request or message")
                        .desired_width(240.0),
                );
                if ui
                    .add_enabled(!self.board_inflight, egui::Button::new("Search"))
                    .clicked()
                    || (!self.board_inflight
                        && field.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    self.board_query.search = self.board_search.trim().into();
                    self.board_query.requests_before = None;
                    self.board_query.messages_before = None;
                    self.board_request_back.clear();
                    self.board_message_back.clear();
                    reload = true;
                }
                if !self.board_search.is_empty()
                    && ui
                        .add_enabled(!self.board_inflight, egui::Button::new("Clear"))
                        .clicked()
                {
                    self.board_search.clear();
                    self.board_query.search.clear();
                    self.board_query.requests_before = None;
                    self.board_query.messages_before = None;
                    self.board_request_back.clear();
                    self.board_message_back.clear();
                    reload = true;
                }
                if self.board_section == BoardSection::Handoffs {
                    ui.menu_button(
                        if self.board_query.include_closed {
                            "Filter: all handoffs"
                        } else {
                            "Filter: active handoffs"
                        },
                        |ui| {
                            if ui
                                .checkbox(
                                    &mut self.board_query.include_closed,
                                    "Include completed, failed and cancelled",
                                )
                                .changed()
                            {
                                self.board_query.requests_before = None;
                                self.board_request_back.clear();
                                reload = true;
                            }
                        },
                    );
                }
            });
        }
        match self.board_section {
            BoardSection::Handoffs => {
                if self.board.more_requests || !self.board_request_back.is_empty() {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.board_inflight && !self.board_request_back.is_empty(),
                                egui::Button::new("Newer handoffs"),
                            )
                            .clicked()
                        {
                            self.board_query.requests_before =
                                self.board_request_back.pop().flatten();
                            reload = true;
                        }
                        if ui
                            .add_enabled(
                                !self.board_inflight && self.board.more_requests,
                                egui::Button::new("Older handoffs"),
                            )
                            .clicked()
                        {
                            self.board_request_back
                                .push(self.board_query.requests_before.clone());
                            self.board_query.requests_before = self.board.request_next.clone();
                            reload = true;
                        }
                        ui.weak(format!(
                            "{} matching handoffs · page {}",
                            self.board.total_requests,
                            self.board_request_back.len() + 1
                        ));
                    });
                }

                egui::ScrollArea::vertical()
                    .id_salt(("board-handoffs", self.board_query.requests_before.as_ref().map(|c| (&c.id, c.updated_ms)), &self.board_query.search, self.board_query.include_closed))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.board.requests.is_empty() { ui.weak("No handoffs match this view."); }
                        for request in &self.board.requests {
                            let status = if request.lapsing { "Check-in due" } else { &request.status };
                            let title = RichText::new(format!("{status} · {}", request.input.title));
                            let title = if request.lapsing || request.status == "failed" { title.color(ui.visuals().warn_fg_color) } else { title };
                            egui::CollapsingHeader::new(title)
                                .id_salt(&request.id)
                                .show_accessible(ui, |ui| {
                                    ui.weak(format!("From {} · {}", request.owner, request.created_at));
                                    if let Some(role) = &request.input.to_role { ui.label(format!("For {role}")); }
                                    if let Some(repo) = &request.input.repo { ui.label(format!("{repo} · {}", request.input.paths.join(", "))); }
                                    if let Some(beads) = &request.input.beads_issue { ui.label(format!("Beads issue: {beads}")); }
                                    ui.add(egui::Label::new(&request.input.body).selectable(true).wrap());
                                    if let Some(claimant) = &request.claimant {
                                        let agent = self.board.claimants.iter().chain(&self.board.agents).find(|a| &a.id == claimant);
                                        ui.label(format!("Claimed by {}", agent.map_or(claimant.as_str(), |a| a.owner.as_str())));
                                        if let Some(agent) = agent
                                            && let Some(target) = chat_button(ui, &agent.client, agent.session_ref.as_deref(), Some(&agent.machine), agent.cwd.as_deref()) {
                                            open = Some((target, agent.cwd.clone()));
                                        }
                                    }
                                    if let Some(until) = &request.lease_until { ui.label(format!("Lease until {until}")); }
                                    if request.lapsing { ui.colored_label(ui.visuals().warn_fg_color, "Lease expires within 15 minutes unless the agent checks in."); }
                                    if let Some(result) = &request.result {
                                        ui.strong("Result");
                                        ui.add(egui::Label::new(result).selectable(true).wrap());
                                    }
                                });
                        }
                    });
            }
            BoardSection::Agents => {
                ui.horizontal_wrapped(|ui| {
                    ui.menu_button(
                        if self.board_query.include_inactive {
                            "Filter: all agents"
                        } else {
                            "Filter: active agents"
                        },
                        |ui| {
                            if ui
                                .checkbox(
                                    &mut self.board_query.include_inactive,
                                    "Include inactive agents",
                                )
                                .changed()
                            {
                                self.board_query.agents_after = 0;
                                self.board_agent_back.clear();
                                reload = true;
                            }
                        },
                    );
                    if self.board.more_agents || !self.board_agent_back.is_empty() {
                        if ui
                            .add_enabled(
                                !self.board_inflight && !self.board_agent_back.is_empty(),
                                egui::Button::new("Previous agents"),
                            )
                            .clicked()
                        {
                            self.board_query.agents_after =
                                self.board_agent_back.pop().unwrap_or(0);
                            reload = true;
                        }
                        if ui
                            .add_enabled(
                                !self.board_inflight && self.board.more_agents,
                                egui::Button::new("More agents"),
                            )
                            .clicked()
                        {
                            self.board_agent_back.push(self.board_query.agents_after);
                            self.board_query.agents_after = self.board.agent_next;
                            reload = true;
                        }
                    }
                    ui.weak(format!(
                        "{} agents · page {}",
                        self.board.total_agents,
                        self.board_agent_back.len() + 1
                    ));
                });

                egui::ScrollArea::vertical()
                    .id_salt((
                        "board-agents",
                        self.board_query.agents_after,
                        self.board_query.include_inactive,
                    ))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for a in &self.board.agents {
                            egui::CollapsingHeader::new(format!(
                                "{} · {} · {}",
                                a.owner,
                                if a.alive { "active" } else { "inactive" },
                                a.roles.join(", ")
                            ))
                            .id_salt(&a.id)
                            .show_accessible(ui, |ui| {
                                ui.label(format!("{} on {}", a.client, a.machine));
                                if let Some(doing) = &a.doing {
                                    ui.add(egui::Label::new(doing).wrap());
                                }
                                ui.weak(format!("Last seen {}", a.last_seen));
                                if let Some(target) = chat_button(
                                    ui,
                                    &a.client,
                                    a.session_ref.as_deref(),
                                    Some(&a.machine),
                                    a.cwd.as_deref(),
                                ) {
                                    open = Some((target, a.cwd.clone()));
                                }
                                egui::CollapsingHeader::new("Claim timeout")
                                    .id_salt(("claim-timeout", &a.id))
                                    .show_accessible(ui, |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            let seconds = self
                                                .lease_drafts
                                                .entry(a.owner.clone())
                                                .or_insert(a.lease_seconds);
                                            ui.label("Release claims after inactivity (seconds)");
                                            ui.add(
                                                egui::DragValue::new(seconds)
                                                    .range(60..=86_400)
                                                    .speed(60),
                                            );
                                            if ui.button("Save ceiling").clicked() {
                                                save = Some(Req::SetLease {
                                                    owner: a.owner.clone(),
                                                    seconds: *seconds,
                                                });
                                            }
                                        });
                                    });
                            });
                        }
                    });
            }
            BoardSection::Messages => {
                if self.board.more_messages || !self.board_message_back.is_empty() {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.board_inflight && !self.board_message_back.is_empty(),
                                egui::Button::new("Newer messages"),
                            )
                            .clicked()
                        {
                            self.board_query.messages_before =
                                self.board_message_back.pop().flatten();
                            reload = true;
                        }
                        if ui
                            .add_enabled(
                                !self.board_inflight && self.board.more_messages,
                                egui::Button::new("Older messages"),
                            )
                            .clicked()
                        {
                            self.board_message_back
                                .push(self.board_query.messages_before);
                            self.board_query.messages_before = self.board.message_next;
                            reload = true;
                        }
                        ui.weak(format!(
                            "{} matching messages · page {}",
                            self.board.total_messages,
                            self.board_message_back.len() + 1
                        ));
                    });
                }

                egui::ScrollArea::vertical()
                    .id_salt((
                        "board-messages",
                        self.board_query.messages_before,
                        &self.board_query.search,
                    ))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.board.messages.is_empty() {
                            ui.weak("No messages match this view.");
                        }
                        for m in &self.board.messages {
                            egui::CollapsingHeader::new(format!("{} · {}", m.owner, m.input.title))
                                .id_salt(&m.id)
                                .show_accessible(ui, |ui| {
                                    use agentbolt_core::board::Destination;
                                    let to = match &m.input.to {
                                        Destination::Everyone => "everyone".into(),
                                        Destination::Role(s) => format!("role {s}"),
                                        Destination::Owner(s) => format!("owner {s}"),
                                        Destination::Agent(s) => s.clone(),
                                    };
                                    ui.weak(format!("To {to} · {}", m.created_at));
                                    if let Some(machine) = &m.sender_machine {
                                        ui.weak(format!("From computer: {}", machine.strip_prefix("remote:").unwrap_or(machine)));
                                    } else if m.input.references.iter().any(|r| !matches!(view::classify(r), Link::Web(_))) {
                                        ui.weak("Source computer unavailable. Copy file and chat references to open them on the originating computer.");
                                    }
                                    if let Some(thread) = &m.input.thread {
                                        ui.label(format!("Thread {thread}"));
                                    }
                                    ui.add(egui::Label::new(&m.input.body).selectable(true).wrap());
                                    for reference in &m.input.references {
                                        evidence_row(ui, reference, reference, &HashMap::new(), m.sender_machine.as_deref());
                                    }
                                });
                        }
                    });
            }
        }
        if reload {
            self.last_board = None;
        }
        if let Some(req) = save {
            self.send(req);
        }
        if let Some((target, cwd)) = open
            && let Err(e) = os::open_chat(&target, cwd.as_deref())
        {
            self.note(e, true);
        }
    }

    fn activity_tab(&mut self, ui: &mut egui::Ui) {
        let mut request = None;
        let mut review_inbox = false;
        egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui,|ui| {
            theme::page_heading(ui, "Activity", "See whether the Desk is helping. Human answers and automatic defaults are shown separately.");
            let Some(report)=&self.metrics else {ui.label("Loading recorded activity…");return;};
            let current=report.observation.as_ref().unwrap_or(&report.current);
            let previous=report.baseline.as_ref().map_or(&report.previous,|b|&b.period);
            if report.reversible_waiting_over_48h > 0 {
                ui.colored_label(ui.visuals().warn_fg_color, format!("Questions waiting over 48 hours: {}", report.reversible_waiting_over_48h));
            }
            if report.irreversible_open > 0 {
                ui.label(format!("Decisions waiting for you: {}", report.irreversible_open));
            }
            if report.irreversible_open > 0 || report.reversible_waiting_over_48h > 0 {
                review_inbox = ui.button("Review Inbox").clicked();
            }
            ui.strong(if report.observation.is_some() { "Current observation" } else { "Latest 14 days" });
            egui::Grid::new("activity-summary").spacing([24.0, 6.0]).min_row_height(20.0).show(ui, |ui| {
                for (label, value) in [
                    ("Questions submitted", current.submitted),
                    ("Human answers", current.human_answers),
                    ("Automatic defaults", current.defaults),
                    ("Missed deadlines", current.unanswered_deadlines),
                ] {
                    ui.label(label);
                    ui.strong(if current.history_span_days.is_some() { value.to_string() } else { "Unknown".into() });
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            egui::CollapsingHeader::new("Detailed activity and comparisons").id_salt("activity-full-comparison").show_accessible(ui, |ui| {
            if let Some(hours) = report.oldest_irreversible_hours { ui.weak(format!("Oldest decision awaiting your answer: {hours:.1} hours")); }
            egui::ScrollArea::horizontal().id_salt("activity-comparison-scroll").show(ui, |ui| {
            egui::Grid::new("activity-comparison").spacing([24.0, 10.0]).striped(true).num_columns(3).show(ui,|ui| {
                ui.strong("Measure");ui.strong(if report.observation.is_some(){"Adoption observation"}else{"Latest 14 days"});ui.strong(if report.baseline.is_some(){"Saved baseline"}else{"Previous 14 days"});ui.end_row();
                for (label,a,b) in [
                    ("Questions submitted",current.submitted,previous.submitted),
                    ("Human answers",current.human_answers,previous.human_answers),
                    ("Defaults",current.defaults,previous.defaults),
                    ("Not needed",current.not_needed,previous.not_needed),
                    ("Withdrawn",current.cancelled,previous.cancelled),
                    ("Missed deadlines",current.unanswered_deadlines,previous.unanswered_deadlines),
                    ("PR/merge overlap checks",current.collision_checks,previous.collision_checks),
                    ("Checks finding overlap",current.overlapping_checks,previous.overlapping_checks),
                    ("Checks with unknown scans",current.unknown_checks,previous.unknown_checks),
                ] {
                    ui.label(label);
                    ui.label(if current.history_span_days.is_some() { a.to_string() } else { "Unknown".into() });
                    ui.label(if previous.history_span_days.is_some() { b.to_string() } else { "Unknown".into() });
                    ui.end_row();
                }
                ui.label("Recorded history span");
                for p in [current,previous] {ui.label(p.history_span_days.map_or("Unknown".into(),|d|format!("{d:.1} days")));}ui.end_row();
                ui.label("Median answer time");
                for p in [current,previous] {ui.label(if p.history_span_days.is_none() { "Unknown".into() } else {p.median_answer_hours.map_or("No human answers".into(),|h|format!("{h:.1} hours"))});}ui.end_row();
                ui.label("Unanswered at deadline");
                for p in [current,previous] {ui.label(if p.history_span_days.is_none() { "Unknown".into() } else {p.unanswered_deadline_share.map_or("No elapsed deadlines".into(),|v|format!("{:.0}%",v*100.0))});}ui.end_row();
                ui.label("One-click answers / shown");
                for p in [current,previous] { ui.label(view::one_click_label(p)); } ui.end_row();
            });
            });
            egui::CollapsingHeader::new("By agent owner").show_accessible(ui, |ui| {
            egui::ScrollArea::both().id_salt("activity-owners-scroll").max_height(220.0).show(ui, |ui| {
            egui::Grid::new("activity-owners").striped(true).show(ui,|ui| {
                for title in ["Owner","Submitted","Not needed / submitted","Missed deadlines"] {ui.strong(title);}ui.end_row();
                for o in &current.owners {
                    ui.label(&o.owner);ui.label(o.submitted.to_string());ui.label(format!("{} / {}",o.not_needed,o.submitted));ui.label(o.unanswered_deadlines.to_string());ui.end_row();
                }
            });
            });
            });
            egui::CollapsingHeader::new("Questions submitted by day (UTC)").show_accessible(ui,|ui| {
                for (day,count) in &current.submissions_by_day {ui.label(format!("{day}: {count}"));}
            });
            egui::CollapsingHeader::new("Methodology and export").id_salt("activity-methods").show_accessible(ui, |ui| {
                ui.add(egui::Label::new(&report.explanation).wrap());
                if ui.button("Copy report as JSON").clicked() {ui.ctx().copy_text(serde_json::to_string_pretty(report).unwrap_or_default());}
            });
            });
            egui::CollapsingHeader::new("Adoption baseline").id_salt("activity-baseline").show_accessible(ui, |ui| {
            if let Some(b)=&report.baseline {
                ui.label(format!("{} · saved {}",b.label,b.recorded_at));
                ui.label(format!("{:.1} of 14 observation days elapsed",report.observation_days.unwrap_or_default()));
                ui.weak("The comparison stays anchored to the first 14 days after this baseline. Elapsed time alone does not prove usage, uptime or improvement.");
            } else {
                ui.label("Record the preceding 14 days before beginning adoption. Only activity already in this store is included; missing earlier history is unknown.");
                ui.text_edit_singleline(&mut self.baseline_label);
                if ui.add_enabled(!self.baseline_label.trim().is_empty(),egui::Button::new("Record baseline and start observation")).clicked() {request=Some(Req::StartBaseline{label:self.baseline_label.clone()});}
            }
            });
            let verdict_changed = !self.verdict_draft.trim().is_empty() && report.verdict.as_ref().is_none_or(|v| v.text.trim() != self.verdict_draft.trim());
            egui::CollapsingHeader::new(if verdict_changed { "Your verdict · unsaved draft" } else { "Your verdict" }).id_salt("activity-verdict").show_accessible(ui, |ui| {
            if let Some(v)=&report.verdict {ui.label(format!("{} · {}",v.recorded_at,v.text));}
            ui.add(egui::TextEdit::multiline(&mut self.verdict_draft).hint_text("Does this work better for you?").desired_rows(3).desired_width(f32::INFINITY));
            if ui.add_enabled(!self.verdict_draft.trim().is_empty(),egui::Button::new("Save verdict")).clicked() {request=Some(Req::RecordVerdict{text:self.verdict_draft.clone()});}
            });
        });
        if review_inbox {
            self.switch_to(Tab::Inbox);
        }
        if let Some(request) = request {
            self.send(request);
        }
    }

    fn holds_tab(&mut self, ui: &mut egui::Ui) {
        let mut request = None;
        theme::page_heading(
            ui,
            "Work holds",
            "See where work overlaps before agents open or merge a PR. Holds are advisory; they do not lock files.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.configure_holds, false, "Active holds");
            ui.selectable_value(&mut self.configure_holds, true, "Configure working copies");
        });
        if !self.configure_holds {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} active agent holds · page {}",
                    self.holds.total_agent_holds,
                    self.holds_back.len() + 1
                ));
                if self.holds.more || !self.holds_back.is_empty() {
                    if ui
                        .add_enabled(
                            !self.holds_inflight && !self.holds_back.is_empty(),
                            egui::Button::new("Previous holds"),
                        )
                        .clicked()
                    {
                        self.holds_after = self.holds_back.pop().flatten();
                        self.last_holds = None;
                    }
                    if ui
                        .add_enabled(
                            !self.holds_inflight && self.holds.more,
                            egui::Button::new("More holds"),
                        )
                        .clicked()
                    {
                        self.holds_back.push(self.holds_after.clone());
                        self.holds_after = self.holds.next.clone();
                        self.last_holds = None;
                    }
                    if ui.button("First page").clicked() {
                        self.holds_after = None;
                        self.holds_back.clear();
                        self.last_holds = None;
                    }
                }
            });
            egui::ScrollArea::vertical().id_salt(("active-holds", &self.holds_after)).auto_shrink([false, false]).show(ui, |ui| {
            let now=agentbolt_core::store::to_ms(time::OffsetDateTime::now_utc());
            if self.holds.config.working_copies.is_empty() {
                ui.weak("No working copies configured. Filesystem scanning is off; only agent holds are shown.");
            }
            if self.holds.holds.is_empty() { ui.weak("No active agent holds on this page."); }
            for source in &self.holds.config.working_copies {
                ui.add_space(8.0);
                ui.strong(&source.repo);
                if agentbolt_core::holds::excluded(&source.path,&self.holds.config) { ui.colored_label(ui.visuals().warn_fg_color,"This copy overlaps an excluded root; collision status is unknown."); }
                let scan = self.holds.scans.iter().find(|s| s.path == source.path);
                if let Some(scan)=scan {
                    let stale=now.saturating_sub(scan.at_ms)>agentbolt_core::holds::FRESH_MS;
                    if let Some(error)=&scan.error{ui.colored_label(ui.visuals().warn_fg_color,error);}
                    if stale{ui.colored_label(ui.visuals().warn_fg_color,"Scan is stale; overlap status is unknown.");}
                    ui.weak(format!("{} changed paths",scan.paths.len()));
                }else{ui.colored_label(ui.visuals().warn_fg_color,"Waiting for the first scan; overlap status is unknown.");}
                egui::CollapsingHeader::new("Scan details").id_salt(("scan-details", &source.path)).show_accessible(ui, |ui| {
                    ui.add(egui::Label::new(&source.path).selectable(true).wrap());
                    if let Some(scan) = scan {
                        ui.weak(format!("Last scan {}",agentbolt_core::store::fmt_ms(scan.at_ms)));
                        if scan.conservative_submodules { ui.weak("Submodule paths are held conservatively; nested checkouts are not scanned."); }
                    }
                });
            }
            for hold in &self.holds.holds {
                egui::CollapsingHeader::new(format!("{} · {} · {} paths",hold.owner,hold.repo,hold.paths.len())).id_salt(&hold.id).show_accessible(ui,|ui|{
                    ui.label(&hold.note);
                    if let Some(until)=&hold.until{ui.weak(format!("Until {until}, renewed while the agent is active"));}
                    for path in &hold.paths{ui.add(egui::Label::new(path).selectable(true).wrap());}
                });
            }
            });
        } else {
            egui::ScrollArea::vertical().id_salt("configure-holds").auto_shrink([false, false]).show(ui, |ui| {
            ui.heading("Working copies");
            ui.label("Only the folders saved here are scanned, using read-only Git status and unpushed history. Scanning is off when the list is empty. Add closed or private roots to exclusions before importing a list.");
            ui.label("Excluded roots, one absolute folder per line");
            ui.add(egui::TextEdit::multiline(&mut self.hold_excludes).desired_rows(3).desired_width(f32::INFINITY));
            egui::CollapsingHeader::new("Import projects from FOCUS.json").id_salt("import-working-copies").show_accessible(ui, |ui| {
            ui.horizontal_wrapped(|ui|{
                ui.label("FOCUS.json");ui.text_edit_singleline(&mut self.focus_file);
                if ui.add_enabled(!self.focus_file.trim().is_empty(),egui::Button::new("Import eligible projects")).clicked(){request=Some(Req::ImportFocus{path:self.focus_file.clone(),excluded:self.hold_excludes.lines().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect()});}
            });
            ui.weak("Import includes only invest/maintain entries with a GitHub repository and path. It does not save or scan until you review and press Save.");
            });
            if let Some(draft)=&mut self.hold_draft {
                let mut remove=None;
                for (i,source) in draft.working_copies.iter().enumerate(){ui.horizontal_wrapped(|ui|{
                    ui.label(format!("{} · {}",source.repo,source.path));if ui.small_button("Remove").clicked(){remove=Some(i);}
                });}
                if let Some(i)=remove{draft.working_copies.remove(i);}
                egui::CollapsingHeader::new("Add a working copy").id_salt("add-working-copy").show_accessible(ui, |ui| {
                ui.horizontal_wrapped(|ui|{
                    ui.add(egui::TextEdit::singleline(&mut self.new_hold_repo).hint_text("GitHub owner/repository"));
                    ui.add(egui::TextEdit::singleline(&mut self.new_hold_path).hint_text("Absolute local folder"));
                    if ui.add_enabled(!self.new_hold_repo.trim().is_empty()&&!self.new_hold_path.trim().is_empty(),egui::Button::new("Add")).clicked(){
                        draft.working_copies.push(agentbolt_core::holds::WorkingCopy{repo:self.new_hold_repo.trim().into(),path:self.new_hold_path.trim().into()});self.new_hold_repo.clear();self.new_hold_path.clear();
                    }
                });
                });
                if ui.add(theme::primary("Save working copies and exclusions")).clicked(){
                    draft.excluded_paths=self.hold_excludes.lines().map(str::trim).filter(|s|!s.is_empty()).map(str::to_owned).collect();request=Some(Req::SetHolds(draft.clone()));
                }
            }
        });
        }
        if let Some(request) = request {
            self.send(request);
        }
    }

    fn phone_settings(&mut self, ui: &mut egui::Ui) {
        use agentbolt_core::phone::{Secret, Update, time_label};
        ui.set_width(ui.available_width());
        if let Some(status) = &self.phone {
            ui.strong(if status.settings.enabled {
                "Phone replies are enabled"
            } else {
                "Phone replies are off"
            });
            if status.pending_replies > 0 {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "{} phone replies waiting for the Desk",
                        status.pending_replies
                    ),
                );
            }
            if let Some(error) = &status.health.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
        ui.weak("Receive questions on your phone and reply to simple choices.");
        let mut update = None;
        egui::CollapsingHeader::new(if self.phone.as_ref().is_some_and(|p| p.settings.enabled) { "Connection settings" } else { "Set up phone replies" })
            .id_salt("phone-connection-settings").show_accessible(ui, |ui| {
        ui.label("Question text is shared with your ntfy server and subscribed phone. Use private topics with anonymous reading and publishing denied.");
        egui::CollapsingHeader::new("Setup and privacy").show_accessible(ui, |ui| {
            ui.label("The Desk checks anonymous read protection before sending.");
            ui.label("Reversible choices with up to three short options get buttons; all other questions need the Desk.");
            ui.label("The Desk account needs read/write access to both topics. Use a separate phone publisher account that can only write to the replies topic. Subscribe to the questions topic in your ntfy app. Low-priority messages request no sound; keep that channel silent on the phone.");
        });
        if let Some(draft) = &mut self.phone_draft {
            ui.checkbox(&mut draft.enabled, "Enable phone replies");
            let field_width = (ui.available_width() - 176.0).clamp(140.0, 420.0);
            egui::Grid::new("phone-reply-settings")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label("ntfy server");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.server)
                            .hint_text("https://ntfy.example.com")
                            .desired_width(field_width),
                    );
                    ui.end_row();
                    ui.label("Questions topic");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.questions).desired_width(field_width),
                    );
                    ui.end_row();
                    ui.label("Replies topic");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.replies).desired_width(field_width),
                    );
                    ui.end_row();
                    ui.label("Desk account token");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.phone_daemon_token)
                            .password(true)
                            .desired_width(field_width)
                            .hint_text("Blank keeps the saved token"),
                    );
                    ui.end_row();
                    ui.label("Phone publisher token");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.phone_reply_token)
                            .password(true)
                            .desired_width(field_width)
                            .hint_text("Blank keeps the saved token"),
                    );
                    ui.end_row();
                });

            ui.horizontal_wrapped(|ui| {
                if ui.add(theme::primary("Save phone settings")).clicked() {
                    update = Some(Update {
                        settings: draft.clone(),
                        daemon_token: (!self.phone_daemon_token.is_empty())
                            .then(|| Secret(self.phone_daemon_token.clone())),
                        reply_token: (!self.phone_reply_token.is_empty())
                            .then(|| Secret(self.phone_reply_token.clone())),
                        forget_credentials: false,
                    });
                }
                ui.menu_button("Other actions", |ui| {
                if ui.button("Disable and forget credentials").clicked() {
                    update = Some(Update {
                        settings: draft.clone(),
                        forget_credentials: true,
                        ..Default::default()
                    });
                    ui.close();
                }
                });
            });
        } else {
            ui.weak("Loading phone settings...");
        }
        });
        if let Some(update) = update {
            self.send(Req::SetPhone(update));
        }
        egui::CollapsingHeader::new("Connection details")
            .id_salt("phone-connection-details").show_accessible(ui, |ui| {
        if let Some(status) = &self.phone {
            ui.weak(format!(
                "Protection checked: {} · Last reply check: {} · Last question sent: {}",
                time_label(status.health.protected_at),
                time_label(status.health.polled_at),
                time_label(status.health.published_at)
            ));
        }
        if ui.small_button("Refresh phone status").clicked() {
            self.send(Req::Phone);
        }
        ui.weak("A phone tap queues a reply. Wait for the follow-up confirmation that the Desk recorded it. Replies can wait while this computer is offline; delivery also depends on your ntfy server's retention period.");
        });
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        theme::page_heading(
            ui,
            "Settings",
            "Manage notifications, phone replies and the agents allowed to reach you.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.settings_section,
                SettingsSection::Notifications,
                "Notifications",
            );
            ui.selectable_value(
                &mut self.settings_section,
                SettingsSection::Phone,
                "Phone replies",
            );
            let pending = self
                .owners
                .iter()
                .filter(|owner| owner.requested_owner.is_some() || owner.binding == "pending")
                .count();
            ui.selectable_value(
                &mut self.settings_section,
                SettingsSection::Owners,
                if pending == 0 {
                    "Owners".into()
                } else {
                    format!("Owners · {pending} to review")
                },
            );
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt(("settings-page", self.settings_section)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(880.0));
            match self.settings_section {
                SettingsSection::Notifications => {
                    theme::surface(ui).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.strong("Desktop notifications");
                        ui.checkbox(&mut self.settings_draft.toasts_off, "Pause desktop notifications");
                        ui.weak("Questions still arrive in your Inbox while notifications are paused.");
                        ui.add_space(12.0);
                        egui::CollapsingHeader::new(if self.settings_draft.ntfy.trim().is_empty() { "Phone summary · off" } else { "Phone summary · configured" })
                            .id_salt("phone-summary-settings").show_accessible(ui, |ui| {
                        ui.weak("An optional count of waiting decisions and the nearest deadline. Question text is never included.");
                        ui.label("ntfy topic URL");
                        ui.add(egui::TextEdit::singleline(&mut self.settings_draft.ntfy)
                            .hint_text("https://ntfy.sh/your-secret-topic")
                            .desired_width(f32::INFINITY));
                        ui.weak("Leave blank to turn the summary off. Use Phone replies for questions with answer buttons.");
                        });
                        let ntfy = self.settings_draft.ntfy.trim().to_string();
                        let valid = ntfy.is_empty() || matches!(view::classify(&ntfy), Link::Web(_));
                        if !valid { ui.colored_label(ui.visuals().error_fg_color, "Enter an https:// topic URL."); }
                        let changed = self.settings.as_ref().is_some_and(|saved| saved.toasts_off != self.settings_draft.toasts_off || saved.ntfy_topic_url.as_deref().unwrap_or_default() != ntfy);
                        ui.add_space(8.0);
                        if changed && ui.add_enabled(valid, theme::primary("Save notifications")).clicked() {
                            self.send(Req::SetSettings(Settings {
                                ntfy_topic_url: (!ntfy.is_empty()).then_some(ntfy),
                                toasts_off: self.settings_draft.toasts_off,
                            }));
                        }
                        if self.settings.is_none() { ui.weak("Loading settings…"); }
                    });
                }
                SettingsSection::Phone => { theme::surface(ui).show(ui, |ui| { self.phone_settings(ui); }); }
                SettingsSection::Owners => self.owners_settings(ui),
            }
        });
    }

    fn owners_settings(&mut self, ui: &mut egui::Ui) {
        ui.weak("Owners are stable names for a routine, project or client. Manage their permissions and notifications here.");
        if self
            .owners_directory
            .controls(ui, "owners", self.owners_inflight)
        {
            self.last_owners = None;
        }
        if self.owners.is_empty() {
            ui.weak("No matching owners.");
            return;
        }
        let mut changed = None;
        let mut resolve = None;
        let mut owners: Vec<_> = self.owners.iter().collect();
        owners
            .sort_by_key(|owner| !(owner.requested_owner.is_some() || owner.binding == "pending"));
        for o in owners {
            ui.push_id(&o.name, |ui| {
                theme::surface(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(&o.name).strong()).wrap());
                    if o.binding == "pending" || o.requested_owner.is_some() || o.muted || o.relay || o.codex_interactive {
                    ui.horizontal_wrapped(|ui| {
                        if o.binding == "pending" || o.requested_owner.is_some() {
                            ui.colored_label(ui.visuals().warn_fg_color, "Needs your review");
                        }
                        if o.muted { ui.weak("Notifications muted"); }
                        if o.relay { ui.weak("Can relay other owners’ answers"); }
                        if o.codex_interactive { ui.weak("Chat resumes after answers"); }
                    });
                    }
                    if let Some(proposed) = &o.requested_owner {
                        ui.label(format!("Proposed name: {proposed}"));
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Approve name").on_hover_text("Keep this session's asks and settings under this name. No access to another owner's answers.").clicked() {
                                resolve = Some(Req::ResolveOwner { owner: o.name.clone(), requested_owner: proposed.clone(), approve: true });
                            }
                            if ui.button("Reject name").clicked() {
                                resolve = Some(Req::ResolveOwner { owner: o.name.clone(), requested_owner: proposed.clone(), approve: false });
                            }
                        });
                    } else if o.binding == "pending" && ui.button("Approve owner").clicked() {
                        changed = Some(Owner { binding: "approved".into(), ..o.clone() });
                    }
                    egui::CollapsingHeader::new("Permissions and notifications")
                        .id_salt(("owner-settings", &o.name)).show_accessible(ui, |ui| {
                    ui.weak(format!("Binding: {} · {} questions marked not needed", o.binding, o.not_needed_count));
                    let mut codex = o.codex_interactive;
                    if ui.checkbox(&mut codex, "Resume this owner’s Codex chat after an answer")
                        .on_hover_text("Answers start this owner's Codex thread's next turn").changed() {
                        changed = Some(Owner { codex_interactive: codex, ..o.clone() });
                    }
                    let mut relay = o.relay;
                    if ui.checkbox(&mut relay, "Allow relaying answers for other owners")
                        .on_hover_text("May list and deliver other owners' answers").changed() {
                        changed = Some(Owner { relay, ..o.clone() });
                    }
                    ui.horizontal_wrapped(|ui| {
                        if let Some(req) = mute_control(ui, &o.name, o.muted) { resolve = Some(req); }
                        if let Some(until) = &o.muted_until { ui.weak(format!("Muted until {until}")); }
                    });
                    });
                });
                ui.add_space(4.0);
            });
        }
        if let Some(o) = changed {
            self.send(Req::SetOwner(o));
        }
        if let Some(req) = resolve {
            self.send(req);
        }
    }
}

fn priority_tag(ui: &mut egui::Ui, p: Priority) {
    let (text, color) = match p {
        Priority::P0 => ("P0", ui.visuals().error_fg_color),
        Priority::P1 => ("P1", ui.visuals().warn_fg_color),
        Priority::P2 => ("P2", ui.visuals().weak_text_color()),
    };
    ui.label(RichText::new(text).monospace().strong().color(color));
}

fn mute_control(ui: &mut egui::Ui, owner: &str, muted: bool) -> Option<Req> {
    let mut request = None;
    ui.horizontal(|ui| {
        if muted && ui.button("Unmute").clicked() {
            request = Some(Req::SetMuted {
                owner: owner.into(),
                muted: false,
            });
        }
        ui.menu_button(
            if muted {
                "Change mute"
            } else {
                "Mute this agent"
            },
            |ui| {
                for (label, seconds) in [
                    ("For 1 hour", 3600),
                    ("For 4 hours", 14400),
                    ("For 1 day", 86400),
                ] {
                    if ui.button(label).clicked() {
                        request = Some(Req::MuteFor {
                            owner: owner.into(),
                            seconds,
                        });
                        ui.close();
                    }
                }
                if ui.button("Until I unmute").clicked() {
                    request = Some(Req::SetMuted {
                        owner: owner.into(),
                        muted: true,
                    });
                    ui.close();
                }
                ui.weak("Questions remain in the Inbox, silently, at the bottom.");
            },
        );
    });
    request
}

/// Honor native Expand/Collapse requests against the actual header ID. CollapsingHeader
/// creates an internal child UI, so its response is the authority for that ID.
trait AccessibleCollapsingHeader {
    fn show_accessible<R>(
        self,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::collapsing_header::CollapsingResponse<R>;
}

impl AccessibleCollapsingHeader for egui::CollapsingHeader {
    fn show_accessible<R>(
        self,
        ui: &mut egui::Ui,
        body: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::collapsing_header::CollapsingResponse<R> {
        let mut response = self.show(ui, body);
        let id = response.header_response.id;
        let mut open = None;
        if response.header_response.enabled() {
            ui.input_mut(|input| {
                input.consume_accesskit_action_requests(id, |request| {
                    open = match request.action {
                        egui::accesskit::Action::Expand => Some(true),
                        egui::accesskit::Action::Collapse => Some(false),
                        _ => return false,
                    };
                    true
                });
            });
        }
        if let Some(open) = open
            && let Some(mut state) = egui::collapsing_header::CollapsingState::load(ui.ctx(), id)
            && state.is_open() != open
        {
            state.set_open(open);
            state.store(ui.ctx());
            response.header_response.mark_changed();
            ui.request_repaint();
        }
        response
    }
}

fn chat_button(
    ui: &mut egui::Ui,
    client: &str,
    session: Option<&str>,
    machine: Option<&str>,
    cwd: Option<&str>,
) -> Option<agentbolt_core::navigation::ChatTarget> {
    if machine.is_none_or(str::is_empty) {
        ui.add_enabled(false, egui::Button::new("Open chat"))
            .on_hover_text("The source computer was not recorded. Copy the session reference and open it on the originating computer.");
        if let Some(session) = session
            && ui.small_button("Copy session reference").clicked()
        {
            ui.ctx().copy_text(session.to_owned());
        }
        return None;
    }
    if let Some(machine) = machine.and_then(|m| m.strip_prefix("remote:")) {
        ui.add_enabled(false,egui::Button::new(format!("Open chat on {machine}"))).on_hover_text("This chat belongs to another computer. Its session and working folder will not be opened locally.");
        if let Some(session) = session
            && ui.small_button("Copy remote session").clicked()
        {
            ui.ctx().copy_text(session.to_owned());
        }
        return None;
    }
    let target = agentbolt_core::navigation::ChatTarget::from_host(client, session);
    let label = target.as_ref().map_or("Open chat", |t| t.label());
    let open = ui.add_enabled(target.is_some(), egui::Button::new(label))
        .on_disabled_hover_text("This host did not provide a supported session id. Older asks may lack their original client details.")
        .clicked();
    if let Some(command) = target.as_ref().and_then(|target| target.resume_command())
        && ui.small_button("Copy resume command")
            .on_hover_text(format!("Paste into a terminal opened in this chat's original folder. Useful when automatic navigation is unavailable or Windows Terminal asks for administrator consent.\nRecorded folder: {}", cwd.unwrap_or("not provided by this host")))
            .clicked()
    {
        ui.ctx().copy_text(command);
    }
    open.then_some(target).flatten()
}

fn delivery_rows(ui: &mut egui::Ui, records: &[agentbolt_core::model::Delivery]) {
    for delivery in records {
        let status = if delivery.confirmed {
            "Delivery confirmed"
        } else if delivery.ok {
            "Accepted; receipt unconfirmed"
        } else {
            "Delivery failed"
        };
        ui.label(format!("{status} · {} · {}", delivery.to, delivery.at));
        if let Some(detail) = &delivery.detail {
            ui.add(egui::Label::new(detail).selectable(true).wrap());
        }
    }
}

fn evidence_row(
    ui: &mut egui::Ui,
    label: &str,
    url: &str,
    exists: &HashMap<std::path::PathBuf, bool>,
    machine: Option<&str>,
) {
    ui.horizontal_wrapped(|ui| match view::classify_evidence(url, machine) {
        Link::Web(u) | Link::Session(u) => {
            if ui.link(label).on_hover_text(&u).clicked()
                && let Err(e) = os::open_url(&u)
            {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        }
        Link::Path(p) => {
            ui.label(format!("{label}:"));
            ui.add(
                egui::Label::new(RichText::new(p.display().to_string()).monospace())
                    .selectable(true),
            );
            // Board references are checked on click; selected ask paths have a cached existence result.
            let exists = exists.get(&p).copied().unwrap_or(true);
            if ui
                .add_enabled(exists, egui::Button::new("Show in Explorer").small())
                .on_disabled_hover_text("Not found on this PC")
                .clicked()
                && let Err(error) = os::show_in_explorer(&p)
            {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
        Link::Text => {
            ui.label(format!("{label}:"));
            ui.add(egui::Label::new(RichText::new(url).monospace()).selectable(true));
            if ui.small_button("Copy reference").clicked() {
                ui.ctx().copy_text(url.to_owned());
            }
        }
    });
}

fn form_controls(ui: &mut egui::Ui, ask: &Ask, draft: &mut Draft) -> Result<Value, String> {
    let mut edited = false;
    let inputs = draft
        .fields
        .get_or_insert_with(|| view::prefill(&ask.input.fields, &ask.input.default));
    ui.push_id(("form", &ask.id), |ui| {
        for f in &ask.input.fields {
            let label = if f.required {
                format!("{} *", f.label)
            } else {
                f.label.clone()
            };
            let label_response = ui.strong(&label);
            let input = inputs
                .entry(f.id.clone())
                .or_insert_with(|| FieldInput::Text(String::new()));
            match (f.field_type.as_str(), input) {
                ("boolean", FieldInput::Bool(b)) => {
                    edited |= ui.checkbox(b, &f.label).changed();
                }
                ("choice", FieldInput::Text(t)) => {
                    egui::ComboBox::from_id_salt(("field", &ask.id, &f.id))
                        .selected_text(t.clone())
                        .show_ui(ui, |ui| {
                            for o in f.options.iter().flatten() {
                                edited |= ui.selectable_value(t, o.clone(), o).changed();
                            }
                        });
                }
                (_, FieldInput::Text(t)) => {
                    edited |= ui
                        .add(egui::TextEdit::singleline(t).desired_width(f32::INFINITY))
                        .labelled_by(label_response.id)
                        .changed();
                }
                (_, FieldInput::Bool(b)) => {
                    edited |= ui.checkbox(b, &f.label).changed();
                }
                (_, FieldInput::OptionalBool(value)) => {
                    egui::ComboBox::from_id_salt(("optional-bool", &ask.id, &f.id))
                        .selected_text(match value {
                            None => "Not answered",
                            Some(true) => "Yes",
                            Some(false) => "No",
                        })
                        .show_ui(ui, |ui| {
                            edited |= ui.selectable_value(value, None, "Not answered").changed();
                            edited |= ui.selectable_value(value, Some(true), "Yes").changed();
                            edited |= ui.selectable_value(value, Some(false), "No").changed();
                        });
                }
            }
            ui.add_space(4.0);
        }
    });
    let result = view::form_values(&ask.input.fields, inputs);
    if edited {
        draft.edited();
    }
    result
}

/// The buttons, text box or form for one ask. Returns the answer to send (note added by the caller).
fn answer_controls(ui: &mut egui::Ui, ask: &Ask, draft: &mut Draft) -> Option<Req> {
    let answer = |choice: Option<String>, values: Option<Value>| Req::Answer {
        ask_id: ask.id.clone(),
        choice,
        values,
        note: None,
        one_click: None, // Filled from the retained draft's interaction history by details().
    };
    match ask.input.kind {
        AskKind::Choose | AskKind::Approve => {
            let mut out = None;
            ui.horizontal_wrapped(|ui| {
                for opt in view::options_for(ask) {
                    let label = view::option_label(ask, &opt);
                    let mut b = egui::Button::new(RichText::new(label).size(15.0))
                        .wrap()
                        .min_size(egui::vec2(110.0, 36.0));
                    if view::is_default(ask, &opt) {
                        b = b.stroke(ui.visuals().selection.stroke);
                    }
                    if ui.add(b).clicked() {
                        out = Some(answer(Some(opt.clone()), None));
                    }
                }
            });
            out
        }
        AskKind::Inform => ui
            .add(theme::primary("Got it"))
            .clicked()
            .then(|| answer(None, None)),
        AskKind::Text => {
            view::fill_reply_once(
                &mut draft.text,
                &mut draft.text_filled,
                ask.input.default.as_str(),
            );
            ui.label("Your reply");
            if ui
                .add(
                    egui::TextEdit::multiline(&mut draft.text)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                draft.edited();
            }
            if let Some(d) = ask.input.default.as_str() {
                ui.weak(format!("Recommended: {d}"));
            }
            let ok = !draft.text.trim().is_empty();
            ui.add_enabled(ok, theme::primary("Send answer"))
                .clicked()
                .then(|| answer(Some(draft.text.trim().to_string()), None))
        }
        AskKind::Form => {
            let values = form_controls(ui, ask, draft);
            if let Err(e) = &values {
                ui.colored_label(ui.visuals().warn_fg_color, e);
            }
            let clicked = ui
                .add_enabled(values.is_ok(), theme::primary("Send answer"))
                .clicked();
            match values {
                Ok(v) if clicked => Some(answer(None, Some(v))),
                _ => None,
            }
        }
    }
}

impl eframe::App for DeskApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let gate = self.upgrade.clone();
        let mut upgrade = gate.lock().unwrap();
        if upgrade.paused() {
            ctx.request_repaint_after(Duration::from_secs(1));
            return;
        }
        self.drain();
        self.poll(ctx);
        // Draining an already-sent answer may clear a draft before the next UI frame.
        upgrade.draft_error = self.save_drafts().err();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let gate = self.upgrade.clone();
        let mut upgrade = gate.lock().unwrap();
        if upgrade.paused() {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("Agentbolt is updating. Your unsent drafts are saved.");
            });
            ui.ctx().request_repaint_after(Duration::from_secs(1));
            return;
        }
        crate::startup::mark("first_ui");
        egui::Panel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| {
                ui.add_space(4.0);
                self.top_bar(ui);
                if let Some(warning) = &self.notification_warning {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Desktop notifications need attention. You can still answer in the Desk.",
                    )
                    .on_hover_text(warning);
                }
                if let Some(error) = &self.draft_error {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Unsent drafts are not being saved. {error}"),
                    );
                }
                ui.add_space(2.0);
            });
        if let Some(n) = &self.notice
            && n.at.elapsed() > Duration::from_secs(if n.error { 20 } else { 6 })
        {
            self.notice = None;
        }
        let down = match &self.conn {
            Conn::Down(e) => Some(e.clone()),
            _ => None,
        };
        if self.notice.is_some() || down.is_some() {
            egui::Panel::bottom("notice").show(ui, |ui| {
                if let Some(e) = &down {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                if let Some(n) = &self.notice {
                    let c = if n.error {
                        ui.visuals().error_fg_color
                    } else {
                        Color32::from_rgb(0x3c, 0x9a, 0x5f)
                    };
                    ui.colored_label(c, &n.text);
                }
            });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(16),
            )
            .show(ui, |ui| match self.tab {
                Tab::Inbox => self.ask_pane(ui),
                Tab::History => self.history_tab(ui),
                Tab::Agents => self.agents_tab(ui),
                Tab::Board => self.board_tab(ui),
                Tab::Activity => self.activity_tab(ui),
                Tab::Holds => self.holds_tab(ui),
                Tab::Settings => self.settings_tab(ui),
            });
        if self
            .draft_retry
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            match self.save_drafts() {
                Ok(()) => {
                    self.draft_error = None;
                    self.draft_retry = None;
                }
                Err(error) => {
                    self.draft_error = Some(error);
                    self.draft_retry = Some(Instant::now());
                }
            }
        }
        if crate::startup::enabled() {
            // Runs after egui has built the font atlas and submitted the UI shapes to the painter.
            ui.painter().add(egui::PaintCallback {
                rect: ui.max_rect(),
                callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(|_, _| {
                    crate::startup::mark("first_frame_painted");
                })),
            });
            crate::startup::mark("first_ui_complete");
        }
        upgrade.draft_error = self.draft_error.clone();
    }
}

#[cfg(test)]
mod accessibility_tests {
    use super::{AccessibleCollapsingHeader, egui};
    use egui::{Context, Event, Id, RawInput, accesskit};

    fn frame(ctx: &Context, event: Option<Event>, enabled: bool) -> [(Id, bool); 2] {
        let mut headers = [(Id::NULL, false); 2];
        let mut output = ctx.run_ui(
            RawInput {
                events: event.into_iter().collect(),
                ..Default::default()
            },
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, slot) in headers.iter_mut().enumerate() {
                        let result = egui::CollapsingHeader::new("Same title")
                            .id_salt(index)
                            .enabled(enabled)
                            .show_accessible(ui, |ui| ui.label("Details"));
                        let id = result.header_response.id;
                        let state =
                            egui::collapsing_header::CollapsingState::load(ctx, id).unwrap();
                        *slot = (id, state.is_open());
                    }
                });
            },
        );
        output.textures_delta.clear();
        headers
    }

    #[test]
    fn native_expand_collapse_targets_the_actual_header_and_respects_disabled_state() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let initial = frame(&ctx, None, true);
        assert!(!initial[0].1 && !initial[1].1);
        assert_ne!(initial[0].0, initial[1].0);
        let request = |action| {
            Some(Event::AccessKitActionRequest(accesskit::ActionRequest {
                action,
                target_tree: accesskit::TreeId::ROOT,
                target_node: initial[1].0.accesskit_id(),
                data: None,
            }))
        };
        for _ in 0..2 {
            let expanded = frame(&ctx, request(accesskit::Action::Expand), true);
            assert!(!expanded[0].1 && expanded[1].1);
        }
        assert!(frame(&ctx, None, true)[1].1, "expansion persists");
        let disabled = frame(&ctx, request(accesskit::Action::Collapse), false);
        assert!(disabled[1].1, "disabled controls ignore actions");
        for _ in 0..2 {
            let collapsed = frame(&ctx, request(accesskit::Action::Collapse), true);
            assert!(!collapsed[0].1 && !collapsed[1].1);
        }
    }
}
