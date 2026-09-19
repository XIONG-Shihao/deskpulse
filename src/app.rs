use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, RichText,
};
use serde::{Deserialize, Serialize};

use crate::autostart::Autostart;
use crate::config::Config;
use crate::format;
use crate::metrics::{self, Shared, Snapshot};
use crate::tray::{Tray, TrayAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Horizontal,
    Vertical,
}

impl Layout {
    /// Initial window size only. The real size is fitted to the rendered
    /// content every frame, so this just avoids a visible first-frame jump.
    pub fn window_size(self) -> [f32; 2] {
        match self {
            Layout::Vertical => [166.0, 178.0],
            Layout::Horizontal => [748.0, 54.0],
        }
    }
}

// Fixed cell sizes keep the content width constant, so the auto-fitted window
// does not jitter as the digits change (e.g. "9.9 KB/s" -> "1.02 MB/s").
const LABEL_W: f32 = 46.0;
const VALUE_W: f32 = 92.0;
const ROW_H: f32 = 17.0;
const COL_W: f32 = 84.0;
const COL_LABEL_H: f32 = 14.0;

const FIT_EPSILON: f32 = 0.5;

pub struct DeskStatsApp {
    shared: Shared,
    layout: Layout,
    config: Config,
    autostart: Autostart,
    tray: Option<Tray>,
    visible: bool,
    menu_open: bool,
    last_fit: Option<egui::Vec2>,
}

const FONT_CANDIDATES: [&str; 4] = [
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\msyhl.ttc",
    r"C:\Windows\Fonts\simhei.ttf",
    r"C:\Windows\Fonts\simsun.ttc",
];

/// egui ships no CJK glyphs, so Chinese labels would render as tofu boxes.
/// Load a system font if one is available; otherwise labels fall back to the
/// bundled Latin font (ASCII still renders fine).
fn install_cjk_font(ctx: &egui::Context) {
    for path in FONT_CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk".to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("cjk".to_owned());
        ctx.set_fonts(fonts);
        return;
    }
}

