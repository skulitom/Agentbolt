//! The Desk window (eframe/egui): Inbox, Agents and Settings. It follows the system's light or dark
//! theme, polls the daemon every 1.5 s while it's open, and exits when closed. It lists the Inbox (which marks
//! asks shown) only while the window is focused and not minimized; otherwise it refreshes the counts only.
//!
//! Ask text is untrusted: bodies are plain selectable text, never HTML or markdown, and links pass
//! `view::classify` before anything opens.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use agentbolt_core::channel::Names;
use agentbolt_core::model::{Agent, AnswerVia, Ask, AskKind, Owner, Priority, Settings};
use agentbolt_core::protocol::{DeskAnswer, MAX_BATCH_ANSWERS};
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;

use crate::drafts::{Draft, Snapshot as DraftSnapshot, Store as DraftStore};
use crate::os;
use crate::view::{self, FieldInput, Link};
use crate::worker::{self, Req, Resp, Worker};

const POLL: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Inbox,
    Agents,
    Settings,
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

pub struct DeskApp {
    names: Names,
    worker: Worker,
    tab: Tab,
    inbox: Vec<Ask>,
    /// Open asks by the last count (a List, or a Status while the window wasn't seen).
    open_count: Option<u32>,
    recent: Vec<Ask>,
    selected_inbox: Option<String>,
    batch_selection: HashSet<String>,
    review_batch: bool,
    drafts: HashMap<String, Draft>,
    draft_store: DraftStore,
    draft_error: Option<String>,
    draft_retry: Option<Instant>,
    sending: HashSet<String>,
    agents: Vec<Agent>,
    owners: Vec<Owner>,
    more_owners: u32,
    settings: Option<Settings>,
    settings_draft: SettingsDraft,
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
    let title = if names.is_main() {
        "Agentbolt".to_string()
    } else {
        format!("Agentbolt ({})", names.channel)
    };
    let Some(_instance) =
        crate::os::first_window(&names, &title, !warm).map_err(anyhow::Error::msg)?
    else {
        return Ok(());
    };
    crate::os::trace_window_shown(&title);
    let data = names.data_dir().map_err(anyhow::Error::msg)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_active(!warm)
            .with_inner_size([900.0, 620.0])
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
            crate::resident::prepare(cc, warm)?;
            crate::startup::mark("renderer_ready");
            let ctx = cc.egui_ctx.clone();
            let worker = worker::spawn(names.clone(), move || ctx.request_repaint());
            Ok(Box::new(DeskApp::new(names, worker)))
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
            names,
            worker,
            tab: Tab::Inbox,
            inbox: Vec::new(),
            open_count: None,
            recent: Vec::new(),
            selected_inbox: state.selected,
            batch_selection: state.batch,
            review_batch: false,
            drafts: state.drafts,
            draft_store,
            draft_error,
            draft_retry: None,
            sending: HashSet::new(),
            agents: Vec::new(),
            owners: Vec::new(),
            more_owners: 0,
            settings: None,
            settings_draft: SettingsDraft::default(),
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
                Resp::List(mut r) => {
                    self.notification_warning = r.notification_warning.take();
                    crate::startup::mark("inbox_received");
                    crate::startup::record("inbox_refreshed");
                    self.list_inflight = false;
                    self.conn = Conn::Up;
                    view::sort_asks(&mut r.inbox);
                    self.inbox = r.inbox;
                    self.open_count = Some(self.inbox.len() as u32);
                    self.recent = r.recent;
                    let ids: HashSet<&str> = self.inbox.iter().map(|a| a.id.as_str()).collect();
                    self.batch_selection.retain(|id| ids.contains(id.as_str()));
                    self.drafts.retain(|id, _| ids.contains(id.as_str()));
                    if self
                        .selected_inbox
                        .as_deref()
                        .is_none_or(|s| !ids.contains(s))
                    {
                        self.selected_inbox = self.inbox.first().map(|a| a.id.clone());
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
                Resp::Agents(r) => {
                    self.agents_inflight = false;
                    self.agents = r.agents;
                }
                Resp::Owners(r) => {
                    self.owners_inflight = false;
                    self.owners = r.owners;
                    self.more_owners = r.more_provisional;
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
        let kind = view::poll_kind(focused && crate::resident::visible(), minimized);
        let seen = kind == view::Poll::List;
        if seen && !self.was_seen {
            self.last_list = None;
        }
        self.was_seen = seen;
        if !self.list_inflight && self.last_list.is_none_or(|t| t.elapsed() >= POLL) {
            self.list_inflight = true;
            self.last_list = Some(Instant::now());
            self.send(match kind {
                view::Poll::List => Req::List,
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
            self.send(Req::Agents);
        }
        if seen
            && !self.owners_inflight
            && self
                .last_owners
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
        {
            self.owners_inflight = true;
            self.last_owners = Some(Instant::now());
            self.send(Req::Owners);
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
            Tab::Settings => {
                self.send(Req::GetSettings);
                self.last_owners = None;
            }
            Tab::Agents => self.last_agents = None,
            _ => {}
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let tabs = [
                (
                    Tab::Inbox,
                    format!(
                        "Inbox ({})",
                        self.open_count.unwrap_or(self.inbox.len() as u32)
                    ),
                ),
                (Tab::Agents, "Agents".to_string()),
                (Tab::Settings, {
                    let pending = self
                        .owners
                        .iter()
                        .filter(|o| o.requested_owner.is_some())
                        .count();
                    if pending == 0 {
                        "Settings".into()
                    } else {
                        format!("Settings ({pending} names to review)")
                    }
                }),
            ];
            for (tab, label) in tabs {
                if ui
                    .selectable_label(self.tab == tab, RichText::new(label).size(15.0))
                    .clicked()
                {
                    self.switch_to(tab);
                }
            }
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match &self.conn {
                    Conn::Up => {
                        ui.weak(format!("connected, channel {}", self.names.channel));
                    }
                    Conn::Unknown => {
                        ui.weak("connecting...");
                    }
                    Conn::Down(_) => {
                        if ui.button("Start Agentbolt").clicked() {
                            self.send(Req::StartDaemon);
                        }
                        ui.colored_label(ui.visuals().error_fg_color, "Agentbolt isn't running");
                    }
                },
            );
        });
    }

    fn ask_list(&mut self, ui: &mut egui::Ui) {
        if !self.batch_selection.is_empty() {
            if ui
                .button(format!("Review {} answers", self.batch_selection.len()))
                .clicked()
            {
                self.review_batch = true;
            }
            if ui.small_button("Clear selection").clicked() {
                self.batch_selection.clear();
                self.review_batch = false;
            }
        }
        let asks = self.inbox.clone();
        let now = time::OffsetDateTime::now_utc();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for a in &asks {
                    let selected = &mut self.selected_inbox;
                    let is_sel = selected.as_deref() == Some(a.id.as_str());
                    let (due, overdue) = view::relative_deadline(&a.input.deadline, now);
                    let frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 4));
                    let resp = frame
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut checked = self.batch_selection.contains(&a.id);
                                let allowed =
                                    checked || self.batch_selection.len() < MAX_BATCH_ANSWERS;
                                if ui
                                    .add_enabled(
                                        allowed,
                                        egui::Checkbox::new(&mut checked, "Select"),
                                    )
                                    .changed()
                                {
                                    if checked {
                                        self.batch_selection.insert(a.id.clone());
                                    } else {
                                        self.batch_selection.remove(&a.id);
                                    }
                                }
                                priority_tag(ui, a.input.priority);
                                ui.add(
                                    egui::Label::new(RichText::new(&a.input.title).strong())
                                        .truncate(),
                                );
                            });
                            ui.horizontal(|ui| {
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
                    }
                    ui.separator();
                }
                if !self.recent.is_empty() {
                    egui::CollapsingHeader::new(format!(
                        "Recently answered ({})",
                        self.recent.len()
                    ))
                    .default_open(false)
                    .show(ui, |ui| {
                        for a in &self.recent {
                            let said = a
                                .answer
                                .as_ref()
                                .map(|x| {
                                    if x.via == AnswerVia::NotNeeded {
                                        return "Not needed".to_string();
                                    }
                                    x.choice
                                        .clone()
                                        .or_else(|| x.values.as_ref().map(Value::to_string))
                                        .unwrap_or_default()
                                })
                                .unwrap_or_default();
                            ui.label(format!("{}: {said}", a.input.title));
                        }
                    });
                }
            });
    }

    /// The selected ask's details and its answer controls. Returns an answer to send.
    fn details(&mut self, ui: &mut egui::Ui, ask: &Ask) -> Option<Req> {
        let now = time::OffsetDateTime::now_utc();
        let mut req = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("details", &ask.id)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&ask.input.title).heading().strong()).wrap());
            ui.horizontal_wrapped(|ui| {
                priority_tag(ui, ask.input.priority);
                ui.label(format!("from {}", ask.owner));
                let (due, overdue) = view::relative_deadline(&ask.input.deadline, now);
                let c = if overdue { ui.visuals().error_fg_color } else { ui.visuals().text_color() };
                ui.colored_label(c, due).on_hover_text(&ask.input.deadline);
                let tag = view::reversible_tag(ask);
                let c = if ask.input.reversible { ui.visuals().weak_text_color() } else { ui.visuals().warn_fg_color };
                ui.colored_label(c, tag);
            });
            if let Some(why) = &ask.input.p0_reason {
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
                ui.strong("Evidence");
                let exists = self.paths.for_ask(ask, |p| p.exists());
                for e in &ask.input.evidence {
                    evidence_row(ui, &e.label, &e.url, exists);
                }
            }
            ui.add_space(4.0);
            if let Some(target) = chat_button(ui, ask.asking_client.as_deref().unwrap_or_default(), ask.asking_session.as_deref())
                && let Err(e) = os::open_chat(&target, ask.asking_cwd.as_deref()) {
                self.note(e, true);
            }
            let mute_label = if ask.muted { "Unmute this agent" } else { "Mute this agent" };
            if ui.button(mute_label).on_hover_text("Applies to this owner's sessions until unmuted. Questions stay in the Inbox, silently, at the bottom.").clicked() {
                self.send(Req::SetMuted { owner: ask.owner.clone(), muted: !ask.muted });
            }
            ui.separator();
            let sending = self.sending.contains(&ask.id);
            let draft = self.drafts.entry(ask.id.clone()).or_default();
            ui.add_enabled_ui(!sending, |ui| {
                // The note comes first: an option button answers at once.
                ui.horizontal(|ui| {
                    ui.label("Note");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.note)
                            .hint_text("optional, sent with your answer")
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.add_space(4.0);
                req = answer_controls(ui, ask, draft);
                ui.add_space(6.0);
                if ui.button("Not needed").on_hover_text("Tell the agent this question was unnecessary. No option is approved.").clicked() {
                    req = Some(Req::NotNeeded { ask_id: ask.id.clone(), note: None });
                }
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
        });
        let note = self
            .drafts
            .get(&ask.id)
            .map(|d| d.note.trim().to_string())
            .filter(|n| !n.is_empty());
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
            },
            Req::NotNeeded { ask_id, .. } => Req::NotNeeded { ask_id, note },
            other => other,
        })
    }

    fn ask_pane(&mut self, ui: &mut egui::Ui) {
        let asks = &self.inbox;
        if asks.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(view::EMPTY_INBOX).size(16.0).weak());
            });
            return;
        }
        egui::Panel::left("inbox-list")
            .resizable(true)
            .default_size(320.0)
            .min_size(220.0)
            .show(ui, |ui| self.ask_list(ui));
        let selected = &self.selected_inbox;
        let ask = selected.as_ref().and_then(|id| {
            let list = &self.inbox;
            list.iter().find(|a| &a.id == id).cloned()
        });
        egui::CentralPanel::default().show(ui, |ui| {
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
                    ui.weak("Pick an ask on the left.");
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
                        let draft = self.drafts.entry(ask.id.clone()).or_default();
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
        if self.agents.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(view::EMPTY_AGENTS).size(16.0).weak());
            });
            return;
        }
        let now = time::OffsetDateTime::now_utc();
        let mut open = None;
        let mut mute = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("agents")
                    .striped(true)
                    .num_columns(6)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        for h in [
                            "Owner",
                            "Client",
                            "Doing",
                            "Last seen",
                            "Session",
                            "Notifications",
                        ] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for a in &self.agents {
                            let owner = if a.provisional {
                                format!("{} (provisional)", a.owner)
                            } else {
                                a.owner.clone()
                            };
                            ui.vertical(|ui| {
                                ui.label(owner);
                                if let Some(proposed) = &a.requested_owner {
                                    ui.weak(format!("Proposes: {proposed} (review in Settings)"));
                                }
                            });
                            ui.label(&a.client);
                            let doing = a.doing.as_deref().unwrap_or("");
                            let short: String = doing.chars().take(60).collect();
                            let r = ui.label(if short.len() < doing.len() {
                                format!("{short}...")
                            } else {
                                short
                            });
                            if !doing.is_empty() {
                                r.on_hover_text(doing);
                            }
                            ui.label(view::ago(&a.last_seen, now))
                                .on_hover_text(&a.last_seen);
                            if let Some(target) =
                                chat_button(ui, &a.client, a.session_ref.as_deref())
                            {
                                open = Some((target, a.cwd.clone()));
                            }
                            if ui
                                .button(if a.muted { "Unmute" } else { "Mute this agent" })
                                .clicked()
                            {
                                mute = Some(Req::SetMuted {
                                    owner: a.owner.clone(),
                                    muted: !a.muted,
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(req) = mute {
            self.send(req);
        }
        if let Some((target, cwd)) = open
            && let Err(e) = os::open_chat(&target, cwd.as_deref())
        {
            self.note(e, true);
        }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.heading("Notifications");
            if self.settings.is_none() {
                ui.weak("Loading the settings...");
            }
            let d = &mut self.settings_draft;
            egui::Grid::new("settings").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                ui.label("Phone push (count only)");
                ui.add(egui::TextEdit::singleline(&mut d.ntfy).hint_text("https://ntfy.sh/your-secret-topic").desired_width(360.0));
                ui.end_row();
                ui.label("Notifications");
                ui.checkbox(&mut d.toasts_off, "off");
                ui.end_row();
            });
            ui.weak("The push says only how many decisions wait and the nearest deadline, never what they are.");
            let ntfy = d.ntfy.trim().to_string();
            let ntfy_ok = ntfy.is_empty() || matches!(view::classify(&ntfy), Link::Web(_));
            if !ntfy_ok {
                ui.colored_label(ui.visuals().error_fg_color, "The topic URL must be an https:// link.");
            }
            if ui.add_enabled(self.settings.is_some() && ntfy_ok, egui::Button::new("Save")).clicked() {
                let s = Settings {
                    ntfy_topic_url: (!ntfy.is_empty()).then_some(ntfy),
                    toasts_off: d.toasts_off,
                };
                self.send(Req::SetSettings(s));
            }

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.heading("Owners");
                if ui.small_button("Refresh").clicked() {
                    self.send(Req::Owners);
                }
            });
            ui.weak("An owner is a stable name for a routine, project or client. Answers and open-ask caps attach to it.");
            if self.owners.is_empty() {
                ui.weak("No owners yet.");
                return;
            }
            if self.more_owners > 0 {
                ui.weak(format!(
                    "Showing the 50 most recent provisional owners; {} older ones are hidden (idle ones go after a week).",
                    self.more_owners
                ));
            }
            let mut changed = None;
            let mut resolve = None;
            egui::Grid::new("owners").striped(true).num_columns(7).spacing([16.0, 6.0]).show(ui, |ui| {
                for h in ["Owner", "Bound by", "", "Codex may be woken", "Relay", "Muted", "Not needed"] {
                    ui.strong(h);
                }
                ui.end_row();
                for o in &self.owners {
                    ui.label(&o.name);
                    ui.label(&o.binding);
                    if let Some(proposed) = &o.requested_owner {
                        ui.vertical(|ui| {
                            ui.label(format!("Proposed name: {proposed}"));
                            ui.horizontal(|ui| {
                                if ui.button("Approve name").on_hover_text("Keep this session's asks and settings under this name. No access to another owner's answers.").clicked() {
                                    resolve = Some(Req::ResolveOwner { owner: o.name.clone(), requested_owner: proposed.clone(), approve: true });
                                }
                                if ui.button("Reject name").clicked() {
                                    resolve = Some(Req::ResolveOwner { owner: o.name.clone(), requested_owner: proposed.clone(), approve: false });
                                }
                            });
                        });
                    } else if o.binding == "pending" {
                        if ui.button("Approve").clicked() {
                            changed = Some(Owner { binding: "approved".into(), ..o.clone() });
                        }
                    } else {
                        ui.label("");
                    }
                    let mut codex = o.codex_interactive;
                    if ui.checkbox(&mut codex, "").on_hover_text("Answers start this owner's Codex thread's next turn").changed() {
                        changed = Some(Owner { codex_interactive: codex, ..o.clone() });
                    }
                    let mut relay = o.relay;
                    if ui.checkbox(&mut relay, "").on_hover_text("May list and deliver other owners' answers").changed() {
                        changed = Some(Owner { relay, ..o.clone() });
                    }
                    let mut muted = o.muted;
                    if ui.checkbox(&mut muted, "").on_hover_text("Silent questions at the bottom, until unmuted").changed() {
                        changed = Some(Owner { muted, ..o.clone() });
                    }
                    ui.label(o.not_needed_count.to_string());
                    ui.end_row();
                }
            });
            if let Some(o) = changed {
                self.send(Req::SetOwner(o));
            }
            if let Some(req) = resolve {
                self.send(req);
            }
        });
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

