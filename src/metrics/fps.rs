//! Frame rate of the foreground application, measured through ETW.
//!
//! No window can be composited over a game that runs in exclusive fullscreen,
//! but the game itself keeps submitting frames, so we subscribe to
//! `Microsoft-Windows-DXGI` and count its `Present` events (event id 42; the
//! stop event is 43) for the process that owns the foreground window. That
//! works in every display mode, including exclusive fullscreen.
//!
//! The event id was confirmed against the provider's own metadata
//! (`wevtutil gp Microsoft-Windows-DXGI`): task `Present`, opcode `win:Start`.
//! A recorded trace of a UE5 game shows why counting *all* DXGI events would be
//! wrong: its 1886 events in 10 s came in start/stop pairs, but only 180 were
//! `Present` (id 42) — the rest were `IDXGISwapChain_GetDesc`,
//! `GetFullscreenState` and `IDXGISwapChain_Present` calls. That is 18 fps, not
//! the 94 fps the raw event count suggests.
//!
//! Known limits, both honest `--` rather than a wrong number:
//! * Games that present through another runtime (Vulkan, D3D9) do not raise
//!   DXGI events; they need the DxgKrnl/D3D9 providers, which are far noisier.
//! * A real-time ETW session needs administrator rights. deskpulse has them
//!   because it self-elevates for the CPU temperature driver; without them the
//!   collector is simply not created.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

/// Session name is ours; anything stale with this name is stopped first.
const SESSION_NAME: &str = "deskpulse-fps";

/// `Microsoft-Windows-DXGI` (confirmed with `logman query providers`).
const DXGI_PROVIDER: Guid = Guid {
    data1: 0xCA11_C036,
    data2: 0x0102,
    data3: 0x4A2D,
    data4: [0xA6, 0xAD, 0xF0, 0x3C, 0xFE, 0xD5, 0xD3, 0xC9],
};

/// `Microsoft-Windows-DXGI` event id for task `Present`, opcode `win:Start`:
/// one per `IDXGISwapChain::Present` call.
const PRESENT_START_ID: u16 = 42;

/// Readings kept to compute the rate. A real-time ETW session hands its buffers
/// over on a timer (one second is the smallest it allows) instead of
/// continuously, so a rate measured over a single refresh interval beats
/// against that delivery and swings wildly — a steady 100 fps game reads 0 and
/// 200 on alternating samples. Averaging over the retained window cancels the
/// lumps: at the default one-second refresh this is a three-second window.
const RATE_WINDOW: usize = 4;

const PROCESS_TRACE_MODE_REAL_TIME: u32 = 0x0000_0100;
const PROCESS_TRACE_MODE_EVENT_RECORD: u32 = 0x1000_0000;
const EVENT_TRACE_REAL_TIME_MODE: u32 = 0x0000_0100;
const EVENT_CONTROL_CODE_ENABLE_PROVIDER: u32 = 1;
const EVENT_TRACE_CONTROL_STOP: u32 = 1;
const TRACE_LEVEL_VERBOSE: u8 = 5;
const WNODE_FLAG_TRACED_GUID: u32 = 0x0002_0000;
/// `Wnode.ClientContext`: 1 = query performance counter timestamps.
const CLIENT_CONTEXT_QPC: u32 = 1;
const ERROR_SUCCESS: u32 = 0;
const ERROR_ALREADY_EXISTS: u32 = 183;
const INVALID_PROCESSTRACE_HANDLE: u64 = u64::MAX;

/// Which process to count presents for; 0 = none. Written by the collector
/// thread, read by the ETW consumer thread.
static TARGET_PID: AtomicU32 = AtomicU32::new(0);
/// `Present` events seen for `TARGET_PID` since it was last set.
static PRESENTS: AtomicUsize = AtomicUsize::new(0);
/// Handle of the running session, so it can be stopped on exit.
static SESSION: AtomicU64 = AtomicU64::new(0);
/// Diagnostic: set once the consumer has delivered any DXGI event, which proves
/// the real-time pipeline (session, provider, consumer, record layout) works.
static FIRST_EVENT_LOGGED: AtomicBool = AtomicBool::new(false);
/// Diagnostic: set once a `Present` was counted for the watched process.
static FIRST_PRESENT_LOGGED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WnodeHeader {
    buffer_size: u32,
    provider_id: u32,
    historical_context: u64,
    timestamp: i64,
    guid: Guid,
    client_context: u32,
    flags: u32,
}

