use std::time::Instant;

use sysinfo::Networks;

/// Sample network throughput by diffing cumulative interface byte counters.
///
/// The first call only records a baseline and reports `None`; a value is
/// available from the second call onward.
pub struct NetCollector {
    networks: Networks,
    last: Option<Counters>,
}

struct Counters {
    rx: u64,
    tx: u64,
    at: Instant,
}

impl NetCollector {
    pub fn new() -> Self {
        Self {
            networks: Networks::new_with_refreshed_list(),
            last: None,
        }
    }

    /// Filter out loopback and virtual adapters so VPN / VM / WSL traffic does
    /// not get counted twice or spike the numbers.
    fn is_physical(name: &str) -> bool {
        const VIRTUAL_MARKERS: [&str; 9] = [
            "loopback",
            "vethernet",
            "virtual",
            "vmware",
            "virtualbox",
            "hyper-v",
            "tunnel",
            "bluetooth",
            "wsl",
        ];
        let lower = name.to_ascii_lowercase();
        !VIRTUAL_MARKERS.iter().any(|marker| lower.contains(marker))
    }

    /// Returns `(upload_bps, download_bps)` in bytes per second.
    pub fn sample(&mut self) -> (Option<f64>, Option<f64>) {
        self.networks.refresh(true);

        let mut rx: u64 = 0;
        let mut tx: u64 = 0;
        for (name, data) in &self.networks {
            if Self::is_physical(name) {
                rx = rx.saturating_add(data.total_received());
                tx = tx.saturating_add(data.total_transmitted());
            }
        }

        let now = Instant::now();
        let previous = self.last.replace(Counters { rx, tx, at: now });
        let Some(previous) = previous else {
            return (None, None);
        };

        let dt = now.duration_since(previous.at).as_secs_f64();
        if dt <= 0.0 {
            return (None, None);
        }

        let up = tx.saturating_sub(previous.tx) as f64 / dt;
        let down = rx.saturating_sub(previous.rx) as f64 / dt;
        (Some(up), Some(down))
    }
}
