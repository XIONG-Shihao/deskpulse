//! Direct client for the PawnIO kernel driver.
//!
//! Windows has no user-mode way to read the MSRs / SMN registers that carry CPU
//! temperature: `RDMSR` is a ring-0 instruction. PawnIO is a signed kernel
//! driver (installed by LibreHardwareMonitor, among others) that executes small
//! signed bytecode modules on our behalf.
//!
//! Protocol (from LibreHardwareMonitor's `PawnIo.cs`):
//! - open `\\?\GLOBALROOT\Device\PawnIO`
//! - `IOCTL_PIO_LOAD_BINARY`: load a module blob
//! - `IOCTL_PIO_EXECUTE_FN`: send `[32-byte fn name][i64 args...]`, read `i64` output

use std::ffi::c_void;

const DEVICE_PATH: &str = r"\\?\GLOBALROOT\Device\PawnIO";
const DEVICE_TYPE: u32 = 41394 << 16;
const IOCTL_LOAD_BINARY: u32 = DEVICE_TYPE | (0x821 << 2);
const IOCTL_EXECUTE_FN: u32 = DEVICE_TYPE | (0x841 << 2);
const FN_NAME_LENGTH: usize = 32;
const INVALID_HANDLE: isize = -1;

// PawnIO modules (from namazso/PawnIO.Modules 0.2.11, LGPL-2.1).
const AMD_MODULE: &[u8] = include_bytes!("../../assets/AMDFamily17.bin");
const INTEL_MODULE: &[u8] = include_bytes!("../../assets/IntelMSR.bin");

// AMD family 17h/19h SMN register holding Tctl/Tdie.
const F17H_THM_TCON_CUR_TMP: i64 = 0x0005_9800;

// Intel MSRs.
const IA32_TEMPERATURE_TARGET: i64 = 0x1A2; // TjMax in bits 16..24
const IA32_PACKAGE_THERM_STATUS: i64 = 0x1B1;
const IA32_THERM_STATUS: i64 = 0x19C;

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_READ_WRITE: u32 = 0x0000_0001 | 0x0000_0002;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: isize,
    ) -> isize;
    fn DeviceIoControl(
        device: isize,
        io_control_code: u32,
        in_buffer: *const c_void,
        in_size: u32,
        out_buffer: *mut c_void,
        out_size: u32,
        bytes_returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn CloseHandle(object: isize) -> i32;
    fn GetLastError() -> u32;
}

/// An open PawnIO handle with a loaded module.
///
/// Not `Send`/`Sync`: it must be created and used on the same thread, which is
/// how the metric collector uses it.
pub struct PawnIo {
    handle: isize,
}

impl PawnIo {
    /// Loads `module`, or returns the Win32 error from opening the device
    /// (5 = access denied, 2 = not found) or from the load IOCTL.
    fn load_module(module: &[u8]) -> Result<Self, u32> {
        let wide: Vec<u16> = DEVICE_PATH
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: valid NUL-terminated path, no security attributes.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                0,
            )
        };
        if handle == INVALID_HANDLE || handle == 0 {
            // SAFETY: no arguments.
            return Err(unsafe { GetLastError() });
        }

        // SAFETY: `module` is a valid byte slice; the call copies it in.
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_LOAD_BINARY,
                module.as_ptr() as *const c_void,
                module.len() as u32,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            // SAFETY: no arguments.
            let error = unsafe { GetLastError() };
            // SAFETY: `handle` was opened above and has not been closed.
            unsafe { CloseHandle(handle) };
            return Err(error);
        }

        Ok(Self { handle })
    }

    fn execute(&self, name: &str, input: &[i64], out_len: usize) -> Option<Vec<i64>> {
        let mut request = vec![0u8; FN_NAME_LENGTH + input.len() * 8];
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(FN_NAME_LENGTH - 1);
        request[..len].copy_from_slice(&name_bytes[..len]);
        for (index, value) in input.iter().enumerate() {
            let at = FN_NAME_LENGTH + index * 8;
            request[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }

        let mut output = vec![0i64; out_len];
        let mut returned: u32 = 0;
        // SAFETY: both buffers are valid for the given sizes.
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_EXECUTE_FN,
                request.as_ptr() as *const c_void,
                request.len() as u32,
                output.as_mut_ptr() as *mut c_void,
                (output.len() * 8) as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }

        output.truncate((returned as usize) / 8);
        Some(output)
    }
}

impl Drop for PawnIo {
    fn drop(&mut self) {
        // SAFETY: the handle is valid until dropped, and dropped once.
        unsafe { CloseHandle(self.handle) };
    }
}

enum Vendor {
    Amd,
    Intel,
    Other,
}

