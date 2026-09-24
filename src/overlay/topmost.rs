//! Keep the overlay above other topmost windows.
//!
//! Two things can put a window above us without the user switching apps: a game
//! or app that makes itself topmost when it goes fullscreen, and other overlay
//! helpers. A foreground hook alone misses those, so a timer re-checks.

use std::sync::atomic::Ordering;

use super::*;

/// A `WM_APP_TOPMOST` is already queued, so a burst of foreground changes
/// collapses into a single z-order fix.
pub(super) static TOPMOST_PENDING: AtomicBool = AtomicBool::new(false);

/// Timer that periodically re-checks the z-order (see `WM_TIMER`).
pub(super) const TOPMOST_TIMER: usize = 1;
pub(super) const TOPMOST_INTERVAL_MS: u32 = 2000;

/// `SHQueryUserNotificationState`: an exclusive-fullscreen D3D app is running.
const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;

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

/// True while an exclusive-fullscreen app owns the display. Windows does not
/// composite any other window over it, so nothing we do can make us visible.
fn is_exclusive_fullscreen() -> bool {
    let mut state = 0;
    // SAFETY: a read-only shell query.
    unsafe { SHQueryUserNotificationState(&mut state) };
    state == QUNS_RUNNING_D3D_FULL_SCREEN
}

impl Overlay {
    /// Puts the overlay back on top when something covers it.
    ///
    /// Called after a foreground change, after a display-mode change and by the
    /// watchdog timer. It only acts when a window that covers at least half of
    /// the panel sits above us, so small transient windows (tooltips, menus) do
    /// not cause churn.
    pub(super) fn ensure_topmost(&mut self) {
        if self.hwnd == 0 || !self.visible {
            return;
        }
        let mut rect = Rect::default();
        // SAFETY: queries about our own window.
        if unsafe { GetWindowRect(self.hwnd, &mut rect) } == 0 {
            return;
        }
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return;
        }

        let Some(_covering) = self.covering_window(&rect) else {
            self.topmost_covered = false;
            return;
        };

        // An exclusive-fullscreen app owns the display; Windows composites
        // nothing on top of it, so raising ourselves would only fight the game.
        let exclusive = is_exclusive_fullscreen();
        if !self.topmost_covered {
            self.topmost_covered = true;
            if exclusive {
                crate::diag::log(
                    "overlay cannot be shown over an exclusive-fullscreen app \
                     (run the game in borderless/windowed mode)",
                );
            } else {
                crate::diag::log("overlay covered by another window: restoring topmost");
            }
        }
        if exclusive {
            return;
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

    /// A window above us that covers at least half of the panel, if any.
    fn covering_window(&self, ours: &Rect) -> Option<isize> {
        let (width, height) = (ours.right - ours.left, ours.bottom - ours.top);
        let mid_x = ours.left + width / 2;
        let mid_y = ours.top + height / 2;
        // Sample well inside the panel body: per-pixel-alpha hit testing treats
        // the transparent rounded corners as "not our window", so they would
        // look covered even when nothing is above us.
        let samples = [
            (mid_x, mid_y),
            (mid_x, ours.top + height / 4),
            (mid_x, ours.top + height * 3 / 4),
            (ours.left + width / 4, mid_y),
            (ours.left + width * 3 / 4, mid_y),
        ];
        // SAFETY: read-only queries about other windows.
        unsafe {
            samples.iter().find_map(|&(x, y)| {
                let hit = WindowFromPoint(Point { x, y });
                if hit == 0 || hit == self.hwnd {
                    return None;
                }
                let mut theirs = Rect::default();
                if GetWindowRect(hit, &mut theirs) == 0 {
                    return None;
                }
                let overlap = (ours.right.min(theirs.right) - ours.left.max(theirs.left))
                    * (ours.bottom.min(theirs.bottom) - ours.top.max(theirs.top));
                (overlap * 2 >= width * height).then_some(hit)
            })
        }
    }
}
