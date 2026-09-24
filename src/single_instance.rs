//! Keep one overlay per Windows logon session.
//!
//! Two translucent panels at the same position blend twice and appear to
//! change opacity as other windows move between them in the z-order.
//!
//! Limits of this guard:
//! - `Local\` names live in the logon session, so a second user session gets
//!   its own overlay, which is intended.
//! - If an instance was elevated with *different* credentials, its mutex may
//!   not be openable from here, so the `is_running` fast path can miss it.
//!   `acquire` still arbitrates for the normal same-user case.

use std::io;

const NAME: &str = "Local\\deskpulse.single-instance";
const SYNCHRONIZE: u32 = 0x0010_0000;
const ERROR_ALREADY_EXISTS: u32 = 183;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenMutexW(desired_access: u32, inherit_handle: i32, name: *const u16) -> isize;
    fn CreateMutexW(
        attributes: *const std::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> isize;
    fn GetLastError() -> u32;
    fn CloseHandle(object: isize) -> i32;
}

fn wide_name(name: &str) -> Vec<u16> {
    crate::wide::wide(name)
}

/// Fast pre-elevation check. `acquire` makes the final, atomic decision.
pub fn is_running() -> bool {
    is_running_named(NAME)
}

fn is_running_named(name: &str) -> bool {
    let name = wide_name(name);
    // SAFETY: `name` is NUL-terminated and remains alive through the call.
    let handle = unsafe { OpenMutexW(SYNCHRONIZE, 0, name.as_ptr()) };
    if handle == 0 {
        return false;
    }
    // SAFETY: `handle` was returned by OpenMutexW and is closed once.
    unsafe { CloseHandle(handle) };
    true
}

pub struct Instance(isize);

/// Atomically claim this session's overlay slot.
pub fn acquire() -> io::Result<Option<Instance>> {
    acquire_named(NAME)
}

fn acquire_named(name: &str) -> io::Result<Option<Instance>> {
    let name = wide_name(name);
    // SAFETY: null security attributes and a live, NUL-terminated name.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle == 0 {
        return Err(io::Error::last_os_error());
    }
    // GetLastError must be checked immediately after CreateMutexW.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if existed {
        // SAFETY: this launch does not own the only allowed instance.
        unsafe { CloseHandle(handle) };
        Ok(None)
    } else {
        Ok(Some(Instance(handle)))
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: the mutex handle is held for this process and closed once.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_cannot_acquire_until_first_exits() {
        let name = format!("Local\\deskpulse.test.{}", std::process::id());
        assert!(!is_running_named(&name));
        let first = acquire_named(&name).unwrap().unwrap();
        assert!(is_running_named(&name));
        assert!(acquire_named(&name).unwrap().is_none());
        drop(first);
        assert!(!is_running_named(&name));
        assert!(acquire_named(&name).unwrap().is_some());
    }
}
