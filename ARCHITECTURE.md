# deskpulse — Tech Stack and Architecture

English | [中文](ARCHITECTURE.zh.md)

This document records the current tech choices, architecture and key trade-offs. For features and usage see [README.md](README.md).

## 1. Tech stack

| Area | Choice | Notes |
| --- | --- | --- |
| Language | Rust, edition 2024 | Windows only (`x86_64-pc-windows-msvc`) |
| GUI | Native Win32 layered window + GDI, hand-written FFI | Per-pixel-alpha `UpdateLayeredWindow`; no GPU API, no GUI framework |
| Network / CPU / memory | `sysinfo` `0.39` | Interface byte counters, CPU usage, memory |
| GPU | `nvml-wrapper` `0.13` | NVIDIA NVML: usage / VRAM / temperature (loads `nvml.dll` at runtime) |
| CPU temperature (primary) | PawnIO kernel driver + hand-written FFI | No third-party Rust wrapper; uses `CreateFile` / `DeviceIoControl` directly |
| CPU temperature (fallback) | `serde_json` + std `TcpStream` | Polls LibreHardwareMonitor's `http://127.0.0.1:8085/data.json` |
| Tray / menus | `tray-icon` `0.25` (uses muda) | Tray icon and the native right-click menu |
| Config | `serde` + `toml` | `%APPDATA%\deskpulse\config.toml` |
| Autostart | `schtasks.exe` | Scheduled task, `RunLevel Highest` |
| Packaging | `winresource`, `+crt-static`, LTO | Icon/version info, no VC++ runtime, single-file exe |
| Chinese text | `Microsoft YaHei` via `CreateFontW` | GDI renders CJK without shipping glyphs |

Design principles: minimise dependencies; every metric that may be unavailable is an `Option` rendered as `--`, and **`0` is never used as a stand-in for unknown**.

## 2. Architecture

### 2.1 Threading model

```
main thread — Win32 message loop (GetMessageW / DispatchMessageW)
├── owns the layered window: lays out, draws into a DIB, handles input and the menu
└── on WM_APP_DATA refresh the panel; on WM_APP_MENU apply a menu action

collector thread (1)
└── samples every refresh_secs → writes Arc<Mutex<Snapshot>> → PostMessageW(WM_APP_DATA)

muda / tray event thread (owned by tray-icon)
└── MenuEvent handler → queue the id → PostMessageW(WM_APP_MENU)
```

- **Collection is decoupled from the UI**: system calls happen only on the collector thread; the UI clones one `Snapshot` when it is woken.
- **Event-driven, no render loop**: the collector posts `WM_APP_DATA` after each new sample, so the panel redraws once per tick and does nothing in between.
- **Menu events are queued, not handled in place**: muda delivers events on its own thread, so the handler pushes the id into an `Arc<Mutex<Vec<String>>>` and posts `WM_APP_MENU`; the main thread owns all UI state.

### 2.2 Data flow

```
Collectors ──sample()──► Snapshot ──(Arc<Mutex>)──► refresh() clones once per tick
```

`Snapshot` holds all 8 metrics, each an `Option` (`None` when unavailable). Percentages and VRAM ratios are computed on the UI side from raw values.

### 2.3 Window model

- **One window for everything.** A borderless `WS_POPUP` window with `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE` — always on top, never activated by a click, and never shown in the taskbar or Alt-Tab.
- **Per-pixel alpha via GDI.** The panel background (a software rounded rectangle filled with the configured alpha) and the text (`DrawTextW`, `Microsoft YaHei`) are composited into a 32-bit top-down DIB. Any non-black pixel is forced to alpha 255 so the text stays crisp over the translucent panel. The DIB is handed to the compositor with a single `UpdateLayeredWindow(..., ULW_ALPHA)` call.
- **Drag** is manual (`SetCapture` + `SetCursorPos`), **the menu** is the native muda menu shown with `show_context_menu_for_hwnd` (one builder produces both it and the tray menu, so the two never drift apart), and **repaint** is triggered by `WM_APP_DATA` rather than a timer.

### 2.4 Module responsibilities

