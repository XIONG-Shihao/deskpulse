use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, RichText,
};
use serde::{Deserialize, Serialize};

use crate::autostart::Autostart;
use crate::config::Config;
use crate::format;
use crate::i18n::{Language, Text};
use crate::metrics::{self, Shared, Snapshot};
use crate::tray::Tray;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Horizontal,
    Grid,
    #[default]
    Vertical,
}

impl Layout {
    /// Initial window size only. The real size is fitted to the rendered
    /// content every frame, so this just avoids a visible first-frame jump.
    pub fn window_size(self) -> [f32; 2] {
        match self {
            Layout::Vertical => [146.0, 151.0],
            Layout::Horizontal => [704.0, 37.0],
            Layout::Grid => [292.0, 77.0],
        }
    }
}

/// Cell spacing preset, selectable from the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spacing {
    /// Original: wide fixed cells with centered text.
    Loose,
    /// Smaller cells; labels right-aligned, values left-aligned.
    #[default]
    Tight,
}

/// A metric that can be shown in the overlay and toggled from the menu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Metric {
    NetUp,
    NetDown,
    Cpu,
    CpuTemp,
    Mem,
    Gpu,
    Vram,
    GpuTemp,
}

impl Metric {
    const ALL: [Metric; 8] = [
        Metric::NetUp,
        Metric::NetDown,
        Metric::Cpu,
        Metric::CpuTemp,
        Metric::Mem,
        Metric::Gpu,
        Metric::Vram,
        Metric::GpuTemp,
    ];

    /// Stable key, used both in the config file and as the menu item id.
    fn id(self) -> &'static str {
        match self {
            Metric::NetUp => "net_up",
            Metric::NetDown => "net_down",
            Metric::Cpu => "cpu",
            Metric::CpuTemp => "cpu_temp",
            Metric::Mem => "mem",
            Metric::Gpu => "gpu",
            Metric::Vram => "vram",
            Metric::GpuTemp => "gpu_temp",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|metric| metric.id() == id)
    }

    fn label(self, t: &Text) -> &'static str {
        match self {
            Metric::NetUp => t.net_up,
            Metric::NetDown => t.net_down,
            Metric::Cpu => t.cpu,
            Metric::CpuTemp => t.cpu_temp,
            Metric::Mem => t.mem,
            Metric::Gpu => t.gpu,
            Metric::Vram => t.vram,
            Metric::GpuTemp => t.gpu_temp,
        }
    }

    fn value(self, snapshot: &Snapshot) -> String {
        match self {
            Metric::NetUp => format::format_speed(snapshot.net_up_bps),
            Metric::NetDown => format::format_speed(snapshot.net_down_bps),
            Metric::Cpu => format::format_percent(snapshot.cpu_usage),
            Metric::CpuTemp => format::format_temp(snapshot.cpu_temp_c),
            Metric::Mem => format::format_percent(snapshot.mem_percent()),
            Metric::Gpu => format::format_percent(snapshot.gpu_usage),
            Metric::Vram => format::format_percent(snapshot.vram_percent()),
            Metric::GpuTemp => format::format_temp(snapshot.gpu_temp_c),
        }
    }
}

/// A change requested by the menu (which runs in a separate viewport and cannot
/// touch the app state directly).
enum MenuAction {
    SetLayout(Layout),
    SetSpacing(Spacing),
    SetLanguage(Language),
    SetVisible(Metric, bool),
    SetAutostart(bool),
    Quit,
}

/// State shared between the app and the menu viewport callback.
#[derive(Default)]
struct MenuState {
    open: bool,
    position: [f32; 2],
    frames: u32,
    had_focus: bool,
    layout: Layout,
    spacing: Spacing,
    language: Language,
    autostart: bool,
    visible: BTreeMap<String, bool>,
    actions: Vec<MenuAction>,
}