fn chat_button(
    ui: &mut egui::Ui,
    client: &str,
    session: Option<&str>,
) -> Option<agentbolt_core::navigation::ChatTarget> {
    let target = agentbolt_core::navigation::ChatTarget::from_host(client, session);
    let label = target.as_ref().map_or("Open chat", |t| t.label());
    if ui.add_enabled(target.is_some(), egui::Button::new(label))
        .on_disabled_hover_text("This host did not provide a supported session id. Older asks may lack their original client details.")
        .clicked() { target } else { None }
}

fn evidence_row(
    ui: &mut egui::Ui,
    label: &str,
    url: &str,
    exists: &HashMap<std::path::PathBuf, bool>,
) {
    ui.horizontal_wrapped(|ui| match view::classify(url) {
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
            let exists = exists.get(&p).copied().unwrap_or(false);
            if ui
                .add_enabled(exists, egui::Button::new("Show in Explorer").small())
                .on_disabled_hover_text("Not found on this PC")
                .clicked()
            {
                let _ = os::show_in_explorer(&p);
            }
        }
        Link::Text => {
            ui.label(format!("{label}:"));
            ui.add(egui::Label::new(RichText::new(url).monospace()).selectable(true));
        }
    });
}

fn form_controls(ui: &mut egui::Ui, ask: &Ask, draft: &mut Draft) -> Result<Value, String> {
    let inputs = draft
        .fields
        .get_or_insert_with(|| view::prefill(&ask.input.fields, &ask.input.default));
    egui::Grid::new(("form", &ask.id))
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            for f in &ask.input.fields {
                let label = if f.required {
                    format!("{} *", f.label)
                } else {
                    f.label.clone()
                };
                ui.label(label);
                let input = inputs
                    .entry(f.id.clone())
                    .or_insert_with(|| FieldInput::Text(String::new()));
                match (f.field_type.as_str(), input) {
                    ("boolean", FieldInput::Bool(b)) => {
                        ui.checkbox(b, "");
                    }
                    ("choice", FieldInput::Text(t)) => {
                        egui::ComboBox::from_id_salt(("field", &ask.id, &f.id))
                            .selected_text(t.clone())
                            .show_ui(ui, |ui| {
                                for o in f.options.iter().flatten() {
                                    ui.selectable_value(t, o.clone(), o);
                                }
                            });
                    }
                    (_, FieldInput::Text(t)) => {
                        ui.add(egui::TextEdit::singleline(t).desired_width(280.0));
                    }
                    (_, FieldInput::Bool(b)) => {
                        ui.checkbox(b, "");
                    }
                }
                ui.end_row();
            }
        });
    view::form_values(&ask.input.fields, inputs)
}

