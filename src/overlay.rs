//! The overlay: a single per-pixel-alpha Win32 layered window drawn entirely
//! with GDI.
//!
//! No GPU API is involved at all (no OpenGL/Direct3D/Vulkan), so it runs on any
//! x64 Windows, including machines with only the Microsoft Basic Display
//! adapter, virtual machines and remote sessions. The panel background and the
//! text are composited by us into one 32-bit DIB and handed to
//! `UpdateLayeredWindow`.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tray_icon::menu::ContextMenu;

use crate::autostart::Autostart;
use crate::config::{Align, Config};
use crate::i18n::{Language, Text};
use crate::metrics::{self, Shared, Snapshot};
use crate::tray::Tray;

// ---------------------------------------------------------------- constants

const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

const WM_DESTROY: u32 = 0x0002;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_DPICHANGED: u32 = 0x02E0;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_APP: u32 = 0x8000;
const WM_APP_DATA: u32 = WM_APP + 1;
const WM_APP_MENU: u32 = WM_APP + 2;
const WM_APP_TOPMOST: u32 = WM_APP + 3;

const HWND_TOPMOST: isize = -1;
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const WINEVENT_OUTOFCONTEXT: u32 = 0;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;

const DT_CENTER: u32 = 0x0001;
const DT_RIGHT: u32 = 0x0002;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_NOPREFIX: u32 = 0x0800;

const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const TRANSPARENT: i32 = 1;
const ULW_ALPHA: u32 = 2;
const AC_SRC_ALPHA: u8 = 1;

const IDC_ARROW: isize = 32512;

pub use dpi::enable_dpi_awareness;

use crate::wide::wide;

use canvas::{Canvas, TextMetrics};
use metric::Metric;
use topmost::{TOPMOST_PENDING, foreground_changed};
use win32::*;

mod canvas;
mod dpi;
mod fullscreen;
mod layout;
mod menu;
mod metric;
mod paint;
mod topmost;
mod win32;

const MARGIN: f32 = 2.0;
/// One typographic point (1/72 inch) expressed in logical 96-DPI units. A
/// length defined in points therefore keeps the same physical size on every
/// monitor, whatever the resolution and display scaling.
const PT: f32 = 96.0 / 72.0;
/// Gap between a metric's name and its value, in typographic points.
const NAME_GAP_PT: f32 = 2.0;
/// Gap between metric columns in the horizontal layout, in logical units.
const COLUMN_GAP: f32 = 4.0;
const ROW_H: f32 = 17.0;
const LABEL_SIZE: f32 = 12.0;
const VALUE_SIZE: f32 = 14.0;
/// How long the exclusive-fullscreen notice stays up after the game releases
/// the display, so it can actually be read.
const FULLSCREEN_HINT_LINGER: Duration = Duration::from_secs(10);

static INSTANCE: AtomicPtr<Overlay> = AtomicPtr::new(std::ptr::null_mut());
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);

pub struct Overlay {
    hwnd: isize,
    foreground_hook: isize,
    config: Config,
    language: Language,
    shared: Shared,
    autostart: Autostart,
    tray: Option<Tray>,
    menu_ids: Arc<Mutex<Vec<String>>>,
    canvas: Option<Canvas>,
    canvas_key: Option<(i32, i32, i32)>,
    metrics: Option<TextMetrics>,
    metrics_key: Option<i32>,
    size: (i32, i32),
    scale: f32,
    visible: bool,
    dragging: Option<(i32, i32, i32, i32)>,
    /// Whether the last foreground-change check found us covered.
    topmost_covered: bool,
    /// Whether the last poll found a real exclusive-fullscreen app.
    exclusive_now: bool,
    /// While in the future, the panel shows the exclusive-fullscreen notice.
    hint_until: Option<Instant>,
}

impl Overlay {
    pub fn new(config: Config) -> Self {
        let autostart = Autostart::new();
        let mut config = config;
        config.autostart = autostart.is_enabled();
        // Remove visibility entries left behind by a metric that no longer
        // exists, so the config file does not accumulate dead keys.
        let before = config.visible.len();
        config.visible.retain(|id, _| Metric::from_id(id).is_some());
        if config.visible.len() != before {
            config.save();
        }
        let language = config.language.unwrap_or(Language::Zh);
        Self {
            hwnd: 0,
            foreground_hook: 0,
            language,
            config,
            shared: Arc::new(Mutex::new(Snapshot::default())),
            autostart,
            tray: None,
            menu_ids: Arc::new(Mutex::new(Vec::new())),
            canvas: None,
            canvas_key: None,
            metrics: None,
            metrics_key: None,
            size: (1, 1),
            scale: 1.0,
            visible: true,
            dragging: None,
            topmost_covered: false,
            exclusive_now: false,
            hint_until: None,
        }
    }

