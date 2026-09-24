//! Detect a game that really took a display away, the one case where the panel
//! cannot be shown at all.
//!
//! The panel is a per-pixel-alpha layered window, so it exists only where DWM
//! composites. When a game switches to a true exclusive fullscreen mode, DWM
//! stops compositing that display and hands it to the game — nothing external
//! can be drawn, layered or not, and no injection-free overlay survives it.
//! Runs that Windows merely *pretends* to make exclusive (DXGI flip-model
//! games, most DX11 titles and every DX12 title) keep DWM in the loop, which is
//! why the panel shows over some games' fullscreen mode and not others.
//!
//! `SHQueryUserNotificationState` reports `QUNS_RUNNING_D3D_FULL_SCREEN`
//! exactly for the first case: it is the same signal the shell uses to suppress
//! notifications. Measured on this machine, with the panel's visibility next to
//! it:
//!
//! | foreground | state | panel |
//! |---|---|---|
//! | League of Legends, 无边框 | 5 accepts notifications | visible |
//! | League of Legends, 全屏 | **3 D3D full screen** | **invisible** |
//! | Black Myth: Wukong, borderless | 2 busy | visible |
//! | Apex Legends (DX12), 全屏 | 2 busy | visible |

/// `QUERY_USER_NOTIFICATION_STATE::QUNS_RUNNING_D3D_FULL_SCREEN`.
const QUNS_RUNNING_D3D_FULL_SCREEN: u32 = 3;

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHQueryUserNotificationState(state: *mut u32) -> i32;
}

/// The shell's current notification state, or `None` if the query failed.
fn state() -> Option<u32> {
    let mut state: u32 = 0;
    // SAFETY: a read-only shell query that only writes the out parameter.
    let result = unsafe { SHQueryUserNotificationState(&mut state) };
    (result >= 0).then_some(state)
}

/// Whether a D3D application currently owns a display in exclusive fullscreen,
/// which means the panel is not being composited and cannot be seen.
pub fn exclusive_fullscreen() -> bool {
    state() == Some(QUNS_RUNNING_D3D_FULL_SCREEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The query itself has to work; a test run is never a fullscreen game, so
    /// the state is readable but never the exclusive D3D one.
    #[test]
    fn shell_answers_the_notification_state() {
        assert!(state().is_some());
        assert!(!exclusive_fullscreen());
    }
}