/// `EVENT_TRACE_PROPERTIES`; the session name is written right after it.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EventTraceProperties {
    wnode: WnodeHeader,
    /// Size of each buffer in kilobytes (the field is confusingly named).
    buffer_size: u32,
    minimum_buffers: u32,
    maximum_buffers: u32,
    maximum_file_size: u32,
    log_file_mode: u32,
    flush_timer: u32,
    enable_flags: u32,
    age_limit: i32,
    number_of_buffers: u32,
    free_buffers: u32,
    events_lost: u32,
    buffers_written: u32,
    log_buffers_lost: u32,
    real_time_buffers_lost: u32,
    logger_thread_id: u64,
    log_file_name_offset: u32,
    logger_name_offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EventDescriptor {
    id: u16,
    version: u8,
    channel: u8,
    level: u8,
    opcode: u8,
    task: u16,
    keyword: u64,
}

/// `EVENT_HEADER`, 80 bytes on 64-bit Windows. The fields we read —
/// `process_id`, `provider_id`, `descriptor.id` — sit at offsets 12, 24 and 40.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EventHeader {
    size: u16,
    header_type: u16,
    flags: u16,
    event_property: u16,
    thread_id: u32,
    process_id: u32,
    timestamp: i64,
    provider_id: Guid,
    descriptor: EventDescriptor,
    processor_time: u64,
    activity_id: Guid,
}

/// `EVENT_RECORD`.
#[repr(C)]
#[derive(Clone, Copy)]
struct EventRecord {
    header: EventHeader,
    buffer_context: u32,
    extended_data_count: u16,
    user_data_length: u16,
    extended_data: *mut c_void,
    user_data: *mut c_void,
    user_context: *mut c_void,
    descriptor: EventDescriptor,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TraceLogfileHeader {
    buffer_size: u32,
    version: u32,
    provider_version: u32,
    number_of_processors: u32,
    end_time: i64,
    timer_resolution: u32,
    maximum_file_size: u32,
    log_file_mode: u32,
    buffers_written: u32,
    log_instance_guid: Guid,
    logger_name: *mut u16,
    log_file_name: *mut u16,
    time_zone: [u8; 172],
    boot_time: i64,
    perf_freq: i64,
    start_time: i64,
    reserved_flags: u32,
    buffers_lost: u32,
}

impl Default for TraceLogfileHeader {
    fn default() -> Self {
        // SAFETY: all-zero is the documented "nothing set yet" state.
        unsafe { std::mem::zeroed() }
    }
}

/// `EVENT_TRACE_HEADER`, the legacy 48-byte header. `EVENT_TRACE` (used inside
/// `EventTraceLogfileW`) still uses this one, even though the records handed to
/// the callback use the 80-byte `EventHeader`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TraceEventHeader {
    size: u16,
    field_type: u16,
    version: u32,
    thread_id: u32,
    process_id: u32,
    timestamp: i64,
    guid: Guid,
    kernel_time: u32,
    user_time: u32,
}

/// `EVENT_TRACE`; only its size matters, it sits inside
/// `EventTraceLogfileW::current_event`.
#[repr(C)]
#[derive(Clone, Copy)]
struct EventTrace {
    header: TraceEventHeader,
    instance_id: u32,
    parent_instance_id: u32,
    parent_guid: Guid,
    mof_data: *mut c_void,
    mof_length: u32,
    client_context: u32,
}

impl Default for EventTrace {
    fn default() -> Self {
        // SAFETY: all-zero is the documented "nothing set yet" state.
        unsafe { std::mem::zeroed() }
    }
}

