# deskpulse — Tech Stack and Architecture

English | [中文](ARCHITECTURE.zh.md)

This document records the current tech choices, architecture and key trade-offs. For features and usage see [README.md](README.md).

## 1. Tech stack

| Area | Choice | Notes |
| --- | --- | --- |
| Language | Rust, edition 2024 | Windows only (`x86_64-pc-windows-msvc`) |
| GUI | `eframe` / `egui` `0.36` (glow backend) | Immediate-mode GUI; borderless, transparent, always-on-top window |
| Network / CPU / memory | `sysinfo` `0.39` | Interface byte counters, CPU usage, memory |
| GPU | `nvml-wrapper` `0.13` | NVIDIA NVML: usage / VRAM / temperature (loads `nvml.dll` at runtime) |
| CPU temperature (primary) | PawnIO kernel driver + hand-written FFI | No third-party Rust wrapper; uses `CreateFile` / `DeviceIoControl` directly |
| CPU temperature (fallback) | `serde_json` + std `TcpStream` | Polls LibreHardwareMonitor's `http://127.0.0.1:8085/data.json` |
| Tray / menus | `tray-icon` `0.25` (uses muda) | Tray icon and native menus |
| Config | `serde` + `toml` | `%APPDATA%\deskpulse\config.toml` |
| Autostart | `schtasks.exe` | Scheduled task, `RunLevel Highest` |
| Packaging | `winresource`, `+crt-static`, LTO | Icon/version info, no VC++ runtime, single-file exe |
| Chinese text | system font loaded at runtime | egui ships no CJK glyphs |

Design principles: minimise dependencies; every metric that may be unavailable is an `Option` rendered as `--`, and **`0` is never used as a stand-in for unknown**.

## 2. Architecture

### 2.1 Threading model

```
main thread (eframe/winit event loop)
├── root viewport: the overlay (metric display)
├── deferred viewport: right-click settings menu (its own small window)
└── App::logic every frame: drain menu actions, handle tray events, request repaint

collector thread (1)
└── periodic sampling → writes Arc<Mutex<Snapshot>>

menu-event thread (1)
└── blocks on MenuEvent::recv(); on event, enqueue + ctx.request_repaint()
```

- **Collection is decoupled from the UI**: system calls happen only on the collector thread; the UI reads one snapshot per frame instead of querying every frame.
- **Menu events wake the UI immediately**: polling tray events only from `logic` would wait up to one repaint period (~400ms); the listener thread calls `request_repaint()` so quit/switch actions take effect instantly.

### 2.2 Data flow

```
Collectors ──sample()──► Snapshot ──(Arc<Mutex>)──► App::ui clones each frame
```

`Snapshot` holds all 8 metrics, each an `Option` (`None` when unavailable). Percentages and VRAM ratios are computed on the UI side from raw values.

### 2.3 Viewport model

- **Root viewport**: borderless + transparent + always-on-top + not in the taskbar. Every frame it measures the content and sends `ViewportCommand::InnerSize` to **hug the content**, so there is no wasted transparent area.
- **Menu viewport**: created on right-click via `show_viewport_deferred` as its own window (`decorations=false`, transparent, always-on-top), tightly fitted; the main window size is unaffected. The menu is a two-level structure.

### 2.4 Module responsibilities

| Module | Responsibility |
| --- | --- |
| `main.rs` | Parse `--dump`; self-elevate; create the window and start eframe |
| `app.rs` | eframe `App` (`logic`/`ui`); layout rendering; window fitting; menu viewport and action draining |
| `config.rs` | `Config` load/save, defaults, legacy-directory migration |
| `i18n.rs` | `Language`, string tables, system-language selection |
| `format.rs` | Speed / percent / temperature formatting (including roll-over rules) |
| `tray.rs` | Tray icon and menu; exposes `set_autostart_checked` |
| `autostart.rs` | Manages the logon scheduled task via `schtasks` |
| `elevate.rs` | `TokenElevation` check + `ShellExecuteW("runas")` self-elevation |
| `diag.rs` | Timestamped diagnostic log |
| `metrics/mod.rs` | `Snapshot`, collector thread, percentage helpers |
| `metrics/*.rs` | Individual collectors (net / cpu / mem / gpu / pawnio / temp) |

## 3. Metric data sources

| Metric | How it is read |
| --- | --- |
| Network speed | `sysinfo::Networks` cumulative bytes, time-differenced; loopback / virtual / VPN interfaces filtered out |
| CPU usage | `sysinfo` `global_cpu_usage()` (one reused `System` instance) |
| Memory | `sysinfo` `used_memory / total_memory` |
| GPU | NVML: `utilization_rates` / `memory_info` / `temperature` |
| CPU temperature | PawnIO direct first, LHM HTTP fallback (next section) |

## 4. CPU temperature: why a kernel driver is required

