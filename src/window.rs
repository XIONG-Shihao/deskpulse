//! Small Win32 window helpers.

const GWL_EXSTYLE: i32 = -20;
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

/// Keeps the window out of the taskbar (and Alt+Tab) by giving it the tool
/// window style.
///
/// `eframe` does not forward `ViewportBuilder::with_taskbar` to winit, so the
/// window would show in the taskbar; winit also re-applies its own styles when
/// the window is shown, which reverts a one-off change. Hence this is called
/// every frame and only touches the window when the style is wrong.
pub fn ensure_tool_window(hwnd: isize) {
    // SAFETY: `hwnd` is a valid window handle from the running process.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = (current | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW;
        if wanted == current {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        // The style change only takes effect after a frame change.
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