    /// Starts the metrics collector, waking the window when data changes.
    pub fn start_metrics(&mut self) {
        let interval = Duration::from_secs(self.config.refresh_secs.max(1));
        let port = self.config.lhm_port;
        let hwnd = self.hwnd;
        self.shared = metrics::spawn(interval, port, move || {
            // SAFETY: posting a message to our own window from another thread.
            unsafe {
                PostMessageW(hwnd, WM_APP_DATA, 0, 0);
            }
        });
    }

    fn text(&self) -> Text {
        self.language.text()
    }

    /// The notice shown while — and for a few seconds after — a game holds the
    /// display in exclusive fullscreen. The panel cannot be composited during
    /// that, so the lingering part is the only readable one.
    fn fullscreen_hint(&self) -> Option<(&'static str, &'static str)> {
        let until = self.hint_until?;
        if Instant::now() >= until {
            return None;
        }
        let t = self.text();
        Some((t.hint_label, t.hint_value))
    }

    /// Keeps the exclusive-fullscreen notice up to date. Called on every tick,
    /// before the visibility check, so the deadline is right even when the
    /// panel is hidden.
    fn poll_fullscreen(&mut self) {
        let exclusive = fullscreen::exclusive_fullscreen();
        if exclusive != self.exclusive_now {
            self.exclusive_now = exclusive;
            crate::diag::log(if exclusive {
                "fullscreen: exclusive D3D app detected; the panel cannot be composited"
            } else {
                "fullscreen: exclusive mode released"
            });
        }
        if exclusive {
            self.hint_until = Some(Instant::now() + FULLSCREEN_HINT_LINGER);
        }
    }

    fn is_visible_metric(&self, metric: Metric) -> bool {
        self.config
            .visible
            .get(metric.id())
            .copied()
            .unwrap_or(true)
    }

    fn set_visible_metric(&mut self, metric: Metric, visible: bool) {
        self.config.visible.insert(metric.id().to_owned(), visible);
        self.config.save();
        self.refresh();
    }

    pub fn init_window(&mut self) {
        let class_name = wide("deskpulse");
        let window_name = wide("deskpulse");
        // SAFETY: registration uses a stack WNDCLASSW with valid pointers.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = WndClassW {
                style: 0,
                wnd_proc: Some(wnd_proc),
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: 0,
                cursor: LoadCursorW(0, IDC_ARROW),
                background: 0,
                menu_name: std::ptr::null(),
                class_name: class_name.as_ptr(),
            };
            RegisterClassW(&class);

