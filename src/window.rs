//! Small Win32 window helpers.

const GWL_STYLE: i32 = -16;
const GWL_EXSTYLE: i32 = -20;

const WS_CAPTION: isize = 0x00C0_0000;
const WS_THICKFRAME: isize = 0x0004_0000;
const WS_SYSMENU: isize = 0x0008_0000;
const WS_MINIMIZEBOX: isize = 0x0002_0000;
const WS_MAXIMIZEBOX: isize = 0x0001_0000;

const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
const WS_EX_APPWINDOW: isize = 0x0004_0000;

const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_FRAMECHANGED: u32 = 0x0020;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowLongPtrW(window: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(window: isize, index: i32, value: isize) -> isize;
    fn SetWindowPos(
        window: isize,
        insert_after: isize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
}

/// Enforces the overlay's native window style:
/// - hidden from the taskbar and Alt+Tab (`WS_EX_TOOLWINDOW`, no `WS_EX_APPWINDOW`)
/// - no title bar or system buttons (`WS_CAPTION`, `WS_SYSMENU`, min/max, frame)
///
/// eframe does not forward `ViewportBuilder::with_taskbar` / `with_decorations`
/// to winit, and winit re-applies its own styles when the window is shown, so
/// this is called every frame and only touches the window when it is wrong.
pub fn ensure_overlay_style(hwnd: isize) {
    // SAFETY: `hwnd` is a valid window handle from the running process.
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);

        let wanted_ex = (ex_style | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
        let wanted_style = style
            & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);

        if wanted_ex == ex_style && wanted_style == style {
            return;
        }
        if wanted_ex != ex_style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted_ex);
        }
        if wanted_style != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, wanted_style);
        }
        // Style changes only take effect after a frame change.
        SetWindowPos(
            hwnd,
            0,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}
