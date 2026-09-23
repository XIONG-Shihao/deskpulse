//! The overlay: a single per-pixel-alpha Win32 layered window drawn entirely
//! with GDI.
//!
//! No GPU API is involved at all (no OpenGL/Direct3D/Vulkan), so it runs on any
//! x64 Windows, including machines with only the Microsoft Basic Display
//! adapter, virtual machines and remote sessions. The panel background and the
//! text are composited by us into one 32-bit DIB and handed to
//! `UpdateLayeredWindow`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tray_icon::menu::{
    CheckMenuItem, ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};

use crate::autostart::Autostart;
use crate::config::{Config, Layout, Spacing};
use crate::format;
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

// Logical layout units (scaled by the monitor DPI at draw time).
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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Size {
    cx: i32,
    cy: i32,
}

#[repr(C)]
#[derive(Default)]
struct Msg {
    hwnd: isize,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: Point,
}

#[repr(C)]
struct WndClassW {
    style: u32,
    wnd_proc: Option<unsafe extern "system" fn(isize, u32, usize, isize) -> isize>,
    cls_extra: i32,
    wnd_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu_name: *const u16,
    class_name: *const u16,
}

#[repr(C)]
struct BlendFunction {
    blend_op: u8,
    blend_flags: u8,
    source_constant_alpha: u8,
    alpha_format: u8,
}

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct RgbQuad {
    blue: u8,
    green: u8,
    red: u8,
    reserved: u8,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [RgbQuad; 1],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *mut c_void,
    ) -> isize;
    fn DefWindowProcW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize;
    fn DestroyWindow(hwnd: isize) -> i32;
    fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostQuitMessage(exit_code: i32);
    fn PostMessageW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
    fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32)
    -> i32;
    fn GetWindowRect(hwnd: isize, rect: *mut Rect) -> i32;
    fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn SetCapture(hwnd: isize) -> isize;
    fn ReleaseCapture() -> i32;
    fn ShowWindow(hwnd: isize, command: i32) -> i32;
    fn LoadCursorW(instance: isize, name: isize) -> isize;
    fn GetDpiForWindow(hwnd: isize) -> u32;
    fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn UpdateLayeredWindow(
        hwnd: isize,
        dst_dc: isize,
        dst: *const Point,
        size: *const Point,
        src_dc: isize,
        src: *const Point,
        color_key: u32,
        blend: *const BlendFunction,
        flags: u32,
    ) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: isize) -> isize;
    fn DeleteDC(dc: isize) -> i32;
    fn GetTextExtentPoint32W(dc: isize, text: *const u16, len: i32, size: *mut Size) -> i32;
    fn CreateDIBSection(
        dc: isize,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: isize,
        offset: u32,
    ) -> isize;
    fn SelectObject(dc: isize, object: isize) -> isize;
    fn DeleteObject(object: isize) -> i32;
    fn SetBkMode(dc: isize, mode: i32) -> i32;
    fn SetTextColor(dc: isize, color: u32) -> u32;
    fn DrawTextW(dc: isize, text: *const u16, len: i32, rect: *mut Rect, flags: u32) -> i32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> isize;
}

static INSTANCE: AtomicPtr<Overlay> = AtomicPtr::new(std::ptr::null_mut());

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Declares per-monitor-v2 DPI awareness so Windows never bitmap-stretches the
/// window; text is then rendered at native pixels and stays sharp. Must be
/// called before any window is created.
pub fn enable_dpi_awareness() {
    // SAFETY: process-wide setting, called once before window creation.
    let ok = unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            SetProcessDPIAware()
        } else {
            1
        }
    };
    crate::diag::log(&format!("dpi awareness set: {ok}"));
}

// Unified side of a display metric.
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

/// One drawable piece of text.
struct Span {
    text: String,
    rect: Rect,
    align: u32,
    color: u32,
    value: bool,
}

/// Cached GDI resources for a fixed bitmap size and font scale.
struct Canvas {
    memory_dc: isize,
    bitmap: isize,
    old_bitmap: isize,
    label_font: isize,
    value_font: isize,
    bits: *mut c_void,
}

