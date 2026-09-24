//! The metrics the overlay can show.

use crate::format;
use crate::i18n::Text;
use crate::metrics::Snapshot;

/// Values above these are shown in the warning colour.
const TEMP_WARN_C: f32 = 80.0;
const PERCENT_WARN: f32 = 80.0;
/// Video memory may legitimately exceed 100 % once the GPU borrows system RAM.
const VRAM_WARN_PERCENT: f32 = 100.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Metric {
    NetUp,
    NetDown,
    Cpu,
    CpuTemp,
    Mem,
    Gpu,
    Vram,
    GpuTemp,
    Fps,
}

impl Metric {
    pub(super) const ALL: [Metric; 9] = [
        Metric::NetUp,
        Metric::NetDown,
        Metric::Cpu,
        Metric::CpuTemp,
        Metric::Mem,
        Metric::Gpu,
        Metric::Vram,
        Metric::GpuTemp,
        Metric::Fps,
    ];

    pub(super) fn id(self) -> &'static str {
        match self {
            Metric::NetUp => "net_up",
            Metric::NetDown => "net_down",
            Metric::Cpu => "cpu",
            Metric::CpuTemp => "cpu_temp",
            Metric::Mem => "mem",
            Metric::Gpu => "gpu",
            Metric::Vram => "vram",
            Metric::GpuTemp => "gpu_temp",
            Metric::Fps => "fps",
        }
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|metric| metric.id() == id)
    }

    pub(super) fn label(self, t: &Text) -> &'static str {
        match self {
            Metric::NetUp => t.net_up,
            Metric::NetDown => t.net_down,
            Metric::Cpu => t.cpu,
            Metric::CpuTemp => t.cpu_temp,
            Metric::Mem => t.mem,
            Metric::Gpu => t.gpu,
            Metric::Vram => t.vram,
            Metric::GpuTemp => t.gpu_temp,
            Metric::Fps => t.fps,
        }
    }

    pub(super) fn value(self, snapshot: &Snapshot) -> String {
        match self {
            Metric::NetUp => format::format_speed(snapshot.net_up_bps),
            Metric::NetDown => format::format_speed(snapshot.net_down_bps),
            Metric::Cpu => format::format_percent(snapshot.cpu_usage),
            Metric::CpuTemp => format::format_temp(snapshot.cpu_temp_c),
            Metric::Mem => format::format_percent(snapshot.mem_percent()),
            Metric::Gpu => format::format_percent(snapshot.gpu_usage),
            Metric::Vram => format::format_percent(snapshot.vram_percent()),
            Metric::GpuTemp => format::format_temp(snapshot.gpu_temp_c),
            Metric::Fps => format::format_frame_rate(snapshot.fps),
        }
    }

    /// Whether this metric is shown before the user has ever toggled it.
    /// Everything is on by default except the frame rate, which is only
    /// meaningful while a game is in the foreground and needs an ETW session.
    pub(super) fn default_visible(self) -> bool {
        !matches!(self, Metric::Fps)
    }

    /// Whether this metric has crossed into its warning range: temperatures
    /// above 80 °C, memory above 80 %, or video memory above 100 % (only
    /// possible once the GPU borrows system memory).
    pub(super) fn is_warning(self, snapshot: &Snapshot) -> bool {
        match self {
            Metric::CpuTemp => snapshot.cpu_temp_c.is_some_and(|t| t > TEMP_WARN_C),
            Metric::GpuTemp => snapshot.gpu_temp_c.is_some_and(|t| t > TEMP_WARN_C),
            Metric::Mem => snapshot.mem_percent().is_some_and(|p| p > PERCENT_WARN),
            Metric::Vram => snapshot
                .vram_percent()
                .is_some_and(|p| p > VRAM_WARN_PERCENT),
            _ => false,
        }
    }

    /// Strings wide enough to cover every value this metric can render, so the
    /// panel reserves the value column once instead of growing with live data.
    /// They follow the format maxima: three integer digits plus one decimal for
    /// speeds, `100%` for percentages, three digits for temperatures.
    pub(super) fn widest_values(self) -> &'static [&'static str] {
        match self {
            Metric::NetUp | Metric::NetDown => &[
                "999.9 MB/s",
                "999.9 KB/s",
                "999.9 GB/s",
                "999.9 TB/s",
                "0.00 MB/s",
            ],
            Metric::Cpu | Metric::Mem | Metric::Gpu | Metric::Vram => &["100%"],
            Metric::CpuTemp | Metric::GpuTemp => &["100\u{00B0}C", "-10\u{00B0}C"],
            Metric::Fps => &["9999 FPS"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Metric, TEMP_WARN_C, VRAM_WARN_PERCENT};
    use crate::metrics::Snapshot;

    #[test]
    fn warning_thresholds_are_inclusive_at_the_boundary() {
        let cpu = |c: f32| Snapshot {
            cpu_temp_c: Some(c),
            ..Snapshot::default()
        };
        assert!(!Metric::CpuTemp.is_warning(&cpu(TEMP_WARN_C)));
        assert!(Metric::CpuTemp.is_warning(&cpu(TEMP_WARN_C + 0.5)));

        let gpu = |c: f32| Snapshot {
            gpu_temp_c: Some(c),
            ..Snapshot::default()
        };
        assert!(Metric::GpuTemp.is_warning(&gpu(95.0)));
        assert!(!Metric::GpuTemp.is_warning(&gpu(40.0)));

        // memory: > 80 %
        let mem = |used: u64| Snapshot {
            mem_used: Some(used),
            mem_total: Some(1000),
            ..Snapshot::default()
        };
        assert!(!Metric::Mem.is_warning(&mem(800)));
        assert!(Metric::Mem.is_warning(&mem(801)));

        // video memory: > 100 % is only reachable through the committed figure
        let vram = |committed: u64| Snapshot {
            gpu_mem_committed: Some(committed),
            gpu_mem_total: Some(1000),
            ..Snapshot::default()
        };
        assert!(!Metric::Vram.is_warning(&vram(1000)));
        assert!(Metric::Vram.is_warning(&vram(1010)));
        let _ = VRAM_WARN_PERCENT;

        // unknown values never warn
        let empty = Snapshot::default();
        for metric in Metric::ALL {
            assert!(
                !metric.is_warning(&empty),
                "{} warned with no data",
                metric.id()
            );
        }
    }
}
