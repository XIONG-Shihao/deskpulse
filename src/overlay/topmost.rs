//! Keep the overlay above other topmost windows.

use std::sync::atomic::Ordering;

use super::*;

/// A `WM_APP_TOPMOST` is already queued, so a burst of foreground changes
/// collapses into a single z-order fix.
pub(super) static TOPMOST_PENDING: AtomicBool = AtomicBool::new(false);

pub(super) unsafe extern "system" fn foreground_changed(
    _hook: isize,
    _event: u32,
    foreground: isize,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let hwnd = OVERLAY_HWND.load(Ordering::Relaxed);
    if hwnd != 0 && foreground != hwnd && !TOPMOST_PENDING.swap(true, Ordering::AcqRel) {
        // Let the foreground transition finish before restoring our z-order.
        // The handler clears the flag, so at most one message is in flight.
        unsafe { PostMessageW(hwnd, WM_APP_TOPMOST, 0, 0) };
    }
}

impl Overlay {
    /// Raises the overlay back above another topmost window that took the
    /// foreground. Runs after a foreground change and only acts when a sample
    /// point inside the panel is genuinely covered by someone else.
    pub(super) fn ensure_topmost(&mut self) {
        if self.hwnd == 0 || !self.visible {
            return;
        }
        let mut rect = Rect::default();
        // SAFETY: queries about our own window.
        if unsafe { GetWindowRect(self.hwnd, &mut rect) } == 0 {
            return;
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return;
        }

        // Sample well inside the panel body: per-pixel-alpha hit testing treats
        // the transparent rounded corners as "not our window", so they would
        // look covered even when nothing is above us.
        let mid_x = rect.left + width / 2;
        let mid_y = rect.top + height / 2;
        let samples = [
            (mid_x, mid_y),
            (mid_x, rect.top + height / 4),
            (mid_x, rect.top + height * 3 / 4),
            (rect.left + width / 4, mid_y),
            (rect.left + width * 3 / 4, mid_y),
        ];
        // SAFETY: `WindowFromPoint` is a read-only query.
        let covered = unsafe {
            samples.iter().any(|&(x, y)| {
                let hit = WindowFromPoint(Point { x, y });
                hit != 0 && hit != self.hwnd
            })
        };
        if !covered {
            self.topmost_covered = false;
            return;
        }
        if !self.topmost_covered {
            self.topmost_covered = true;
            crate::diag::log("overlay covered by another window: restoring topmost");
        }

        // SAFETY: change only our window's z-order, without moving it or
        // taking focus from the foreground application.
        let ok = unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        if ok == 0 {
            crate::diag::log("failed to restore overlay topmost position");
        }
    }
}
