use sysinfo::System;

pub struct MemCollector {
    sys: System,
}

impl MemCollector {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }

    pub fn sample(&mut self) -> (u64, u64) {
        self.sys.refresh_memory();
        (self.sys.used_memory(), self.sys.total_memory())
    }
}
