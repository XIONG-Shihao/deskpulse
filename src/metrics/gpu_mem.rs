//! Device-wide GPU memory, including the part borrowed from system RAM.
//!
//! NVML reports the *dedicated* VRAM only, so its percentage can never exceed
//! 100%. Windows lets a GPU spill into system memory (WDDM "shared GPU
//! memory", typically up to half the physical RAM) and exposes both numbers as
//! the `GPU Adapter Memory` performance counters — the same source Task Manager
//! uses. Reading the committed total from there makes ">100 %" meaningful.

const PDH_FMT_DOUBLE: u32 = 0x0000_0200;
const PDH_MORE_DATA: u32 = 0x8000_07D2;
const PDH_SUCCESS: u32 = 0;

/// `PDH_FMT_COUNTERVALUE`: a status word followed by an 8-byte union. With
/// `PDH_FMT_DOUBLE` the payload is the `double`, which lands at offset 8.
#[repr(C)]
struct PdhFmtCounterValue {
    status: u32,
    value: f64,
}

#[repr(C)]
struct PdhFmtCounterValueItemW {
    name: *mut u16,
    value: PdhFmtCounterValue,
}

#[link(name = "pdh")]
unsafe extern "system" {
    fn PdhOpenQueryW(source: *const u16, user_data: usize, query: *mut isize) -> u32;
    fn PdhAddEnglishCounterW(
        query: isize,
        path: *const u16,
        user_data: usize,
        counter: *mut isize,
    ) -> u32;
    fn PdhCollectQueryData(query: isize) -> u32;
    fn PdhGetFormattedCounterArrayW(
        counter: isize,
        format: u32,
        size: *mut u32,
        count: *mut u32,
        buffer: *mut PdhFmtCounterValueItemW,
    ) -> u32;
    fn PdhCloseQuery(query: isize) -> u32;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct GpuMemCollector {
    query: isize,
    committed: isize,
    dedicated: isize,
}

impl GpuMemCollector {
    pub fn new() -> Option<Self> {
        // SAFETY: plain PDH setup; every handle is released in `Drop`.
        unsafe {
            let mut query: isize = 0;
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != PDH_SUCCESS {
                return None;
            }
            let mut committed: isize = 0;
            let mut dedicated: isize = 0;
            let a = PdhAddEnglishCounterW(
                query,
                wide(r"\GPU Adapter Memory(*)\Total Committed").as_ptr(),
                0,
                &mut committed,
            );
            let b = PdhAddEnglishCounterW(
                query,
                wide(r"\GPU Adapter Memory(*)\Dedicated Usage").as_ptr(),
                0,
                &mut dedicated,
            );
            if a != PDH_SUCCESS || b != PDH_SUCCESS {
                PdhCloseQuery(query);
                return None;
            }
            // Instances only appear after the first collect.
            PdhCollectQueryData(query);
            Some(Self {
                query,
                committed,
                dedicated,
            })
        }
    }

    /// `(dedicated bytes, dedicated + borrowed bytes)` for the busiest adapter
    /// — the discrete GPU, i.e. the one NVML is reporting on.
    pub fn sample(&mut self) -> Option<(u64, u64)> {
        // SAFETY: `self.query` is live for as long as `self`.
        unsafe {
            if PdhCollectQueryData(self.query) != PDH_SUCCESS {
                return None;
            }
            let dedicated = read_counter(self.dedicated)?;
            let committed = read_counter(self.committed)?;
            let (busiest_name, busiest_dedicated) =
                dedicated.iter().max_by_key(|(_, bytes)| *bytes)?;
            let busiest_committed = committed
                .iter()
                .find(|(name, _)| name == busiest_name)
                .map(|(_, bytes)| *bytes)?;
            Some((*busiest_dedicated, busiest_committed))
        }
    }
}

impl Drop for GpuMemCollector {
    fn drop(&mut self) {
        // SAFETY: the query was opened by `new` and is closed once.
        unsafe { PdhCloseQuery(self.query) };
    }
}

/// One formatted sample of every instance of `counter`.
///
/// # Safety
/// `counter` must come from `PdhAddEnglishCounterW` and the query must have
/// been collected at least once.
unsafe fn read_counter(counter: isize) -> Option<Vec<(String, u64)>> {
    // SAFETY: the caller guarantees a live counter handle.
    unsafe {
        let mut size: u32 = 0;
        let mut count: u32 = 0;
        let status = PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            std::ptr::null_mut(),
        );
        if status != PDH_MORE_DATA && status != PDH_SUCCESS {
            return None;
        }
        if size == 0 {
            return None;
        }

        // The size covers the item array *and* the instance name strings that
        // follow it, so use an 8-byte-aligned byte buffer of that exact size.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let items = buffer.as_mut_ptr() as *mut PdhFmtCounterValueItemW;
        let status =
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, items);
        if status != PDH_SUCCESS {
            return None;
        }

        let items = std::slice::from_raw_parts(items, count as usize);
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            if item.value.status != PDH_SUCCESS || item.name.is_null() {
                continue;
            }
            let mut len = 0usize;
            while len < 256 && *item.name.add(len) != 0 {
                len += 1;
            }
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(item.name, len));
            out.push((name, item.value.value.max(0.0) as u64));
        }
        Some(out)
    }
}