impl Canvas {
    fn create(width: i32, height: i32, scale: f32) -> Option<Self> {
        // SAFETY: all handles are checked and released in `Drop`.
        unsafe {
            let screen = CreateCompatibleDC(0);
            if screen == 0 {
                return None;
            }
            let memory_dc = CreateCompatibleDC(screen);
            DeleteDC(screen);
            if memory_dc == 0 {
                return None;
            }
            let info = BitmapInfo {
                header: BitmapInfoHeader {
                    size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                    width,
                    height: -height,
                    planes: 1,
                    bit_count: 32,
                    compression: BI_RGB,
                    size_image: (width * height * 4) as u32,
                    x_pels_per_meter: 0,
                    y_pels_per_meter: 0,
                    clr_used: 0,
                    clr_important: 0,
                },
                colors: [RgbQuad {
                    blue: 0,
                    green: 0,
                    red: 0,
                    reserved: 0,
                }],
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(memory_dc, &info, DIB_RGB_COLORS, &mut bits, 0, 0);
            if bitmap == 0 || bits.is_null() {
                DeleteDC(memory_dc);
                return None;
            }
            let old_bitmap = SelectObject(memory_dc, bitmap);
            SetBkMode(memory_dc, TRANSPARENT);

            let face = wide("Microsoft YaHei");
            let label_font = CreateFontW(
                -(LABEL_SIZE * scale).round() as i32,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );
            let value_font = CreateFontW(
                -(VALUE_SIZE * scale).round() as i32,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );

            Some(Self {
                memory_dc,
                bitmap,
                old_bitmap,
                label_font,
                value_font,
                bits,
            })
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: objects were created together and are released once.
        unsafe {
            SelectObject(self.memory_dc, self.old_bitmap);
            DeleteObject(self.bitmap);
            if self.label_font != 0 {
                DeleteObject(self.label_font);
            }
            if self.value_font != 0 {
                DeleteObject(self.value_font);
            }
            DeleteDC(self.memory_dc);
        }
    }
}

/// A throwaway GDI DC with the two text fonts selected, used only to measure
/// how wide a label or value will render so the panel can hug its content.
struct TextMetrics {
    dc: isize,
    label_font: isize,
    value_font: isize,
    old_font: isize,
}

impl TextMetrics {
    fn create(scale: f32) -> Option<Self> {
        // SAFETY: handles are checked and released in `Drop`.
        unsafe {
            let dc = CreateCompatibleDC(0);
            if dc == 0 {
                return None;
            }
            let face = wide("Microsoft YaHei");
            let label_font = CreateFontW(
                -(LABEL_SIZE * scale).round() as i32,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );
            let value_font = CreateFontW(
                -(VALUE_SIZE * scale).round() as i32,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );
            let old_font = SelectObject(dc, label_font);
            Some(Self {
                dc,
                label_font,
                value_font,
                old_font,
            })
        }
    }

    /// Width in physical pixels of `text` in the label or value font.
    fn width(&self, text: &str, value: bool) -> f32 {
        // SAFETY: `self.dc` is live; the font handles belong to this DC.
        unsafe {
            SelectObject(
                self.dc,
                if value {
                    self.value_font
                } else {
                    self.label_font
                },
            );
            let mut units: Vec<u16> = text.encode_utf16().collect();
            let mut size = Size::default();
            GetTextExtentPoint32W(self.dc, units.as_mut_ptr(), units.len() as i32, &mut size);
            size.cx as f32
        }
    }
}

impl Drop for TextMetrics {
    fn drop(&mut self) {
        // SAFETY: objects were created together and are released once.
        unsafe {
            SelectObject(self.dc, self.old_font);
            if self.label_font != 0 {
                DeleteObject(self.label_font);
            }
            if self.value_font != 0 {
                DeleteObject(self.value_font);
            }
            DeleteDC(self.dc);
        }
    }
}

pub struct Overlay {
    hwnd: isize,
    config: Config,
    layout: Layout,
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
}

impl Overlay {
    pub fn new(config: Config) -> Self {
        let autostart = Autostart::new();
        let mut config = config;
        config.autostart = autostart.is_enabled();
        let language = config.language.unwrap_or(Language::Zh);
        Self {
            hwnd: 0,
            layout: config.layout,
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
    }

    fn rows(&self, snapshot: &Snapshot) -> Vec<(&'static str, String)> {
        let t = self.text();
        Metric::ALL
            .iter()
            .filter(|metric| self.is_visible_metric(**metric))
            .map(|metric| (metric.label(&t), metric.value(snapshot)))
            .collect()
    }

    // ------------------------------------------------------------- window

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
            crate::diag::log("overlay window ready (win32/gdi)");
        }

        self.scale = self.query_scale();
        self.tray = Tray::new(self.config.autostart, &self.language.text());
        self.install_menu_handler();
        self.refresh();
    }

    fn query_scale(&self) -> f32 {
        let hwnd = self.hwnd;
        // SAFETY: `GetDpiForWindow` on a live window, else the system DPI.
        let dpi = unsafe { if hwnd != 0 { GetDpiForWindow(hwnd) } else { 0 } };
        if dpi >= 48 { dpi as f32 / 96.0 } else { 1.0 }
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

    /// Vertical distance between rows for the current spacing preset.
    fn row_gap(&self) -> f32 {
        match self.config.spacing {
            Spacing::Tight => 1.0,
            Spacing::Loose => 2.0,
        }
    }

    /// Rendered name↔value gap in whole physical pixels: 2pt rounded **up**, so
    /// it is never smaller than 2pt on any DPI.
    fn name_gap(&self) -> i32 {
        (NAME_GAP_PT * PT * self.scale).ceil() as i32
    }

    /// Physical size of the panel that exactly fits the given content widths.
    /// All geometry is in whole pixels so the spans and the panel agree.
    fn content_size(&self, rows: usize, label_w: f32, value_w: f32) -> (i32, i32) {
        let s = self.scale;
        let m = (MARGIN * s).round() as i32;
        let gap = self.name_gap();
        let col_gap = (COLUMN_GAP * s).round() as i32;
        let row_h = (ROW_H * s).round() as i32;
        let row_gap = (self.row_gap() * s).round() as i32;
        let label_w = label_w.round() as i32;
        let value_w = value_w.round() as i32;
        let count = rows.max(1) as i32;
        match self.layout {
            Layout::Vertical => (
                m * 2 + label_w + gap + value_w,
                m * 2 + count * row_h + (count - 1) * row_gap,
            ),
            Layout::Grid => {
                let lines = (count + 1) / 2;
                (
                    m * 2 + 2 * (label_w + gap + value_w),
                    m * 2 + lines * row_h + (lines - 1) * row_gap,
                )
            }
            Layout::Horizontal => {
                let col = label_w.max(value_w);
                (
                    m * 2 + count * col + (count - 1) * col_gap,
                    m * 2 + 2 * row_h + gap,
                )
            }
        }
    }

    fn build_spans(
        &self,
        rows: &[(&'static str, String)],
        label_w: f32,
        value_w: f32,
    ) -> Vec<Span> {
        let tight = self.config.spacing == Spacing::Tight;
        let s = self.scale;
        let m = (MARGIN * s).round() as i32;
        let gap = self.name_gap();
        let col_gap = (COLUMN_GAP * s).round() as i32;
        let row_h = (ROW_H * s).round() as i32;
        let row_gap = (self.row_gap() * s).round() as i32;
        let label_w = label_w.round() as i32;
        let value_w = value_w.round() as i32;

        let mut spans = Vec::new();
        let mut cell =
            |x: i32, y: i32, label: &str, value: &str, label_align: u32, value_align: u32| {
                let label_right = x + label_w;
                let value_left = label_right + gap;
                spans.push(Span {
                    text: label.to_owned(),
                    rect: Rect {
                        left: x,
                        top: y,
                        right: label_right,
                        bottom: y + row_h,
                    },
                    align: label_align,
                    color: 0x96_96_96,
                    value: false,
                });
                spans.push(Span {
                    text: value.to_owned(),
                    rect: Rect {
                        left: value_left,
                        top: y,
                        right: value_left + value_w,
                        bottom: y + row_h,
                    },
                    align: value_align,
                    color: 0x00FF_FFFF,
                    value: true,
                });
            };

        match self.layout {
            Layout::Vertical => {
                // Name flush left, 2pt gap, value immediately after.
                for (index, (label, value)) in rows.iter().enumerate() {
                    let y = m + index as i32 * (row_h + row_gap);
                    cell(m, y, label, value, 0, 0);
                }
            }
            Layout::Grid => {
                let cell_w = label_w + gap + value_w;
                for (index, (label, value)) in rows.iter().enumerate() {
                    let column = (index % 2) as i32;
                    let line = (index / 2) as i32;
                    let y = m + line * (row_h + row_gap);
                    let x = m + column * cell_w;
                    let label_align = if !tight {
                        DT_CENTER
                    } else if column == 1 {
                        0
                    } else {
                        DT_RIGHT
                    };
                    let value_align = if tight { 0 } else { DT_CENTER };
                    cell(x, y, label, value, label_align, value_align);
                }
            }
            Layout::Horizontal => {
                let col = label_w.max(value_w);
                for (index, (label, value)) in rows.iter().enumerate() {
                    let x = m + index as i32 * (col + col_gap);
                    spans.push(Span {
                        text: (*label).to_owned(),
                        rect: Rect {
                            left: x,
                            top: m,
                            right: x + col,
                            bottom: m + row_h,
                        },
                        align: DT_CENTER,
                        color: 0x96_96_96,
                        value: false,
                    });
                    spans.push(Span {
                        text: value.to_owned(),
                        rect: Rect {
                            left: x,
                            top: m + row_h + gap,
                            right: x + col,
                            bottom: m + 2 * row_h + gap,
                        },
                        align: DT_CENTER,
                        color: 0x00FF_FFFF,
                        value: true,
                    });
                }
            }
        }
        spans
    }

    // -------------------------------------------------------------- draw

    fn refresh(&mut self) {
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
        let (label_w, value_w) = match self.metrics.as_ref() {
            Some(metrics) => (
                rows.iter()
                    .map(|(label, _)| metrics.width(label, false))
                    .fold(0.0_f32, f32::max)
                    .ceil(),
                rows.iter()
                    .map(|(_, value)| metrics.width(value, true))
                    .fold(0.0_f32, f32::max)
                    .ceil(),
            ),
            None => (46.0 * self.scale, 74.0 * self.scale),
        };

        let (desired_w, height) = self.content_size(rows.len(), label_w, value_w);
        // Grow immediately when the content needs room, but shrink only once the
        // content is clearly narrower, so the panel does not twitch as digits
        // change every tick.
        let mut width = desired_w;
        if self.size.0 > desired_w && (self.size.0 - desired_w) as f32 <= 10.0 * self.scale {
            width = self.size.0;
        }
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

    fn paint(&mut self, spans: &[Span]) {
        if self.hwnd == 0 || !self.visible {
            return;
        }
        let scale = self.scale;

        // SAFETY: all GDI objects are used on the UI thread.
        unsafe {
            let mut client = Rect::default();
            if GetClientRect(self.hwnd, &mut client) == 0 {
                return;
            }
            let width = client.right - client.left;
            let height = client.bottom - client.top;
            if width <= 0 || height <= 0 {
                return;
            }

            let key = (width, height, (scale * 1000.0) as i32);
            if self.canvas_key != Some(key) {
                self.canvas = Canvas::create(width, height, scale);
                self.canvas_key = Some(key);
                if self.canvas.is_none() {
                    return;
                }
            }
            let Some(canvas) = self.canvas.as_ref() else {
                return;
            };

            let alpha = (self.config.opacity.clamp(0.0, 1.0) * 255.0) as u8;
            let pixels = width as usize * height as usize;
            let data = canvas.bits as *mut u8;
            let radius = (10.0 * scale).round() as i32;
            for y in 0..height {
                for x in 0..width {
                    let at = (y as usize * width as usize + x as usize) * 4;
                    if inside_rounded(x, y, width, height, radius) {
                        // black panel, translucent
                        *data.add(at) = 0;
                        *data.add(at + 1) = 0;
                        *data.add(at + 2) = 0;
                        *data.add(at + 3) = alpha;
                    } else {
                        *data.add(at + 3) = 0;
                    }
                }
            }

            for span in spans {
                let font = if span.value {
                    canvas.value_font
                } else {
                    canvas.label_font
                };
                if font == 0 {
                    continue;
                }
                SelectObject(canvas.memory_dc, font);
                SetTextColor(canvas.memory_dc, span.color);
                let mut rect = span.rect;
                let flags = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | span.align;
                let mut text: Vec<u16> = span.text.encode_utf16().collect();
                DrawTextW(
                    canvas.memory_dc,
                    text.as_mut_ptr(),
                    text.len() as i32,
                    &mut rect,
                    flags,
                );
            }

            // Opaque text: any non-black pixel gets full alpha.
            for index in 0..pixels {
                let at = index * 4;
                if *data.add(at) != 0 || *data.add(at + 1) != 0 || *data.add(at + 2) != 0 {
                    *data.add(at + 3) = 255;
                }
            }

            let mut window_rect = Rect::default();
            GetWindowRect(self.hwnd, &mut window_rect);
            let position = Point {
                x: window_rect.left,
                y: window_rect.top,
            };
            let size = Point {
                x: width,
                y: height,
            };
            let src = Point { x: 0, y: 0 };
            let blend = BlendFunction {
                blend_op: 0,
                blend_flags: 0,
                source_constant_alpha: 255,
                alpha_format: AC_SRC_ALPHA,
            };
            let result = UpdateLayeredWindow(
                self.hwnd,
                0,
                &position,
                &size,
                canvas.memory_dc,
                &src,
                0,
                &blend,
                ULW_ALPHA,
            );
            let _ = result;
        }
    }

    // ------------------------------------------------------------- menu

    fn install_menu_handler(&self) {
        let hwnd = self.hwnd;
        let queue = Arc::clone(&self.menu_ids);
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Ok(mut queue) = queue.lock() {
                queue.push(event.id.0.clone());
            }
            // SAFETY: posting to our own window.
            unsafe {
                PostMessageW(hwnd, WM_APP_MENU, 0, 0);
            }
        }));
    }

    fn show_context_menu(&self) {
        let t = self.text();
        let menu = Menu::new();
        let show = Submenu::new(t.metrics, true);
        for metric in Metric::ALL {
            let item = CheckMenuItem::with_id(
                metric.id(),
                metric.label(&t),
                true,
                self.is_visible_metric(metric),
                None,
            );
            let _ = show.append(&item);
        }
        let _ = menu.append(&show);

        let layout = Submenu::new(t.layout, true);
        for (id, label, selected) in [
            (
                "layout_vertical",
                t.vertical,
                self.layout == Layout::Vertical,
            ),
            (
                "layout_horizontal",
                t.horizontal,
                self.layout == Layout::Horizontal,
            ),
            ("layout_grid", t.grid, self.layout == Layout::Grid),
        ] {
            let _ = layout.append(&CheckMenuItem::with_id(id, label, true, selected, None));
        }
        let _ = menu.append(&layout);

        let spacing = Submenu::new(t.spacing, true);
        for (id, label, selected) in [
            (
                "spacing_loose",
                t.spacing_loose,
                self.config.spacing == Spacing::Loose,
            ),
            (
                "spacing_tight",
                t.spacing_tight,
                self.config.spacing == Spacing::Tight,
            ),
        ] {
            let _ = spacing.append(&CheckMenuItem::with_id(id, label, true, selected, None));
        }
        let _ = menu.append(&spacing);

        let language = Submenu::new(t.language, true);
        let _ = language.append(&CheckMenuItem::with_id(
            "lang_zh",
            "中文",
            true,
            self.language == Language::Zh,
            None,
        ));
        let _ = language.append(&CheckMenuItem::with_id(
            "lang_en",
            "English",
            true,
            self.language == Language::En,
            None,
        ));
        let _ = menu.append(&language);

        let _ = menu.append(&CheckMenuItem::with_id(
            "autostart",
            t.autostart,
            true,
            self.config.autostart,
            None,
        ));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("quit", t.quit, true, None));

        // SAFETY: called on the thread that owns the window.
        unsafe {
            menu.show_context_menu_for_hwnd(self.hwnd, None);
        }
    }

    fn apply_menu_ids(&mut self) {
        let ids = self
            .menu_ids
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();
        for id in ids {
            match id.as_str() {
                "toggle" => self.toggle_visible(),
                "layout_v" | "layout_vertical" => self.set_layout(Layout::Vertical),
                "layout_h" | "layout_horizontal" => self.set_layout(Layout::Horizontal),
                "layout_grid" => self.set_layout(Layout::Grid),
                "spacing_loose" => self.set_spacing(Spacing::Loose),
                "spacing_tight" => self.set_spacing(Spacing::Tight),
                "lang_zh" => self.set_language(Language::Zh),
                "lang_en" => self.set_language(Language::En),
                "autostart" => {
                    let enabled = !self.config.autostart;
                    self.set_autostart(enabled);
                }
                "quit" => {
                    // SAFETY: destroy our own window.
                    unsafe { DestroyWindow(self.hwnd) };
                }
                other => {
                    if let Some(metric) = Metric::from_id(other) {
                        let visible = !self.is_visible_metric(metric);
                        self.set_visible_metric(metric, visible);
                    }
                }
            }
        }
    }

    fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        // SAFETY: show/hide our own window.
        unsafe {
            ShowWindow(
                self.hwnd,
                if self.visible {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_HIDE
                },
            );
        }
    }

