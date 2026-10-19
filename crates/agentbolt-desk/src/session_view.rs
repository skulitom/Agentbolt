//! The session switcher: running work first; idle and recent chats stay collapsed.

use crate::app::AccessibleCollapsingHeader;
use crate::sessions::{Session, Snapshot, Worker};
use crate::{theme, view};
use agentbolt_core::channel::Names;
use agentbolt_core::navigation::ChatTarget;
use eframe::egui::{self, RichText};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
enum AppFilter {
    #[default]
    All,
    Codex,
    Claude,
}

pub struct Page {
    worker: Worker,
    snapshot: Snapshot,
    last: Option<Instant>,
    busy: bool,
    search: String,
    app: AppFilter,
    background: bool,
    recent_open: Option<bool>,
    other_open: Option<bool>,
}

impl Page {
    pub fn new(names: &Names, ctx: egui::Context) -> Self {
        Self {
            worker: Worker::spawn(names, move || ctx.request_repaint()),
            snapshot: Snapshot::default(),
            last: None,
            busy: false,
            search: String::new(),
            app: AppFilter::All,
            background: false,
            recent_open: None,
            other_open: None,
        }
    }

    pub fn enter(&mut self) {
        self.last = None;
        self.recent_open = None;
        self.other_open = None;
    }

    pub fn poll(&mut self, visible: bool) {
        if let Some(snapshot) = self.worker.latest() {
            self.snapshot = snapshot;
            self.busy = false;
            self.last = Some(Instant::now());
        }
        if visible
            && !self.busy
            && self
                .last
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.busy = self.worker.refresh();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<(ChatTarget, String)> {
        theme::page_heading(
            ui,
            "Sessions",
            "Find a chat by name or project, then jump back into it.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search sessions or projects")
                    .desired_width(ui.available_width().min(360.0)),
            );
            if !self.search.is_empty() && ui.button("Clear").clicked() {
                self.search.clear();
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Refresh"))
                .clicked()
            {
                self.last = None;
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (filter, label) in [
                (AppFilter::All, "All apps"),
                (AppFilter::Codex, "Codex"),
                (AppFilter::Claude, "Claude"),
            ] {
                if theme::nav_button(ui, ("session-app", filter), label, self.app == filter)
                    .clicked()
                {
                    self.app = filter;
                }
            }
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.background, "Include background agents");
            });
        });
        ui.add_space(8.0);
        let query = self.search.trim().to_lowercase();
        let items: Vec<_> = self
            .snapshot
            .items
            .iter()
            .filter(|item| {
                (self.background || !item.background)
                    && item.matches(&query)
                    && match self.app {
                        AppFilter::All => true,
                        AppFilter::Codex => matches!(item.target, ChatTarget::Codex(_)),
                        AppFilter::Claude => !matches!(item.target, ChatTarget::Codex(_)),
                    }
            })
            .collect();
        let running: Vec<_> = items.iter().copied().filter(|s| s.running()).collect();
        let other: Vec<_> = items
            .iter()
            .copied()
            .filter(|s| s.open && !s.running())
            .collect();
        let recent: Vec<_> = items.iter().copied().filter(|s| !s.open).collect();
        let animate = ui.style().animation_time > 0.0
            && self
                .last
                .is_some_and(|t| t.elapsed() < Duration::from_secs(10))
            && ui.input(|i| {
                i.viewport().focused.unwrap_or(i.focused)
                    && !i.viewport().minimized.unwrap_or(false)
            });
        let mut chosen = None;
        egui::ScrollArea::vertical().id_salt("session-list").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("Running now ({})", running.len()));
                if self.busy { ui.spinner(); }
            });
            ui.add_space(4.0);
            if running.is_empty() && !self.busy {
                ui.weak(if query.is_empty() { "No sessions are currently reporting active work." } else { "No running sessions match your search." });
            }
            for item in &running {
                if let Some(target) = session_card(ui, item, animate) { chosen = Some(target); }
            }
            if !other.is_empty() {
                ui.add_space(12.0);
                let header = egui::CollapsingHeader::new(format!("Other open chats ({})", other.len()))
                    .id_salt("other-open-sessions")
                    .open(if self.other_open.is_none() { Some(false) } else { None })
                    .show_accessible(ui, |ui| {
                        ui.weak("Idle, waiting, or without a confirmed running status.");
                        for item in &other {
                            if let Some(target) = session_card(ui, item, false) { chosen = Some(target); }
                        }
                    });
                self.other_open = Some(egui::collapsing_header::CollapsingState::load(ui.ctx(), header.header_response.id).is_some_and(|s| s.is_open()));
            }
            if !recent.is_empty() {
                ui.add_space(12.0);
                let header = egui::CollapsingHeader::new(format!("Recent chats ({})", recent.len()))
                    .id_salt("recent-sessions")
                    .open(if self.recent_open.is_none() { Some(false) } else { None })
                    .show_accessible(ui, |ui| {
                        ui.weak("Not confirmed open. These chats do not imply a reply is needed.");
                        for item in &recent {
                            if let Some(target) = session_card(ui, item, false) { chosen = Some(target); }
                        }
                    });
                self.recent_open = Some(egui::collapsing_header::CollapsingState::load(ui.ctx(), header.header_response.id).is_some_and(|s| s.is_open()));
            }
            if !self.snapshot.warnings.is_empty() {
                ui.add_space(12.0);
                ui.colored_label(ui.visuals().warn_fg_color, "Some session information is unavailable.");
                egui::CollapsingHeader::new("Discovery details").id_salt("session-warnings").show_accessible(ui, |ui| {
                    for warning in &self.snapshot.warnings { ui.add(egui::Label::new(warning).wrap()); }
                });
            }
            ui.add_space(12.0);
            egui::CollapsingHeader::new("About session discovery").id_salt("session-discovery-help").show_accessible(ui, |ui| {
                ui.add(egui::Label::new("Shows local Codex and Claude Code sessions, including Code chats in Claude Desktop. Browser chats and sessions on other computers are not discovered.").wrap());
                ui.add(egui::Label::new("Running means Codex reports an unfinished turn in a loaded chat, or Claude reports working in a verified live process. The shimmer indicates activity, not how much work is complete. Status refreshes every two seconds.").wrap());
                ui.add(egui::Label::new("Other open chats are idle, waiting or have an unavailable status. Recent chats include the latest 200 unarchived Codex chats and retained Claude session entries. Neither group implies that a reply is needed.").wrap());
                ui.add(egui::Label::new("Desktop chats open in their app. A terminal session offers a resume action in a new terminal; it does not focus an existing terminal. Background agents are available under View.").wrap());
                ui.weak("Session titles and project folders stay on this computer. Conversation contents are not read.");
            });
        });
        chosen
    }
}