/// `EVENT_TRACE_LOGFILEW`.
#[repr(C)]
#[derive(Clone, Copy)]
struct EventTraceLogfileW {
    log_file_name: *mut u16,
    logger_name: *mut u16,
    current_time: i64,
    buffers_read: u32,
    /// Union of `LogFileMode` and `ProcessTraceMode` in the C header.
    process_trace_mode: u32,
    current_event: EventTrace,
    logfile_header: TraceLogfileHeader,
    buffer_callback: *mut c_void,
    buffer_size: u32,
    filled: u32,
    events_lost: u32,
    pad: u32,
    event_record_callback: Option<unsafe extern "system" fn(*mut EventRecord)>,
    is_kernel_trace: u32,
    pad2: u32,
    context: *mut c_void,
}

impl Default for EventTraceLogfileW {
    fn default() -> Self {
        // SAFETY: every field is a plain integer or pointer; all-zero is the
        // documented "nothing set yet" state.
        unsafe { std::mem::zeroed() }
    }
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn StartTraceW(
        session: *mut u64,
        name: *const u16,
        properties: *mut EventTraceProperties,
    ) -> u32;
    fn EnableTraceEx2(
        session: u64,
        provider: *const Guid,
        control_code: u32,
        level: u8,
        match_any_keyword: u64,
        match_all_keyword: u64,
        timeout: u32,
        parameters: *const c_void,
    ) -> u32;
    fn OpenTraceW(logfile: *mut EventTraceLogfileW) -> u64;
    fn ProcessTrace(handles: *const u64, count: u32, start: *const i64, end: *const i64) -> u32;
    fn CloseTrace(handle: u64) -> u32;
    fn ControlTraceW(
        session: u64,
        name: *const u16,
        properties: *mut EventTraceProperties,
        control_code: u32,
    ) -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
}

/// Called on the ETW consumer thread for every event of the enabled provider.
unsafe extern "system" fn on_event(record: *mut EventRecord) {
    if record.is_null() {
        return;
    }
    // SAFETY: ETW guarantees the record is valid for the duration of the call.
    let header = unsafe { (*record).header };
    if header.provider_id != DXGI_PROVIDER {
        return;
    }
    if !FIRST_EVENT_LOGGED.swap(true, Ordering::Relaxed) {
        crate::diag::log("fps: real-time consumer is receiving DXGI events");
    }
    if header.descriptor.id != PRESENT_START_ID {
        return;
    }
    let target = TARGET_PID.load(Ordering::Relaxed);
    if target != 0 && header.process_id == target {
        if !FIRST_PRESENT_LOGGED.swap(true, Ordering::Relaxed) {
            crate::diag::log(&format!("fps: counting presents for pid {target}"));
        }
        PRESENTS.fetch_add(1, Ordering::Relaxed);
    }
}

/// A blank properties block with `name` appended, as ETW expects.
fn properties_with_name(name: &[u16]) -> Vec<u64> {
    let size = std::mem::size_of::<EventTraceProperties>();
    let total = size + name.len() * 2;
    let mut buffer = vec![0u64; total.div_ceil(8)];
    // SAFETY: the buffer is large enough for the struct plus the name.
    unsafe {
        let props = buffer.as_mut_ptr() as *mut EventTraceProperties;
        (*props).wnode.buffer_size = total as u32;
        (*props).wnode.flags = WNODE_FLAG_TRACED_GUID;
        (*props).wnode.client_context = CLIENT_CONTEXT_QPC;
        (*props).buffer_size = 8; // KB per buffer
        (*props).minimum_buffers = 4;
        (*props).maximum_buffers = 16;
        (*props).log_file_mode = EVENT_TRACE_REAL_TIME_MODE;
        (*props).flush_timer = 1; // seconds; the minimum for a real-time session
        (*props).logger_name_offset = size as u32;
        (*props).log_file_name_offset = 0;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            (buffer.as_mut_ptr() as *mut u8).add(size) as *mut u16,
            name.len(),
        );
    }
    buffer
}