// Fixed cell sizes keep the content width constant, so the auto-fitted window
// does not jitter as the digits change (e.g. "9.9 KB/s" -> "1.02 MB/s").
const LABEL_W_LOOSE: f32 = 46.0;
const VALUE_W_LOOSE: f32 = 92.0;
const LABEL_W_TIGHT: f32 = 40.0;
const VALUE_W_TIGHT: f32 = 74.0;
const ROW_H: f32 = 17.0;
/// Gap between a label and its value inside one cell.
const LABEL_VALUE_GAP: f32 = 4.0;
const COL_W: f32 = 84.0;
const COL_LABEL_H: f32 = 14.0;

const FIT_EPSILON: f32 = 0.5;

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

/// Cursor position in screen points, for placing the menu window.
fn cursor_screen_pos(pixels_per_point: f32) -> egui::Pos2 {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
    }

    let mut point = Point { x: 0, y: 0 };
    // SAFETY: we pass a valid pointer to a stack value.
    unsafe {
        GetCursorPos(&mut point);
    }
    egui::pos2(
        point.x as f32 / pixels_per_point,
        point.y as f32 / pixels_per_point,
    )
}

pub struct DeskStatsApp {
    shared: Shared,
    layout: Layout,
    config: Config,
    autostart: Autostart,
    tray: Option<Tray>,
    visible: bool,
    menu: Arc<Mutex<MenuState>>,
    /// Menu ids delivered by the listener thread (see `new`).
    menu_events: Arc<Mutex<Vec<String>>>,
    last_fit: Option<egui::Vec2>,
}

/// The native window handle, used to hide the window from the taskbar.
fn window_hwnd(frame: &eframe::Frame) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match frame.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
        _ => None,
    }
}