fn cpu_vendor() -> Vendor {
    #[cfg(target_arch = "x86_64")]
    {
        let result = std::arch::x86_64::__cpuid(0);
        let mut vendor = [0u8; 12];
        vendor[0..4].copy_from_slice(&result.ebx.to_le_bytes());
        vendor[4..8].copy_from_slice(&result.edx.to_le_bytes());
        vendor[8..12].copy_from_slice(&result.ecx.to_le_bytes());
        match &vendor {
            b"AuthenticAMD" => Vendor::Amd,
            b"GenuineIntel" => Vendor::Intel,
            _ => Vendor::Other,
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        Vendor::Other
    }
}

/// Reads CPU package temperature straight from the hardware via PawnIO.
pub enum CpuTemp {
    Amd(PawnIo),
    Intel(PawnIo),
    /// The driver could not be used. Carries the Win32 error (5 = access
    /// denied -> needs admin, 2 = not found -> driver not installed).
    Unavailable {
        vendor: &'static str,
        error: u32,
    },
}

impl CpuTemp {
    pub fn new() -> Self {
        let (vendor, module) = match cpu_vendor() {
            Vendor::Amd => ("amd", AMD_MODULE),
            Vendor::Intel => ("intel", INTEL_MODULE),
            Vendor::Other => {
                return CpuTemp::Unavailable {
                    vendor: "other",
                    error: 0,
                };
            }
        };

        match PawnIo::load_module(module) {
            Ok(pawn) => match vendor {
                "amd" => CpuTemp::Amd(pawn),
                _ => CpuTemp::Intel(pawn),
            },
            Err(error) => CpuTemp::Unavailable { vendor, error },
        }
    }

    /// Human-readable description of the backend and, if unavailable, why.
    pub fn describe(&self) -> String {
        match self {
            CpuTemp::Amd(_) => "PawnIO (AMD)".to_string(),
            CpuTemp::Intel(_) => "PawnIO (Intel)".to_string(),
            CpuTemp::Unavailable { vendor, error: 5 } => {
                format!("PawnIO unavailable: access denied (vendor={vendor}); needs admin")
            }
            CpuTemp::Unavailable { vendor, error: 2 } => {
                format!("PawnIO unavailable: driver not installed (vendor={vendor})")
            }
            CpuTemp::Unavailable { vendor, error } => {
                format!("PawnIO unavailable: error {error} (vendor={vendor})")
            }
        }
    }

    pub fn sample(&self) -> Option<f32> {
        match self {
            CpuTemp::Amd(pawn) => amd_temperature(pawn),
            CpuTemp::Intel(pawn) => intel_temperature(pawn),
            CpuTemp::Unavailable { .. } => None,
        }
    }
}

fn plausible(celsius: f32) -> Option<f32> {
    if celsius.is_finite() && (1.0..=150.0).contains(&celsius) {
        Some(celsius)
    } else {
        None
    }
}

/// AMD family 17h+: Tctl/Tdie.
/// `temp = (raw >> 21) * 0.125`, minus 49 C when the range/Tj-select bits say so.
fn amd_temperature(pawn: &PawnIo) -> Option<f32> {
    let raw = *pawn
        .execute("ioctl_read_smn", &[F17H_THM_TCON_CUR_TMP], 1)?
        .first()? as u32;

    let offset_flag = (raw & 0x0008_0000) != 0 || (raw & 0x0003_0000) == 0x0003_0000;
    let mut temperature = ((raw >> 21) as f32) * 0.125;
    if offset_flag {
        temperature -= 49.0;
    }
    plausible(temperature)
}

/// Intel: `TjMax - digital readout`. Prefers the package sensor, falls back to
/// the core sensor.
fn intel_temperature(pawn: &PawnIo) -> Option<f32> {
    let tjmax_raw = *pawn
        .execute("ioctl_read_msr", &[IA32_TEMPERATURE_TARGET], 1)?
        .first()? as u32;
    let tjmax = ((tjmax_raw >> 16) & 0xFF) as f32;
    if !(50.0..=120.0).contains(&tjmax) {
        return None;
    }

    for msr in [IA32_PACKAGE_THERM_STATUS, IA32_THERM_STATUS] {
        let Some(out) = pawn.execute("ioctl_read_msr", &[msr], 1) else {
            continue;
        };
        let Some(eax) = out.first().copied() else {
            continue;
        };
        let eax = eax as u32;
        if eax & 0x8000_0000 != 0 {
            let delta = ((eax >> 16) & 0x7F) as f32;
            if let Some(temperature) = plausible(tjmax - delta) {
                return Some(temperature);
            }
        }
    }
    None
}
