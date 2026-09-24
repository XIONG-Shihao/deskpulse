//! DPI awareness and per-monitor re-scaling.

use super::*;

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

impl Overlay {
    pub(super) fn query_scale(&self) -> f32 {
        let hwnd = self.hwnd;
        // SAFETY: `GetDpiForWindow` on a live window, else the system DPI.
        let dpi = unsafe { if hwnd != 0 { GetDpiForWindow(hwnd) } else { 0 } };
        if dpi >= 48 { dpi as f32 / 96.0 } else { 1.0 }
    }

    /// Re-renders at a new monitor DPI. The OS suggests a rectangle to keep the
    /// window on the same visual spot.
    pub(super) fn apply_dpi(&mut self, dpi: u32, x: i32, y: i32) {
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
