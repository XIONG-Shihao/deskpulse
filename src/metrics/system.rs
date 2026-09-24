//! CPU load and memory usage. Both come from one `sysinfo::System`, so the
//! process keeps a single copy of the CPU topology.

use sysinfo::System;

pub struct SystemCollector {
    sys: System,
}

impl SystemCollector {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        Self { sys }
    }

    /// `(cpu usage in percent, used memory, total memory)`, all in bytes.
    pub fn sample(&mut self) -> (f32, u64, u64) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        (
            self.sys.global_cpu_usage(),
            self.sys.used_memory(),
            self.sys.total_memory(),
        )
    }
}
