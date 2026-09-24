//! Raw Win32 declarations: the FFI blocks and the value types they use.

use std::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Point {
    pub(super) x: i32,
    pub(super) y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Rect {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Size {
    pub(super) cx: i32,
    pub(super) cy: i32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct Msg {
    pub(super) hwnd: isize,
    pub(super) message: u32,
    pub(super) w_param: usize,
    pub(super) l_param: isize,
    pub(super) time: u32,
    pub(super) pt: Point,
}

#[repr(C)]
pub(super) struct WndClassW {
    pub(super) style: u32,
    pub(super) wnd_proc: Option<unsafe extern "system" fn(isize, u32, usize, isize) -> isize>,
    pub(super) cls_extra: i32,
    pub(super) wnd_extra: i32,
    pub(super) instance: isize,
    pub(super) icon: isize,
    pub(super) cursor: isize,
    pub(super) background: isize,
    pub(super) menu_name: *const u16,
    pub(super) class_name: *const u16,
}

#[repr(C)]
pub(super) struct BlendFunction {
    pub(super) blend_op: u8,
    pub(super) blend_flags: u8,
    pub(super) source_constant_alpha: u8,
    pub(super) alpha_format: u8,
}

#[repr(C)]
pub(super) struct BitmapInfoHeader {
    pub(super) size: u32,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) planes: u16,
    pub(super) bit_count: u16,
    pub(super) compression: u32,
    pub(super) size_image: u32,
    pub(super) x_pels_per_meter: i32,
    pub(super) y_pels_per_meter: i32,
    pub(super) clr_used: u32,
    pub(super) clr_important: u32,
}

#[repr(C)]
pub(super) struct RgbQuad {
    pub(super) blue: u8,
    pub(super) green: u8,
    pub(super) red: u8,
    pub(super) reserved: u8,
}

#[repr(C)]
pub(super) struct BitmapInfo {
    pub(super) header: BitmapInfoHeader,
    pub(super) colors: [RgbQuad; 1],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(super) fn GetModuleHandleW(name: *const u16) -> isize;
    pub(super) fn GetLastError() -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    pub(super) fn RegisterClassW(class: *const WndClassW) -> u16;
    pub(super) fn CreateWindowExW(
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
    pub(super) fn DefWindowProcW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize;
    pub(super) fn DestroyWindow(hwnd: isize) -> i32;
    pub(super) fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    pub(super) fn TranslateMessage(msg: *const Msg) -> i32;
    pub(super) fn DispatchMessageW(msg: *const Msg) -> isize;
    pub(super) fn PostQuitMessage(exit_code: i32);
    pub(super) fn PostMessageW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
    pub(super) fn SetWindowPos(
        hwnd: isize,
        after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    pub(super) fn GetWindowRect(hwnd: isize, rect: *mut Rect) -> i32;
    pub(super) fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
    pub(super) fn GetCursorPos(point: *mut Point) -> i32;
    pub(super) fn WindowFromPoint(point: Point) -> isize;
    pub(super) fn SetCapture(hwnd: isize) -> isize;
    pub(super) fn ReleaseCapture() -> i32;
    pub(super) fn ShowWindow(hwnd: isize, command: i32) -> i32;
    pub(super) fn LoadCursorW(instance: isize, name: isize) -> isize;
    pub(super) fn GetDpiForWindow(hwnd: isize) -> u32;
    pub(super) fn SetTimer(hwnd: isize, id: usize, elapse: u32, callback: isize) -> usize;
    pub(super) fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    pub(super) fn SetProcessDPIAware() -> i32;
    pub(super) fn UpdateLayeredWindow(
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
    pub(super) fn SetWinEventHook(
        event_min: u32,
        event_max: u32,
        module: isize,
        callback: Option<unsafe extern "system" fn(isize, u32, isize, i32, i32, u32, u32)>,
        process: u32,
        thread: u32,
        flags: u32,
    ) -> isize;
    pub(super) fn UnhookWinEvent(hook: isize) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    pub(super) fn CreateCompatibleDC(dc: isize) -> isize;
    pub(super) fn DeleteDC(dc: isize) -> i32;
    pub(super) fn GetTextExtentPoint32W(
        dc: isize,
        text: *const u16,
        len: i32,
        size: *mut Size,
    ) -> i32;
    pub(super) fn CreateDIBSection(
        dc: isize,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: isize,
        offset: u32,
    ) -> isize;
    pub(super) fn SelectObject(dc: isize, object: isize) -> isize;
    pub(super) fn DeleteObject(object: isize) -> i32;
    pub(super) fn SetBkMode(dc: isize, mode: i32) -> i32;
    pub(super) fn SetTextColor(dc: isize, color: u32) -> u32;
    pub(super) fn DrawTextW(
        dc: isize,
        text: *const u16,
        len: i32,
        rect: *mut Rect,
        flags: u32,
    ) -> i32;
    pub(super) fn CreateFontW(
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

#[link(name = "shell32")]
unsafe extern "system" {
    pub(super) fn SHQueryUserNotificationState(state: *mut i32) -> i32;
}