Windows has no reliable public CPU temperature API. Core temperature can only be read from **MSR** (Intel) / **SMN** (AMD) registers, and `RDMSR` is a **ring-0 privileged instruction** — executing it in user mode raises #GP. A kernel driver must read on our behalf. The options:

| Option | Cost |
| --- | --- |
| **PawnIO driver** (current primary) | Needs the driver installed + admin rights; no resident app required |
| LibreHardwareMonitor HTTP (fallback) | Needs LHM running |
| Own signed driver | EV certificate + Microsoft attestation; costly and often blocked — not used |
| ACPI thermal-zone WMI | Often cannot read core temperature — not used |

### 4.1 PawnIO protocol

```
open \\.\GLOBALROOT\Device\PawnIO
  → DeviceIoControl(IOCTL_PIO_LOAD_BINARY, module bytes)            // load a .bin module
  → DeviceIoControl(IOCTL_PIO_EXECUTE_FN, [32-byte fn name][i64 args…])  // run, get i64 results
```

`DEVICE_TYPE = 41394 << 16`; `LOAD_BINARY = DEVICE_TYPE | (0x821 << 2)`; `EXECUTE_FN = DEVICE_TYPE | (0x841 << 2)`.

### 4.2 Registers and formulas

- **AMD**: `ioctl_read_smn(0x00059800)` → `temp = ((raw >> 21) & 0x7FF) * 0.125`; subtract 49°C when `raw & 0x80000 != 0` or `raw & 0x30000 == 0x30000`.
- **Intel**: `ioctl_read_msr(0x1A2)` gives TjMax (bits 16..24); the DTS reading `delta` (bits 16..22) comes from `0x1B1` (package) or `0x19C` (core); `temp = TjMax - delta`.

Modules come from [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) (LGPL-2.1) and are embedded in the app.

## 5. Elevation and autostart

- Direct reading needs administrator rights (a PawnIO device restriction); this is a hard constraint.
- **Self-elevation**: on startup `OpenProcessToken` + `TokenElevation` decides; if not elevated the app relaunches itself via `ShellExecuteW("runas")` (one UAC prompt). When started by the scheduled task it is already elevated, so no prompt.
- **Autostart**: a logon scheduled task `deskpulse` is created with `schtasks` (`/SC ONLOGON /RL HIGHEST`), avoiding a UAC prompt at every logon. The in-app "Start with Windows" toggle manages that task.

## 6. Window and layout

- **Content fitting**: each frame takes the content rect, compares it with the previous size and only sends `InnerSize` past a threshold, avoiding jitter.
- **Fixed cells**: labels/values use fixed-width boxes (tight 40 / 74, loose 46 / 92), so digit changes do not change the window width.
- **Alignment**: in tight mode the first column's label is right-aligned and its value left-aligned; the two-column layout's second-column labels are left-aligned (their left edges line up). Aligned layouts force the box width with `set_min_width`, otherwise the box shrinks to the text and the column drifts.
- **Spacing preset**: `item_spacing.x = 0`; the 4pt label↔value gap is added explicitly inside a cell; there is no gap between the two columns.
- **Menu**: a deferred viewport with dark `Visuals` + near-black background and near-white text (not tied to the system theme), two-level structure; actions are passed back through an `Arc<Mutex<MenuState>>` queue that the parent drains every frame.

## 7. Configuration and migration

- Path `%APPDATA%\deskpulse\config.toml`; `#[serde(default)]` fills missing fields with defaults.
- On first run, if the new path is missing, the app reads the old path `%APPDATA%\desk-stats\config.toml` and migrates it (the project used to be named `desk-stats`).
- `language` is an `Option`: when unset it is chosen from the system UI language (`GetUserDefaultUILanguage`).

## 8. Packaging

- `build.rs` embeds `assets/icon.ico` and version info with `winresource`.
- `.cargo/config.toml` enables `+crt-static` for the msvc target, removing the VC++ runtime dependency.
- release: `lto` + `codegen-units=1` + `strip` + `panic="abort"`, about 5.9 MB.

## 9. Key trade-offs

1. **CPU temperature moved from WMI to a direct PawnIO read.** Newer LibreHardwareMonitor (0.9.x) removed the WMI provider, so the old `root\LibreHardwareMonitor` approach no longer works; going further, to drop the dependency on a resident app we read the PawnIO driver directly. LHM HTTP is only a fallback.
2. **GPU uses NVML only.** No generic PDH fallback; non-NVIDIA GPUs show `--`.
3. **No custom driver, no embedded hidden helper.** The first is too costly to maintain; the second is treated as malware by AV.
4. **Unknown means `--`.** Never substitute `0`.

## 10. Known limitations / not done

- Direct temperature reading needs admin; without it the app falls back to LHM (`--` if not installed).
- The Intel temperature path is implemented but not verified on an Intel machine (the dev machine is AMD).
- Multiple GPUs: only `device_by_index(0)` is used.
- No history graphs, no log persistence, only zh/en.