    fn set_layout(&mut self, layout: Layout) {
        if self.layout == layout {
            return;
        }
        self.layout = layout;
        self.config.layout = layout;
        self.config.save();
        self.refresh();
    }

    fn set_spacing(&mut self, spacing: Spacing) {
        if self.config.spacing == spacing {
            return;
        }
        self.config.spacing = spacing;
        self.config.save();
        self.refresh();
    }

    fn set_language(&mut self, language: Language) {
        if self.language == language {
            return;
        }
        self.language = language;
        self.config.language = Some(language);
        self.config.save();
        self.tray = Tray::new(self.config.autostart, &language.text());
        self.install_menu_handler();
        self.refresh();
    }

    fn set_autostart(&mut self, enabled: bool) {
        if self.autostart.set(enabled) {
            self.config.autostart = enabled;
        }
        self.config.save();
        if let Some(tray) = self.tray.as_ref() {
            tray.set_autostart_checked(self.config.autostart);
        }
    }

    // ------------------------------------------------------------ messages

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

    /// Re-renders at a new monitor DPI. The OS suggests a rectangle to keep the
    /// window on the same visual spot.
    fn apply_dpi(&mut self, dpi: u32, x: i32, y: i32) {
        self.scale = if dpi >= 48 { dpi as f32 / 96.0 } else { 1.0 };
        self.canvas_key = None;
        self.metrics_key = None;
        self.config.position = Some([x as f32, y as f32]);
        // SAFETY: move our own window to the suggested origin.
        unsafe {
            SetWindowPos(
                self.hwnd,
                0,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        self.refresh();
    }
}

/// True if the pixel lies inside a rounded rectangle of the given radius.
fn inside_rounded(x: i32, y: i32, width: i32, height: i32, radius: i32) -> bool {
    let radius = radius.min(width / 2).min(height / 2).max(0);
    if radius == 0 {
        return true;
    }
    let cx = if x < radius {
        radius
    } else if x >= width - radius {
        width - radius - 1
    } else {
        return true;
    };
    let cy = if y < radius {
        radius
    } else if y >= height - radius {
        height - radius - 1
    } else {
        return true;
    };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= radius * radius
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
