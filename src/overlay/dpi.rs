//! DPI awareness, the layout scale, and keeping the panel on screen.

use crate::config::{MAX_SCALE_PERCENT, MIN_SCALE_PERCENT};

use super::*;

/// `MonitorFromWindow`: pick the monitor the window is nearest to.
const MONITOR_DEFAULTTONEAREST: u32 = 2;

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

/// The factor everything is laid out at: the monitor's DPI factor (1.0 at
/// 96 DPI, 1.5 at 150 % display scaling) multiplied by the user's setting, so
/// `100` means "follow Windows". A DPI below 48 means "no window yet" and
/// contributes nothing.
pub(super) fn scale_for(dpi: u32, percent: u32) -> f32 {
    let display = if dpi >= 48 { dpi as f32 / 96.0 } else { 1.0 };
    let user = percent.clamp(MIN_SCALE_PERCENT, MAX_SCALE_PERCENT) as f32 / 100.0;
    display * user
}

impl Overlay {
    /// The layout scale for the monitor the window is on.
    pub(super) fn effective_scale(&self) -> f32 {
        let hwnd = self.hwnd;
        // SAFETY: `GetDpiForWindow` on a live window, else the system DPI.
        let dpi = unsafe { if hwnd != 0 { GetDpiForWindow(hwnd) } else { 0 } };
        scale_for(dpi, self.config.scale_percent)
    }

    /// Re-lays out after the user changed the scale setting.
    pub(super) fn apply_scale(&mut self) {
        self.scale = self.effective_scale();
        self.canvas_key = None;
        self.metrics_key = None;
        self.refresh();
        self.keep_on_screen();
    }

    /// Re-renders at a new monitor DPI. The OS suggests a rectangle to keep the
    /// window on the same visual spot.
    pub(super) fn apply_dpi(&mut self, dpi: u32, x: i32, y: i32) {
        self.scale = scale_for(dpi, self.config.scale_percent);
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
        self.keep_on_screen();
    }

    /// Pulls the panel back inside its monitor, so growing it (or moving to a
    /// monitor with another scaling factor) can never strand it off screen.
    /// Does nothing when the `keep_on_screen` setting is off.
    pub(super) fn keep_on_screen(&mut self) {
        if self.hwnd == 0 || !self.config.keep_on_screen {
            return;
        }
        let mut rect = Rect::default();
        // SAFETY: read-only query about our own window.
        if unsafe { GetWindowRect(self.hwnd, &mut rect) } == 0 {
            return;
        }
        let (x, y) = self.clamped_position(
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        );
        if (x, y) == (rect.left, rect.top) {
            return;
        }
        // SAFETY: move our own window.
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
        self.config.position = Some([x as f32, y as f32]);
        self.config.save();
    }

    /// `(x, y)` pulled inside the monitor the window is on, for a panel of the
    /// given size. Returns the input unchanged when the monitor is unknown.
    pub(super) fn clamped_position(&self, x: i32, y: i32, width: i32, height: i32) -> (i32, i32) {
        if self.hwnd == 0 || width <= 0 || height <= 0 {
            return (x, y);
        }
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            ..MonitorInfo::default()
        };
        // SAFETY: read-only monitor queries.
        unsafe {
            let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
            if monitor == 0 || GetMonitorInfoW(monitor, &mut info) == 0 {
                return (x, y);
            }
        }
        clamp_to_area(x, y, width, height, info.monitor)
    }
}

/// The nearest origin inside `area` for a window of this size. A window larger
/// than the area is pinned to its top-left corner, so it never jumps around.
pub(super) fn clamp_to_area(x: i32, y: i32, width: i32, height: i32, area: Rect) -> (i32, i32) {
    (
        x.clamp(area.left, (area.right - width).max(area.left)),
        y.clamp(area.top, (area.bottom - height).max(area.top)),
    )
}

#[cfg(test)]
mod tests {
    use super::{clamp_to_area, scale_for};
    use crate::overlay::win32::Rect;

    fn area(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn scale_combines_display_scaling_and_the_user_setting() {
        // 100 % means "follow Windows": 96 DPI is 1.0, 144 DPI (150 %) is 1.5.
        assert_eq!(scale_for(96, 100), 1.0);
        assert_eq!(scale_for(144, 100), 1.5);
        // The user setting multiplies that.
        assert_eq!(scale_for(144, 150), 2.25);
        assert_eq!(scale_for(96, 50), 0.5);
        assert_eq!(scale_for(192, 175), 3.5);
        // No window yet: the display contributes nothing.
        assert_eq!(scale_for(0, 100), 1.0);
        assert_eq!(scale_for(0, 175), 1.75);
        // Out-of-range values are clamped, not honoured.
        assert_eq!(scale_for(96, 0), 0.25);
        assert_eq!(scale_for(96, 10_000), 4.0);
    }

    /// The whole panel has to stay inside the monitor, not just its origin.
    #[test]
    fn clamping_keeps_the_whole_panel_inside() {
        let monitor = area(0, 0, 2560, 1440);
        let (w, h) = (300, 200);

        // Already inside: untouched, including right up against the edges.
        assert_eq!(clamp_to_area(100, 100, w, h, monitor), (100, 100));
        assert_eq!(clamp_to_area(2260, 1240, w, h, monitor), (2260, 1240));

        // Past the right or bottom edge: the far edge is pulled in.
        assert_eq!(clamp_to_area(2500, 1300, w, h, monitor), (2260, 1240));
        assert_eq!(clamp_to_area(9999, 9999, w, h, monitor), (2260, 1240));

        // Past the left or top edge.
        assert_eq!(clamp_to_area(-50, -50, w, h, monitor), (0, 0));

        // A monitor to the left of the primary one has negative coordinates.
        let left_of_primary = area(-1920, 0, 0, 1080);
        assert_eq!(
            clamp_to_area(-2000, 500, w, h, left_of_primary),
            (-1920, 500)
        );
        assert_eq!(clamp_to_area(-100, 500, w, h, left_of_primary), (-300, 500));

        // A panel larger than the monitor is pinned to the top-left corner
        // instead of being pushed around.
        assert_eq!(clamp_to_area(500, 500, 3000, 2000, monitor), (0, 0));
        assert_eq!(clamp_to_area(-500, -500, 3000, 2000, monitor), (0, 0));
    }
}
