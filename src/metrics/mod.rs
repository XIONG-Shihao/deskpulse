use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) mod fps;
mod gpu;
mod gpu_mem;
mod net;
mod pawnio;
mod system;
mod temp;

/// A point-in-time view of all metrics.
///
/// Any metric that could not be sampled is `None` and rendered as `--`.
/// We never substitute `0` for an unknown value, because that would be a lie.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub net_up_bps: Option<f64>,
    pub net_down_bps: Option<f64>,
    pub cpu_usage: Option<f32>,
    pub cpu_temp_c: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub gpu_usage: Option<f32>,
    pub gpu_mem_used: Option<u64>,
    pub gpu_mem_total: Option<u64>,
    /// Device-wide committed GPU memory (dedicated + borrowed from RAM).
    pub gpu_mem_committed: Option<u64>,
    pub gpu_temp_c: Option<f32>,
    /// Present rate of the foreground application, from ETW.
    pub fps: Option<f32>,
}

pub type Shared = Arc<Mutex<Snapshot>>;

fn percent(used: Option<u64>, total: Option<u64>) -> Option<f32> {
    match (used, total) {
        (Some(used), Some(total)) if total > 0 => Some(used as f32 / total as f32 * 100.0),
        _ => None,
    }
}

impl Snapshot {
    pub fn mem_percent(&self) -> Option<f32> {
        percent(self.mem_used, self.mem_total)
    }

    pub fn vram_percent(&self) -> Option<f32> {
        // Prefer the device-wide committed figure (dedicated + borrowed from
        // system RAM), which is allowed to exceed 100%; fall back to NVML's
        // dedicated usage when the performance counters are unavailable.
        match (self.gpu_mem_committed, self.gpu_mem_total) {
            (Some(committed), Some(total)) if total > 0 => {
                Some(committed as f32 / total as f32 * 100.0)
            }
            _ => percent(self.gpu_mem_used, self.gpu_mem_total),
        }
    }
}

pub fn spawn(interval: Duration, lhm_port: u16, wake: impl Fn() + Send + 'static) -> Shared {
    let shared: Shared = Arc::new(Mutex::new(Snapshot::default()));
    let out = Arc::clone(&shared);

    thread::spawn(move || {
        let mut system = system::SystemCollector::new();
        let mut net = net::NetCollector::new();
        let mut gpu = gpu::GpuCollector::new();
        let mut gpu_mem = gpu_mem::GpuMemCollector::new();
        let mut fps = fps::FpsCollector::new();
        let mut temp = temp::TempCollector::new(lhm_port);

        // Prime the counters so the first real sample has a delta to compare against.
        system.sample();
        net.sample();
        thread::sleep(Duration::from_millis(500));

        loop {
            let started = Instant::now();

            let (cpu_usage, mem_used, mem_total) = system.sample();
            let (net_up, net_down) = net.sample();
            let gpu_sample = gpu.sample();
            let gpu_committed = gpu_mem.as_mut().and_then(|collector| collector.sample());
            let fps_sample = fps.as_mut().and_then(|collector| collector.sample());
            let cpu_temp = temp.sample();

            if let Ok(mut snap) = out.lock() {
                snap.cpu_usage = Some(cpu_usage);
                snap.mem_used = Some(mem_used);
                snap.mem_total = Some(mem_total);
                snap.net_up_bps = net_up;
                snap.net_down_bps = net_down;
                snap.gpu_usage = gpu_sample.usage;
                snap.gpu_mem_used = gpu_sample.mem_used;
                snap.gpu_mem_total = gpu_sample.mem_total;
                snap.gpu_mem_committed = gpu_committed.map(|(_, committed)| committed);
                snap.gpu_temp_c = gpu_sample.temp_c;
                snap.cpu_temp_c = cpu_temp;
                snap.fps = fps_sample;
            }

            // Wake the UI only when there is new data to show, instead of
            // repainting on a fixed timer.
            wake();

            let elapsed = started.elapsed();
            if let Some(remaining) = interval.checked_sub(elapsed) {
                thread::sleep(remaining);
            }
        }
    });

    shared
}