fn session_card(ui: &mut egui::Ui, item: &Session, animate: bool) -> Option<(ChatTarget, String)> {
    let mut chosen = None;
    ui.push_id((item.app(), item.target.id()), |ui| {
        let background = ui.painter().add(egui::Shape::Noop);
        let mut frame = theme::surface(ui).inner_margin(10);
        if item.running() {
            frame = frame
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE);
        }
        let card = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(&item.title).strong()).wrap());
            ui.horizontal_wrapped(|ui| {
                if ui.button(item.target.label()).clicked() {
                    chosen = Some((item.target.clone(), item.cwd.clone()));
                }
                ui.menu_button("Details", |ui| {
                    ui.set_max_width(360.0);
                    if !item.cwd.is_empty() {
                        ui.add(egui::Label::new(&item.cwd).wrap().selectable(true));
                    }
                    if let Ok(updated) = time::OffsetDateTime::from_unix_timestamp(item.updated)
                        && let Ok(updated) =
                            updated.format(&time::format_description::well_known::Rfc3339)
                    {
                        ui.weak(format!(
                            "Last activity {}",
                            view::ago(&updated, time::OffsetDateTime::now_utc())
                        ));
                    }
                    ui.add(egui::Label::new(item.target.id()).wrap().selectable(true));
                    if ui.small_button("Copy session ID").clicked() {
                        ui.ctx().copy_text(item.target.id().to_owned());
                    }
                    if let Some(command) = item.target.resume_command()
                        && ui.small_button("Copy resume command").clicked()
                    {
                        ui.ctx().copy_text(command);
                    }
                });
                ui.add(
                    egui::Label::new(if item.running() {
                        RichText::new(format!("{} · Running", item.app()))
                            .strong()
                            .color(ui.visuals().selection.stroke.color)
                    } else {
                        RichText::new(format!("{} · {}", item.app(), item.status())).weak()
                    })
                    .extend(),
                );
                if let Some(project) = std::path::Path::new(&item.cwd)
                    .file_name()
                    .and_then(|n| n.to_str())
                {
                    ui.add(egui::Label::new(RichText::new(project).weak()).wrap())
                        .on_hover_text(&item.cwd);
                }
                if item.background {
                    ui.weak("Background agent");
                }
            });
        });
        if item.running() {
            theme::running_surface(ui, card.response.rect, background, animate);
        }
        ui.add_space(6.0);
    });
    chosen
}