| Module | Responsibility |
| --- | --- |
| `main.rs` | Parse `--dump`; self-elevate; declare DPI awareness; own the `Overlay` instance and run the message loop |
| `overlay.rs` | The whole UI: window creation, DPI handling, layout, GDI drawing, native menu, drag and message handling |
| `config.rs` | `Config`/`Layout`/`Spacing`/`Align` load-save, defaults, legacy-directory migration |
| `i18n.rs` | `Language`, string tables, system-language selection |
| `format.rs` | Speed / percent / temperature formatting (including roll-over rules) |
| `tray.rs` | Owns the tray icon and swaps in the menu that `overlay` builds |
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

## 6. Window, DPI and layout

- **DPI awareness first.** Before any window exists, `main.rs` calls `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` (falling back to `SetProcessDPIAware`). Without this Windows bitmap-stretches the window and blurs the text. `GetDpiForWindow` then yields the real DPI (`144` at 150%), and `WM_DPICHANGED` re-scales, moves to the OS-suggested rectangle and rebuilds the fonts and DIB when the window crosses to another monitor.
- **Fixed width from the metric set.** All geometry is derived from `scale = dpi / 96`. The name column is the widest visible label and the value column the widest value each visible metric can render, measured with `GetTextExtentPoint32W` in a scratch DC; the panel is then `margin + label + gap + value + margin` wide. Because the reserve follows the format maxima (`999.9 MB/s`, `100%`, `100°C`) rather than the live data, the width is **constant**: it changes only when a metric is shown/hidden or the language changes, never as the values change.
- **Point-based gap.** The name↔value gap is defined in typographic points (`PT = 96/72` logical units) and rounded **up** to a whole pixel, so it is a physical 2 pt on every monitor regardless of resolution and scaling.
- **Whole-pixel geometry.** Every rectangle and the panel size are integers, so the spans and the panel always agree (no 1 px clipping) and the rows fall on the pixel grid, which keeps GDI text crisp.
- **Alignment and spacing.** `align` maps to `DrawTextW` flags (`left`/`center`/`right`) for the name and value inside each cell. `spacing` sets the vertical gap between rows (tight 1 / loose 2 logical units).
- **Opacity.** The panel is filled with `opacity * 255` as its alpha; the metric text is forced opaque, so the numbers stay high-contrast even on a light desktop.

## 7. Configuration and migration

- Path `%APPDATA%\deskpulse\config.toml`; `#[serde(default)]` fills missing fields with defaults, so new options (like `align`) do not break an existing file.
- On first run, if the new path is missing, the app reads the old path `%APPDATA%\desk-stats\config.toml` and migrates it (the project used to be named `desk-stats`).
- `language` is an `Option`: when unset it is chosen from the system UI language (`GetUserDefaultUILanguage`).

## 8. Packaging

- `build.rs` embeds `assets/icon.ico` and version info with `winresource`.
- `.cargo/config.toml` enables `+crt-static` for the msvc target, removing the VC++ runtime dependency.
- release: `lto` + `codegen-units=1` + `strip` + `panic="abort"`.

## 9. Key trade-offs

1. **The GUI was rewritten from `egui` / `wgpu` to a native Win32/GDI window.** The framework dragged in a GPU API and rendered every frame; on the dev machine the same overlay used a 125 MB working set with the OpenGL backend and 420 MB with WARP, at 3–40× the CPU. Drawing one small DIB by hand and pushing it with `UpdateLayeredWindow` costs ≈ 27 MB private / ≈ 43 MB working set and ≈ 0.29 % of one core, and it runs where no GPU API exists at all (Basic Display adapter, VMs, RDP).
2. **CPU temperature moved from WMI to a direct PawnIO read.** Newer LibreHardwareMonitor (0.9.x) removed the WMI provider, so the old `root\LibreHardwareMonitor` approach no longer works; going further, to drop the dependency on a resident app we read the PawnIO driver directly. LHM HTTP is only a fallback.
3. **GPU uses NVML only.** No generic PDH fallback; non-NVIDIA GPUs show `--`.
4. **No custom driver, no embedded hidden helper.** The first is too costly to maintain; the second is treated as malware by AV.
5. **Unknown means `--`.** Never substitute `0`.

## 10. Known limitations / not done

- Direct temperature reading needs admin; without it the app falls back to LHM (`--` if not installed).
- The Intel temperature path is implemented but not verified on an Intel machine (the dev machine is AMD).
- Multiple GPUs: only `device_by_index(0)` is used.
- No history graphs, no log persistence, only zh/en.