/// Stops a session by name; used both for cleanup and before restarting ours.
/// Starts the real-time session and enables the DXGI provider in it.
fn start_session() -> Option<u64> {
    let name = crate::wide::wide(SESSION_NAME);
    let mut buffer = properties_with_name(&name);
    // SAFETY: `buffer` outlives every call below.
    unsafe {
        let props = buffer.as_mut_ptr() as *mut EventTraceProperties;
        let mut session: u64 = 0;
        let mut status = StartTraceW(&mut session, name.as_ptr(), props);
        if status == ERROR_ALREADY_EXISTS {
            // A previous run left it behind: stop it and take the name over.
            ControlTraceW(0, name.as_ptr(), props, EVENT_TRACE_CONTROL_STOP);
            status = StartTraceW(&mut session, name.as_ptr(), props);
        }
        if status != ERROR_SUCCESS {
            crate::diag::log(&format!("fps: StartTrace failed with code {status}"));
            return None;
        }
        let status = EnableTraceEx2(
            session,
            &DXGI_PROVIDER,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER,
            TRACE_LEVEL_VERBOSE,
            u64::MAX,
            0,
            0,
            std::ptr::null(),
        );
        if status != ERROR_SUCCESS {
            crate::diag::log(&format!("fps: EnableTraceEx2 failed with code {status}"));
            ControlTraceW(session, name.as_ptr(), props, EVENT_TRACE_CONTROL_STOP);
            return None;
        }
        Some(session)
    }
}

/// Runs the consumer for a session by name, or for a recorded trace file, on
/// the calling thread. Returns when the trace ends.
fn consume(logfile: &mut EventTraceLogfileW) {
    // SAFETY: the caller keeps `logfile` alive for the whole call.
    unsafe {
        let handle = OpenTraceW(logfile);
        if handle == INVALID_PROCESSTRACE_HANDLE {
            crate::diag::log("fps: OpenTrace failed");
            return;
        }
        ProcessTrace(&handle, 1, std::ptr::null(), std::ptr::null());
        CloseTrace(handle);
    }
}

pub struct FpsCollector {
    /// The last few (time, present count) readings, newest last.
    history: VecDeque<(Instant, usize)>,
    last_pid: u32,
}

impl FpsCollector {
    /// Returns `None` when the ETW session cannot be started (no administrator
    /// rights, or the provider is unavailable); the metric then shows `--`.
    pub fn new() -> Option<Self> {
        let session = start_session()?;
        let name = crate::wide::wide(SESSION_NAME);
        std::thread::spawn(move || {
            let mut logfile = EventTraceLogfileW {
                logger_name: name.as_ptr() as *mut u16,
                process_trace_mode: PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD,
                event_record_callback: Some(on_event),
                ..EventTraceLogfileW::default()
            };
            consume(&mut logfile);
            crate::diag::log("fps: DXGI present consumer stopped");
        });
        crate::diag::log("fps: DXGI present counter started");
        SESSION.store(session, Ordering::Relaxed);
        Some(Self {
            history: VecDeque::new(),
            last_pid: 0,
        })
    }

    /// Frames per second for the application owning the foreground window, or
    /// `None` when it submitted no frames since the previous call (desktop,
    /// idle app, or an application that presents outside DXGI).
    pub fn sample(&mut self) -> Option<f32> {
        // SAFETY: read-only queries about the foreground window.
        let pid = unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd == 0 {
                0
            } else {
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                pid
            }
        };

        // Counting is per target, so switching windows restarts the count.
        if pid != self.last_pid {
            self.last_pid = pid;
            self.history.clear();
            PRESENTS.store(0, Ordering::Relaxed);
            TARGET_PID.store(pid, Ordering::Relaxed);
            return None;
        }

        self.history
            .push_back((Instant::now(), PRESENTS.load(Ordering::Relaxed)));
        while self.history.len() > RATE_WINDOW {
            self.history.pop_front();
        }

        if pid == 0 {
            return None;
        }
        rate(&self.history)
    }
}

/// Presents per second across the retained readings, or `None` until there are
/// two of them or nothing has been presented.
fn rate(history: &VecDeque<(Instant, usize)>) -> Option<f32> {
    let (oldest_at, oldest_count) = *history.front()?;
    let (newest_at, newest_count) = *history.back()?;
    let seconds = newest_at.duration_since(oldest_at).as_secs_f32();
    let presents = newest_count.saturating_sub(oldest_count);
    if seconds <= 0.0 || presents == 0 {
        return None;
    }
    Some(presents as f32 / seconds)
}