/// The buttons, text box or form for one ask. Returns the answer to send (note added by the caller).
fn answer_controls(ui: &mut egui::Ui, ask: &Ask, draft: &mut Draft) -> Option<Req> {
    let answer = |choice: Option<String>, values: Option<Value>| Req::Answer {
        ask_id: ask.id.clone(),
        choice,
        values,
        note: None,
    };
    match ask.input.kind {
        AskKind::Choose | AskKind::Approve => {
            let mut out = None;
            ui.horizontal_wrapped(|ui| {
                for opt in view::options_for(ask) {
                    let label = view::option_label(ask, &opt);
                    let mut b = egui::Button::new(RichText::new(label).size(15.0));
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
        AskKind::Inform => ui.button("Got it").clicked().then(|| answer(None, None)),
        AskKind::Text => {
            view::fill_reply_once(
                &mut draft.text,
                &mut draft.text_filled,
                ask.input.default.as_str(),
            );
            ui.label("Your reply");
            ui.add(
                egui::TextEdit::multiline(&mut draft.text)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            if let Some(d) = ask.input.default.as_str() {
                ui.weak(format!("Recommended: {d}"));
            }
            let ok = !draft.text.trim().is_empty();
            ui.add_enabled(ok, egui::Button::new("Send"))
                .clicked()
                .then(|| answer(Some(draft.text.trim().to_string()), None))
        }
        AskKind::Form => {
            let values = form_controls(ui, ask, draft);
            if let Err(e) = &values {
                ui.colored_label(ui.visuals().warn_fg_color, e);
            }
            let clicked = ui
                .add_enabled(values.is_ok(), egui::Button::new("Send"))
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
        self.drain();
        self.poll(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::startup::mark("first_ui");
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            if let Some(warning) = &self.notification_warning {
                ui.colored_label(ui.visuals().warn_fg_color, warning);
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
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Inbox => self.ask_pane(ui),
            Tab::Agents => self.agents_tab(ui),
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
    }
}
