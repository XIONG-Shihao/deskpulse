//! Self-elevation.
//!
//! CPU temperature needs the PawnIO kernel driver, whose device only accepts
//! requests from an elevated process. Launching the app normally would silently
//! lose temperature, so on startup we relaunch ourselves elevated (one UAC
//! prompt) when needed. When started by the logon scheduled task we are already
//! elevated and this does nothing.

use std::ffi::c_void;

use crate::wide::wide;

const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_ELEVATION: u32 = 20;
const SW_SHOWNORMAL: i32 = 1;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn CloseHandle(object: isize) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: isize, desired_access: u32, token: *mut isize) -> i32;
    fn GetTokenInformation(
        token: isize,
        class: u32,
        info: *mut c_void,
        info_len: u32,
        return_len: *mut u32,
    ) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: isize,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}

fn is_elevated() -> bool {
    // SAFETY: all handles are valid, buffers sized to the requested class.
    unsafe {
        let mut token: isize = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevated: u32 = 0;
        let mut returned: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TOKEN_ELEVATION,
            &mut elevated as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
            &mut returned,
        );
        CloseHandle(token);
        ok != 0 && elevated != 0
    }
}

/// Relaunches this executable elevated. Returns true if a new elevated process
/// was started (the caller should then exit).
pub fn ensure_elevated() -> bool {
    if is_elevated() {
        return false;
    }

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };

    let operation = wide("runas");
    let file = wide(&exe.to_string_lossy());
    // SAFETY: valid NUL-terminated strings, no parameters, no working dir.
    let result = unsafe {
        ShellExecuteW(
            0,
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    // ShellExecute returns a value > 32 on success.
    result > 32
}