impl DeskStatsApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        install_cjk_font(&cc.egui_ctx);
        // The overlay always has a dark background, so force dark widget colors.
        // Otherwise a light system theme yields dark text on our dark panel,
        // which is barely readable.
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let autostart = Autostart::new();
        let mut config = config;
        // The registry is the source of truth for whether autostart is active.
        config.autostart = autostart.is_enabled();

        let language = config.language.unwrap_or(Language::Zh);
        let tray = Tray::new(config.autostart, &language.text());
        let refresh = Duration::from_secs(config.refresh_secs.max(1));

        // Tray menu events must be picked up immediately, not on the next
        // scheduled repaint (which is up to ~400ms away). A listener thread
        // receives them, queues the ids, and wakes the UI right away.
        let menu_events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let queue = Arc::clone(&menu_events);
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                while let Ok(event) = tray_icon::menu::MenuEvent::receiver().recv() {
                    if let Ok(mut queue) = queue.lock() {
                        queue.push(event.id.0.clone());
                    }
                    ctx.request_repaint();
                }
            });
        }

        Self {
            shared: metrics::spawn(refresh, config.lhm_port),
            layout: config.layout,
            config,
            autostart,
            tray,
            visible: true,
            menu: Arc::new(Mutex::new(MenuState::default())),
            menu_events,
            last_fit: None,
        }
    }

    fn language(&self) -> Language {
        self.config.language.unwrap_or(Language::Zh)
    }

    fn text(&self) -> Text {
        self.language().text()
    }

    fn is_visible(&self, metric: Metric) -> bool {
        self.config
            .visible
            .get(metric.id())
            .copied()
            .unwrap_or(true)
    }

    fn set_visible(&mut self, metric: Metric, visible: bool) {
        self.config
            .visible
            .insert(metric.id().to_owned(), visible);
        self.config.save();
        self.last_fit = None;
    }

    fn draw(&self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        // No automatic horizontal spacing: the label/value gap is added inside a
        // cell, and grid columns are packed with no gap between them.
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.5);

        let t = self.text();
        let label_color = Color32::from_gray(150);
        let rows: Vec<(&'static str, String)> = Metric::ALL
            .iter()
            .filter(|metric| self.is_visible(**metric))
            .map(|metric| (metric.label(&t), metric.value(snapshot)))
            .collect();

        let label = |text: &str| RichText::new(text).color(label_color).size(12.0);
        let value = |text: String| RichText::new(text).color(Color32::WHITE).strong().size(14.0);

        let spacing = self.config.spacing;

        // One label/value cell. `Loose` uses wide boxes with centered text;
        // `Tight` shrinks the boxes and packs the label against a left-aligned
        // value. `label_left` left-aligns the label instead (used for the
        // second column of the grid, so its names line up on the left).
        let cell = |ui: &mut egui::Ui, key: &str, val: String, label_left: bool| {
            let label_widget = egui::Label::new(label(key)).selectable(false);
            let value_widget = egui::Label::new(value(val)).selectable(false);
            match spacing {
                Spacing::Loose => {
                    ui.add_sized([LABEL_W_LOOSE, ROW_H], label_widget);
                    ui.add_space(LABEL_VALUE_GAP);
                    ui.add_sized([VALUE_W_LOOSE, ROW_H], value_widget);
                }
                Spacing::Tight => {
                    let label_layout = if label_left {
                        egui::Layout::left_to_right(egui::Align::Center)
                    } else {
                        egui::Layout::right_to_left(egui::Align::Center)
                    };
                    ui.allocate_ui_with_layout(
                        egui::vec2(LABEL_W_TIGHT, ROW_H),
                        label_layout,
                        |ui| {
                            // Force the box width: a right/left aligned layout
                            // otherwise shrinks to the text, which would shift
                            // the next column per row.
                            ui.set_min_width(LABEL_W_TIGHT);
                            ui.set_min_height(ROW_H);
                            ui.add(label_widget);
                        },
                    );
                    ui.add_space(LABEL_VALUE_GAP);
                    ui.allocate_ui_with_layout(
                        egui::vec2(VALUE_W_TIGHT, ROW_H),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(VALUE_W_TIGHT);
                            ui.set_min_height(ROW_H);
                            ui.add(value_widget);
                        },
                    );
                }
            }
        };

        match self.layout {
            Layout::Vertical => {
                for (key, val) in rows {
                    ui.horizontal(|ui| {
                        cell(ui, key, val, false);
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
                        ui.add_space(LABEL_VALUE_GAP);
                    }
                });
            }
            Layout::Grid => {
                // Two label/value cells per row. The second column's labels are
                // left-aligned (in tight mode) so the names line up on the left.
                for chunk in rows.chunks(2) {
                    ui.horizontal(|ui| {
                        for (index, (key, val)) in chunk.iter().enumerate() {
                            cell(ui, key, val.clone(), index == 1);
                        }
                    });
                }
            }
        }
    }

    fn open_menu(&mut self, ctx: &egui::Context) {
        let pos = cursor_screen_pos(ctx.pixels_per_point());
        if let Ok(mut state) = self.menu.lock() {
            state.open = true;
            state.position = [pos.x, pos.y];
            state.frames = 0;
            state.had_focus = false;
            state.actions.clear();
        }
    }

    /// Drives the menu, which lives in its own small, tightly-fitted window so
    /// the main overlay stays exactly its content size (no wasted transparent
    /// area). Must be called every frame while the menu is open.
    fn show_menu(&mut self, ctx: &egui::Context) {
        let open = self.menu.lock().map(|state| state.open).unwrap_or(false);
        if !open {
            return;
        }

        if let Ok(mut state) = self.menu.lock() {
            state.layout = self.layout;
            state.spacing = self.config.spacing;
            state.language = self.language();
            state.autostart = self.config.autostart;
            state.visible = self.config.visible.clone();
        }

        let position = {
            let state = self.menu.lock().ok();
            let [x, y] = state.as_ref().map(|s| s.position).unwrap_or([0.0, 0.0]);
            egui::pos2(x, y)
        };

        let builder = egui::ViewportBuilder::default()
            .with_title("deskpulse-menu")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false)
            .with_position(position)
            .with_inner_size([200.0, 380.0]);

        let state = Arc::clone(&self.menu);
        let id = egui::ViewportId::from_hash_of("deskpulse-context-menu");
        ctx.show_viewport_deferred(id, builder, move |ui, _class| {
            let Ok(mut state) = state.lock() else {
                return;
            };
            if !state.open {
                return;
            }
            state.frames += 1;

            let child = ui.ctx().clone();
            let current = ui.max_rect().size();

            let focused = child.input(|input| input.viewport().focused);
            if focused == Some(true) {
                state.had_focus = true;
            }
            // Dismiss on focus loss, Escape, or a second right-click. Ignore the
            // very first frame so the opening click cannot close it immediately.
            let dismiss = (state.had_focus && focused == Some(false))
                || (state.frames > 1
                    && (child.input(|input| input.key_pressed(egui::Key::Escape))
                        || child.input(|input| input.pointer.secondary_clicked())));
            if dismiss {
                state.open = false;
                child.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }

            let language = state.language;
            let layout = state.layout;
            let spacing = state.spacing;
            let autostart = state.autostart;
            let visible = state.visible.clone();
            let mut actions = Vec::new();

            // Dark widgets + light text, regardless of the system theme. Only
            // overriding the text color left buttons with a light background
            // (white-on-light, unreadable).
            let mut visuals = egui::Visuals::dark();
            visuals.override_text_color = Some(Color32::from_rgb(245, 245, 245));
            *ui.visuals_mut() = visuals;

            let t = language.text();
            let mut close = false;
            let response = egui::Frame::NONE
                .fill(Color32::from_rgb(26, 26, 26))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.set_min_width(150.0);

                    ui.menu_button(t.metrics, |ui| {
                        for metric in Metric::ALL {
                            let mut on = visible.get(metric.id()).copied().unwrap_or(true);
                            if ui.checkbox(&mut on, metric.label(&t)).changed() {
                                actions.push(MenuAction::SetVisible(metric, on));
                            }
                        }
                    });

                    ui.menu_button(t.layout, |ui| {
                        if ui
                            .selectable_label(layout == Layout::Vertical, t.vertical)
                            .clicked()
                        {
                            actions.push(MenuAction::SetLayout(Layout::Vertical));
                            close = true;
                        }
                        if ui
                            .selectable_label(layout == Layout::Horizontal, t.horizontal)
                            .clicked()
                        {
                            actions.push(MenuAction::SetLayout(Layout::Horizontal));
                            close = true;
                        }
                        if ui
                            .selectable_label(layout == Layout::Grid, t.grid)
                            .clicked()
                        {
                            actions.push(MenuAction::SetLayout(Layout::Grid));
                            close = true;
                        }
                    });

                    ui.menu_button(t.spacing, |ui| {
                        if ui
                            .selectable_label(spacing == Spacing::Loose, t.spacing_loose)
                            .clicked()
                        {
                            actions.push(MenuAction::SetSpacing(Spacing::Loose));
                            close = true;
                        }
                        if ui
                            .selectable_label(spacing == Spacing::Tight, t.spacing_tight)
                            .clicked()
                        {
                            actions.push(MenuAction::SetSpacing(Spacing::Tight));
                            close = true;
                        }
                    });

                    ui.menu_button(t.language, |ui| {
                        if ui
                            .selectable_label(language == Language::Zh, "中文")
                            .clicked()
                        {
                            actions.push(MenuAction::SetLanguage(Language::Zh));
                            close = true;
                        }
                        if ui
                            .selectable_label(language == Language::En, "English")
                            .clicked()
                        {
                            actions.push(MenuAction::SetLanguage(Language::En));
                            close = true;
                        }
                    });

                    let mut on = autostart;
                    if ui.checkbox(&mut on, t.autostart).changed() {
                        actions.push(MenuAction::SetAutostart(on));
                    }

                    ui.separator();
                    if ui.button(t.quit).clicked() {
                        actions.push(MenuAction::Quit);
                        close = true;
                    }
                });

            state.actions.append(&mut actions);

            // Shrink-wrap the menu window to its content.
            let desired = response.response.rect.size();
            if (current - desired).length() > FIT_EPSILON {
                child.send_viewport_cmd(egui::ViewportCommand::InnerSize(desired));
            }

            if close {
                state.open = false;
                child.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    /// Applies actions requested by the menu. Must run every frame regardless of
    /// whether the menu is still open: the deferred viewport callback runs
    /// asynchronously, and a click on "layout"/"language" both emits an action
    /// and closes the menu, so the action would otherwise be dropped.
    fn apply_menu_actions(&mut self, ctx: &egui::Context) {
        let actions = self
            .menu
            .lock()
            .map(|mut state| std::mem::take(&mut state.actions))
            .unwrap_or_default();
        for action in actions {
            match action {
                MenuAction::SetLayout(value) => self.set_layout(value),
                MenuAction::SetSpacing(value) => self.set_spacing(value),
                MenuAction::SetLanguage(value) => self.set_language(value),
                MenuAction::SetVisible(metric, value) => self.set_visible(metric, value),
                MenuAction::SetAutostart(value) => self.set_autostart(value),
                MenuAction::Quit => self.quit(ctx),
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
        self.last_fit = None;
    }

    fn set_spacing(&mut self, spacing: Spacing) {
        if self.config.spacing == spacing {
            return;
        }
        self.config.spacing = spacing;
        self.config.save();
        self.last_fit = None;
    }

    fn set_language(&mut self, language: Language) {
        if self.config.language == Some(language) {
            return;
        }
        self.config.language = Some(language);
        self.config.save();
        // Rebuild the tray so its menu picks up the new language.
        self.tray = Tray::new(self.config.autostart, &language.text());
        self.last_fit = None;
    }

    fn set_autostart(&mut self, enabled: bool) {
        // On success trust the requested state instead of re-reading the
        // registry (the read queries both HKLM and HKCU and is slow).
        if self.autostart.set(enabled) {
            self.config.autostart = enabled;
        }
        self.config.save();
        if let Some(tray) = self.tray.as_ref() {
            tray.set_autostart_checked(self.config.autostart);
        }
    }

    fn quit(&mut self, ctx: &egui::Context) {
        crate::diag::log("quit() called");
        self.config.save();
        // Send to the root viewport so it works from the menu viewport too.
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
    }

    /// Tray menu ids collected by the listener thread, dispatched by id.
    fn handle_menu_events(&mut self, ctx: &egui::Context) {
        let events = self
            .menu_events
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();

        for id in events {
            match id.as_str() {
                "toggle" => {
                    self.visible = !self.visible;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
                    if self.visible {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
                "layout_v" => self.set_layout(Layout::Vertical),
                "layout_h" => self.set_layout(Layout::Horizontal),
                "autostart" => {
                    let enabled = !self.config.autostart;
                    self.set_autostart(enabled);
                }
                "quit" => {
                    crate::diag::log("tray quit event");
                    self.quit(ctx);
                }
                id => {
                    if let Some(metric) = Metric::from_id(id) {
                        let visible = !self.is_visible(metric);
                        self.set_visible(metric, visible);
                    }
                }
            }
        }
    }
}

impl eframe::App for DeskStatsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    /// Runs before every `ui`, and also while the window is hidden as long as a
    /// repaint is requested. Menu events must be handled here so the window can
    /// be shown again after being hidden.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_menu_actions(ctx);
        self.handle_menu_events(ctx);
        ctx.request_repaint_after(Duration::from_millis(400));
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Keep the window out of the taskbar: eframe ignores with_taskbar(false)
        // and winit re-applies its styles on show, so this runs every frame.
        if let Some(hwnd) = window_hwnd(frame) {
            crate::window::ensure_overlay_style(hwnd);
        }

        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            self.config.position = Some([rect.min.x, rect.min.y]);
        }

        // Right-click opens the settings menu in its own window; the metrics
        // window stays exactly its content size.
        if ctx.input(|input| input.pointer.secondary_clicked()) {
            self.open_menu(&ctx);
        }

        let snapshot = self
            .shared
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();

        let alpha = (self.config.opacity.clamp(0.0, 1.0) * 255.0) as u8;

        let response = egui::Frame::NONE
            .fill(Color32::from_black_alpha(alpha))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(2))
            .show(ui, |ui| {
                // Never wrap: wrapping would distort the measured content size.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                let response = ui.interact(
                    ui.max_rect(),
                    ui.id().with("drag-area"),
                    egui::Sense::click_and_drag(),
                );
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                self.draw(ui, &snapshot);
            });

        self.show_menu(&ctx);

        // Shrink-wrap the window to the content.
        self.fit_to(&ctx, response.response.rect.size());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::diag::log("on_exit");
        self.config.save();
    }
}