            let (x, y) = self
                .config
                .position
                .map(|[x, y]| (x as i32, y as i32))
                .unwrap_or((100, 100));

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                window_name.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                1,
                1,
                0,
                0,
                instance,
                std::ptr::null_mut(),
            );
            if hwnd == 0 {
                crate::diag::log("overlay window creation failed");
                return;
            }
            self.hwnd = hwnd;
            OVERLAY_HWND.store(hwnd, Ordering::Relaxed);
            crate::diag::log("overlay window ready (win32/gdi)");
        }

        // Other topmost windows can move above us when they take focus.
        // The callback only posts a message; the UI thread changes z-order.
        self.foreground_hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                0,
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if self.foreground_hook == 0 {
            crate::diag::log("foreground event hook failed");
        } else {
            crate::diag::log("foreground event hook ready");
        }
        self.ensure_topmost();
        self.scale = self.query_scale();
        self.tray = Tray::new(self.build_menu());
        crate::diag::log(&format!(
            "tray menu: {}",
            if self.tray.is_some() {
                "ready"
            } else {
                "unavailable"
            }
        ));
        self.install_menu_handler();
        self.refresh();
    }

    pub fn run(&mut self) -> i32 {
        let mut msg = Msg::default();
        // SAFETY: standard Win32 message pump on this thread.
        unsafe {
            loop {
                let result = GetMessageW(&mut msg, 0, 0, 0);
                if result <= 0 {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        msg.w_param as i32
    }

    // ------------------------------------------------------------- layout

    fn refresh(&mut self) {
        self.poll_fullscreen();
        if self.hwnd == 0 || !self.visible {
            return;
        }
        let key = (self.scale * 1000.0) as i32;
        if self.metrics_key != Some(key) {
            self.metrics = TextMetrics::create(self.scale);
            self.metrics_key = Some(key);
        }

        let snapshot = self.shared.lock().map(|g| g.clone()).unwrap_or_default();
        let rows = self.rows(&snapshot);
        let text = self.text();
        // Reserve the columns from the metric set, not from the current values:
        // the label from the labels, the value from the widest string each
        // visible metric can render, so the panel width never grows with data.
        // The temporary fullscreen notice is part of the reservation while it
        // is up, so its longer text is never clipped.
        let hint = self.fullscreen_hint();
        let (mut label_w, mut value_w) = (0.0_f32, 0.0_f32);
        if let (Some(metrics), Some((label, value))) = (self.metrics.as_ref(), hint) {
            label_w = metrics.width(label, false);
            value_w = metrics.width(value, true);
        }
        let (label_w, value_w) = match self.metrics.as_ref() {
            Some(metrics) => Metric::ALL
                .iter()
                .filter(|metric| self.is_visible_metric(**metric))
                .fold((label_w, value_w), |(label_w, value_w), metric| {
                    let label = metrics.width(metric.label(&text), false).max(label_w);
                    let value = metric
                        .widest_values()
                        .iter()
                        .map(|candidate| metrics.width(candidate, true))
                        .fold(value_w, f32::max);
                    (label, value)
                }),
            None => (
                (46.0 * self.scale).max(label_w),
                (74.0 * self.scale).max(value_w),
            ),
        };
        let (label_w, value_w) = (label_w.ceil(), value_w.ceil());

        let (width, height) = self.content_size(rows.len(), label_w, value_w);
        if (width, height) != self.size {
            self.size = (width, height);
            // SAFETY: resizing our own window.
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    0,
                    0,
                    0,
                    width,
                    height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            self.canvas_key = None;
        }

        let spans = self.build_spans(&rows, label_w, value_w);
        self.paint(&spans);
    }

    fn handle_message(&mut self, hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize {
        match msg {
            WM_APP_DATA => {
                self.refresh();
                0
            }
            WM_APP_MENU => {
                self.apply_menu_ids();
                0
            }
            WM_APP_TOPMOST => {
                TOPMOST_PENDING.store(false, Ordering::Release);
                self.ensure_topmost();
                0
            }
            WM_ERASEBKGND => 1,
            WM_LBUTTONDOWN => {
                self.begin_drag();
                0
            }
            WM_MOUSEMOVE => {
                self.update_drag();
                0
            }
            WM_LBUTTONUP => {
                self.end_drag();
                0
            }
            WM_RBUTTONUP => {
                self.show_context_menu();
                0
            }
            WM_DPICHANGED => {
                let dpi = ((w_param >> 16) & 0xFFFF) as u32;
                // SAFETY: `l_param` points to the OS-suggested RECT for this message.
                let suggested = unsafe { *(l_param as *const Rect) };
                self.apply_dpi(dpi, suggested.left, suggested.top);
                0
            }
            WM_DESTROY => {
                OVERLAY_HWND.store(0, Ordering::Relaxed);
                if self.foreground_hook != 0 {
                    // SAFETY: this hook was installed on the UI thread.
                    unsafe { UnhookWinEvent(self.foreground_hook) };
                    self.foreground_hook = 0;
                }
                self.config.save();
                // SAFETY: ends the message loop.
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => {
                // SAFETY: default handling for everything else.
                unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) }
            }
        }
    }

    fn begin_drag(&mut self) {
        // SAFETY: capture on our own window.
        unsafe {
            let mut cursor = Point::default();
            let mut rect = Rect::default();
            if GetCursorPos(&mut cursor) == 0 || GetWindowRect(self.hwnd, &mut rect) == 0 {
                return;
            }
            self.dragging = Some((cursor.x, cursor.y, rect.left, rect.top));
            SetCapture(self.hwnd);
        }
    }

    fn update_drag(&mut self) {
        let Some((start_x, start_y, win_x, win_y)) = self.dragging else {
            return;
        };
        // SAFETY: moving our own window.
        unsafe {
            let mut cursor = Point::default();
            if GetCursorPos(&mut cursor) == 0 {
                return;
            }
            let x = win_x + (cursor.x - start_x);
            let y = win_y + (cursor.y - start_y);
            SetWindowPos(
                self.hwnd,
                0,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            self.config.position = Some([x as f32, y as f32]);
        }
    }

    fn end_drag(&mut self) {
        if self.dragging.take().is_some() {
            // SAFETY: release the mouse capture we took.
            unsafe { ReleaseCapture() };
            self.config.save();
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize {
    let ptr = INSTANCE.load(Ordering::Relaxed);
    if ptr.is_null() {
        // SAFETY: default handling.
        return unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) };
    }
    // SAFETY: `INSTANCE` holds a live, single `Overlay`.
    let overlay = unsafe { &mut *ptr };
    if overlay.hwnd == 0 {
        overlay.hwnd = hwnd;
    }
    overlay.handle_message(hwnd, msg, w_param, l_param)
}

/// Installs the global instance pointer and returns a stable reference.
///
/// # Safety
/// Must be called exactly once, before `init_window`.
pub unsafe fn set_instance(overlay: *mut Overlay) {
    INSTANCE.store(overlay, Ordering::SeqCst);
}
