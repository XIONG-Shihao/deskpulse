use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::TemperatureSensor;

/// NVIDIA GPU metrics via NVML.
///
/// NVML is loaded dynamically at runtime, so a machine without an NVIDIA driver
/// simply reports `None` for everything instead of failing to start.
pub struct GpuCollector {
    nvml: Option<Nvml>,
}

pub struct GpuSample {
    pub usage: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub temp_c: Option<f32>,
}

impl GpuCollector {
    pub fn new() -> Self {
        Self {
            nvml: Nvml::init().ok(),
        }
    }

    pub fn sample(&mut self) -> GpuSample {
        let empty = GpuSample {
            usage: None,
            mem_used: None,
            mem_total: None,
            temp_c: None,
        };
        let Some(nvml) = self.nvml.as_ref() else {
            return empty;
        };
        let Ok(device) = nvml.device_by_index(0) else {
            return empty;
        };

        let usage = device.utilization_rates().ok().map(|u| u.gpu as f32);
        let (mem_used, mem_total) = match device.memory_info() {
            Ok(info) => (Some(info.used), Some(info.total)),
            Err(_) => (None, None),
        };
        let temp_c = device
            .temperature(TemperatureSensor::Gpu)
            .ok()
            .map(|t| t as f32);

        GpuSample {
            usage,
            mem_used,
            mem_total,
            temp_c,
        }
    }
}
