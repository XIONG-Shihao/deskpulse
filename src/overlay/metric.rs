//! The metrics the overlay can show.

use crate::format;
use crate::i18n::Text;
use crate::metrics::Snapshot;

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
}

impl Metric {
    pub(super) const ALL: [Metric; 8] = [
        Metric::NetUp,
        Metric::NetDown,
        Metric::Cpu,
        Metric::CpuTemp,
        Metric::Mem,
        Metric::Gpu,
        Metric::Vram,
        Metric::GpuTemp,
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
        }
    }
}