impl Drop for FpsCollector {
    fn drop(&mut self) {
        shutdown();
    }
}

/// Stops the ETW session.
///
/// An ETW session outlives the process that started it, so both exit paths
/// call this explicitly: the metrics thread is still running when the process
/// ends, which means `Drop` would never run for the ordinary exit.
pub fn shutdown() {
    let session = SESSION.swap(0, Ordering::Relaxed);
    if session == 0 {
        return;
    }
    // SAFETY: stopping our own session by handle, which makes a null
    // properties block acceptable.
    unsafe {
        ControlTraceW(
            session,
            std::ptr::null(),
            std::ptr::null_mut(),
            EVENT_TRACE_CONTROL_STOP,
        );
    }
    crate::diag::log("fps: DXGI present counter stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A steady frame rate that ETW delivers in lumps must still read steady.
    /// This is the shape a one-second window gets wrong: it sees 0, then 300.
    #[test]
    fn rate_smooths_bursty_delivery() {
        use std::time::Duration;

        let t0 = Instant::now();
        let at = |secs: u64| t0 + Duration::from_secs(secs);
        // 300 presents arrive in one lump every other second: 100/s on average.
        let bursty = VecDeque::from([(at(0), 0), (at(1), 0), (at(2), 300), (at(3), 300)]);
        assert_eq!(rate(&bursty), Some(100.0));

        // A single reading, or one with no presents, is not a rate.
        let single = VecDeque::from([(at(0), 0)]);
        assert_eq!(rate(&single), None);
        let idle = VecDeque::from([(at(0), 7), (at(1), 7)]);
        assert_eq!(rate(&idle), None);
    }

    /// The layout assumptions the callback depends on.
    #[test]
    fn event_header_layout_matches_windows() {
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<EventHeader>(), 80);
        assert_eq!(offset_of!(EventHeader, process_id), 12);
        assert_eq!(offset_of!(EventHeader, provider_id), 24);
        assert_eq!(offset_of!(EventHeader, descriptor), 40);
        assert_eq!(size_of::<EventDescriptor>(), 16);

        // EVENT_TRACE uses the legacy header, and EVENT_TRACE_LOGFILEW embeds
        // it: getting either wrong would push the callback pointer out of place
        // and silently deliver no events.
        assert_eq!(size_of::<TraceEventHeader>(), 48);
        assert_eq!(size_of::<EventTrace>(), 88);
        assert_eq!(size_of::<TraceLogfileHeader>(), 280);
        assert_eq!(offset_of!(EventTraceLogfileW, current_event), 32);
        assert_eq!(offset_of!(EventTraceLogfileW, logfile_header), 120);
        assert_eq!(offset_of!(EventTraceLogfileW, event_record_callback), 424);
    }

    /// Replays a recorded trace when `DESKPULSE_FPS_ETL` points at one and
    /// `DESKPULSE_FPS_PID` names the process, so the ETW plumbing can be
    /// checked without administrator rights. Skipped when unset.
    ///
    /// With the trace captured while a UE5 game was running, the expected
    /// count is 180 `Present` events.
    #[test]
    fn replays_a_recorded_trace() {
        let (Ok(path), Ok(pid)) = (
            std::env::var("DESKPULSE_FPS_ETL"),
            std::env::var("DESKPULSE_FPS_PID"),
        ) else {
            return;
        };
        let pid: u32 = u32::from_str_radix(pid.trim_start_matches("0x"), 16).unwrap();
        TARGET_PID.store(pid, Ordering::Relaxed);
        PRESENTS.store(0, Ordering::Relaxed);

        let mut wide = crate::wide::wide(&path);
        let mut logfile = EventTraceLogfileW {
            log_file_name: wide.as_mut_ptr(),
            process_trace_mode: PROCESS_TRACE_MODE_EVENT_RECORD,
            event_record_callback: Some(on_event),
            ..EventTraceLogfileW::default()
        };
        consume(&mut logfile);

        let presents = PRESENTS.load(Ordering::Relaxed);
        println!("pid {pid:#x}: {presents} present events");
        assert!(presents > 0, "no Present events found in {path}");
    }
}
