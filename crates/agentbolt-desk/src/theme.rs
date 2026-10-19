//! Quiet surfaces, amber selection, and one spacing/type scale across the Desk.
use eframe::egui::{self, Color32, FontId, RichText, Stroke, TextStyle};

pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(22.0));
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size.y = 32.0;
        style.spacing.text_edit_width = 300.0;
        let v = &mut style.visuals;
        let dark = v.dark_mode;
        let (background, surface, text, muted, border, accent, selected) = if dark {
            (
                Color32::from_rgb(24, 26, 30),
                Color32::from_rgb(32, 35, 40),
                Color32::from_rgb(235, 236, 239),
                Color32::from_rgb(167, 173, 182),
                Color32::from_rgb(61, 66, 75),
                Color32::from_rgb(255, 196, 46),
                Color32::from_rgb(69, 56, 28),
            )
        } else {
            (
                Color32::from_rgb(248, 248, 246),
                Color32::WHITE,
                Color32::from_rgb(32, 37, 44),
                Color32::from_rgb(92, 99, 110),
                Color32::from_rgb(215, 218, 222),
                Color32::from_rgb(137, 83, 0),
                Color32::from_rgb(255, 235, 183),
            )
        };
        v.panel_fill = background;
        v.window_fill = surface;
        v.extreme_bg_color = surface;
        v.text_edit_bg_color = Some(surface);
        v.faint_bg_color = surface;
        v.override_text_color = Some(text);
        v.weak_text_color = Some(muted);
        v.selection.bg_fill = selected;
        v.selection.stroke = Stroke::new(1.0, accent);
        v.hyperlink_color = accent;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
        for widget in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            widget.corner_radius = 5.into();
            widget.fg_stroke = Stroke::new(1.0, text);
            widget.bg_stroke = Stroke::new(1.0, border);
        }
        v.widgets.inactive.bg_fill = surface;
        v.widgets.hovered.bg_fill = selected;
        v.widgets.active.bg_fill = selected;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent);
        v.widgets.active.bg_stroke = Stroke::new(1.0, accent);
        v.warn_fg_color = if dark {
            Color32::from_rgb(255, 204, 102)
        } else {
            Color32::from_rgb(137, 83, 0)
        };
        v.error_fg_color = if dark {
            Color32::from_rgb(255, 139, 139)
        } else {
            Color32::from_rgb(175, 40, 40)
        };
    });
}

pub fn page_heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.heading(title);
    ui.add(egui::Label::new(RichText::new(description).weak()).wrap());
    ui.add_space(8.0);
}

pub fn surface(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(6)
        .inner_margin(14)
}

pub fn empty(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.28).max(24.0));
        ui.heading(title);
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(description).weak()).wrap());
    });
}

pub fn primary(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        RichText::new(label)
            .color(Color32::from_rgb(41, 31, 9))
            .strong(),
    )
    .fill(Color32::from_rgb(255, 196, 46))
    .min_size(egui::vec2(100.0, 34.0))
}

pub fn brand(ui: &mut egui::Ui) {
    let id = egui::Id::unique("agentbolt-brand-texture");
    let texture = ui
        .ctx()
        .data(|data| data.get_temp::<egui::TextureHandle>(id))
        .unwrap_or_else(|| {
            let texture = ui.ctx().load_texture(
                "Agentbolt",
                egui::ColorImage::from_rgba_unmultiplied(
                    [64, 64],
                    &agentbolt_core::icon::rgba_at(64),
                ),
                egui::TextureOptions::LINEAR,
            );
            ui.ctx()
                .data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        });
    ui.add(egui::Image::new((texture.id(), egui::vec2(24.0, 24.0))).alt_text("Agentbolt icon"));
}