impl DeskStatsApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        install_cjk_font(&cc.egui_ctx);

        let autostart = Autostart::new();
        let mut config = config;
        // The registry is the source of truth for whether autostart is active.
        config.autostart = autostart.is_enabled();

        let tray = Tray::new(config.autostart);
        let refresh = Duration::from_secs(config.refresh_secs.max(1));

        Self {
            shared: metrics::spawn(refresh, config.lhm_port),
            layout: config.layout,
            config,
            autostart,
            tray,
            visible: true,
            menu_open: false,
            last_fit: None,
        }
    }

    fn rows(snapshot: &Snapshot) -> [(&'static str, String); 8] {
        [
            ("上传", format::format_speed(snapshot.net_up_bps)),
            ("下载", format::format_speed(snapshot.net_down_bps)),
            ("CPU", format::format_percent(snapshot.cpu_usage)),
            ("CPU温", format::format_temp(snapshot.cpu_temp_c)),
            ("内存", format::format_percent(snapshot.mem_percent())),
            ("GPU", format::format_percent(snapshot.gpu_usage)),
            ("显存", format::format_percent(snapshot.vram_percent())),
            ("GPU温", format::format_temp(snapshot.gpu_temp_c)),
        ]
    }

    fn draw(&self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        let rows = Self::rows(snapshot);
        let label_color = Color32::from_gray(150);

        let label = |text: &str| RichText::new(text).color(label_color).size(12.0);
        let value = |text: String| RichText::new(text).color(Color32::WHITE).strong().size(14.0);

        match self.layout {
            Layout::Vertical => {
                for (key, val) in rows {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [LABEL_W, ROW_H],
                            egui::Label::new(label(key)).selectable(false),
                        );
                        ui.add_sized(
                            [VALUE_W, ROW_H],
                            egui::Label::new(value(val)).selectable(false),
                        );
                    });
                }
            }
            Layout::Horizontal => {
                ui.horizontal(|ui| {
                    for (key, val) in rows {
                        ui.vertical(|ui| {
                            ui.add_sized(
                                [COL_W, COL_LABEL_H],
                                egui::Label::new(label(key)).selectable(false),
                            );
                            ui.add_sized(
                                [COL_W, ROW_H],
                                egui::Label::new(value(val)).selectable(false),
                            );
                        });
                    }
                });
            }
        }
    }

    fn fit_to(&mut self, ctx: &egui::Context, desired: egui::Vec2) {
        let changed = self
            .last_fit
            .is_none_or(|last| (last - desired).length() > FIT_EPSILON);
        if changed {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired));
            self.last_fit = Some(desired);
        }
    }

    fn set_layout(&mut self, layout: Layout) {
        if self.layout == layout {
            return;
        }
        self.layout = layout;
        self.config.layout = layout;
        self.config.save();
        // Force a refit for the new content size.
        self.last_fit = None;
    }

    fn set_autostart(&mut self, enabled: bool) {
        if self.autostart.set(enabled) {
            self.config.autostart = self.autostart.is_enabled();
        }
        self.config.save();
        if let Some(tray) = self.tray.as_ref() {
            tray.set_autostart_checked(self.config.autostart);
        }
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.config.save();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// The settings menu is drawn inline (not as an egui popup) so the
    /// auto-fitted window grows to contain it. A popup would be clipped by the
    /// tiny window edge.
    fn draw_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("布局");
            let mut picked = false;
            if ui
                .selectable_label(self.layout == Layout::Vertical, "竖排")
                .clicked()
            {
                self.set_layout(Layout::Vertical);
                picked = true;
            }
            if ui
                .selectable_label(self.layout == Layout::Horizontal, "横排")
                .clicked()
            {
                self.set_layout(Layout::Horizontal);
                picked = true;
            }
            if picked {
                self.menu_open = false;
            }
        });

        let mut autostart = self.config.autostart;
        if ui.checkbox(&mut autostart, "开机自启").changed() {
            self.set_autostart(autostart);
        }

        if ui.button("退出").clicked() {
            self.menu_open = false;
            self.quit(ctx);
        }
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        let actions = self
            .tray
            .as_ref()
            .map(|tray| tray.poll())
            .unwrap_or_default();

        for action in actions {
            match action {
                TrayAction::ToggleVisible => {
                    self.visible = !self.visible;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
                    if self.visible {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
                TrayAction::SetLayout(layout) => self.set_layout(layout),
                TrayAction::SetAutostart(enabled) => self.set_autostart(enabled),
                TrayAction::Quit => self.quit(ctx),
            }
        }
    }
}

impl eframe::App for DeskStatsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    /// Runs before every `ui`, and also while the window is hidden as long as a
    /// repaint is requested. Tray events must be handled here so the window can
    /// be shown again after being hidden.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray(ctx);
        ctx.request_repaint_after(Duration::from_millis(400));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            self.config.position = Some([rect.min.x, rect.min.y]);
        }

        let snapshot = self
            .shared
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();

        let alpha = (self.config.opacity.clamp(0.0, 1.0) * 255.0) as u8;

        // Right-click anywhere on the widget toggles the inline settings menu.
        if ui.input(|input| input.pointer.secondary_clicked()) {
            self.menu_open = !self.menu_open;
        }

        let response = egui::Frame::NONE
            .fill(Color32::from_black_alpha(alpha))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                // Never wrap: wrapping would distort the measured content size.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                // The drag area would swallow clicks on the menu buttons, so it
                // is only active while the menu is closed.
                if !self.menu_open {
                    let response = ui.interact(
                        ui.max_rect(),
                        ui.id().with("drag-area"),
                        egui::Sense::click_and_drag(),
                    );
                    if response.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }

                self.draw(ui, &snapshot);

                if self.menu_open {
                    self.draw_menu(ui, &ctx);
                }
            });

        // Shrink-wrap the window to the content.
        let content = response.response.rect.size();
        self.fit_to(&ctx, content);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.config.save();
    }
}
