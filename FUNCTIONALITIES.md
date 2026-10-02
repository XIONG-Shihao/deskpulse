# deskpulse — Functionalities

English | [中文](docs/FUNCTIONALITIES.zh.md)

Everything the overlay does, in detail. [README.md](README.md) is the short version; the tech stack, architecture and trade-offs are in [ARCHITECTURE.md](ARCHITECTURE.md).

## Metrics

- **Up / Down** — current network throughput, physical adapters only: loopback, virtual and VPN interfaces are filtered out.
- **CPU** — total CPU usage. **Temp** is the CPU temperature (package / Tctl).
- **Mem** — used physical memory as a percentage.
- **GPU / GPU T** — GPU utilisation and temperature (NVIDIA, via NVML).
- **VRAM** — video memory in use, from the GPU performance counters.

**VRAM counts borrowed memory too.** The figure is the device-wide committed amount — dedicated memory **plus** what the GPU borrows from system RAM — so it can legitimately exceed 100 %. NVML's dedicated-only number, which is used as a fallback, can never. That is deliberate: "over 100 %" is exactly the state worth noticing.

**A value turns red** once it crosses its threshold: temperature over 80 °C, memory over 80 %, video memory over 100 %. Names stay muted so the values keep the visual lead.

**Unknown is `--`, never `0`.** Every metric is optional internally; one that cannot be read is shown as `--` rather than a plausible-looking zero. GPU usage and GPU temperature need the NVIDIA driver; the remaining metrics, VRAM included, work on any adapter.

## Panel

### Layout

- **Three layouts**: vertical (one metric per row), horizontal (all in one row), and two-column (two per row; the default order is Up/Down, CPU/Temp, Mem/GPU, VRAM/GPU T).
- **Two spacing presets** control the vertical gap between rows: tight and loose.
- **Alignment** — left / centred / right — applies to the name and the value inside each cell.
- **Fixed width, sized to the worst case.** The name column is the widest visible label and the value column the widest string each visible metric can render (`999.9 MB/s`, `100%`, `100°C`), both measured with `GetTextExtentPoint32W` in the fonts actually used. The width follows the metric set and the language, never the live data, so the panel does not grow or twitch as the numbers change.
- **Metrics are selectable**: hiding one removes its row and narrows the panel accordingly.

### Size and position

- **DPI aware.** The process declares per-monitor-v2 DPI awareness, so Windows never bitmap-stretches the window; the panel and its fonts are laid out on the monitor's native pixel grid and everything is recalculated when the window crosses to a monitor with a different scaling factor.
- **Scale.** The layout scale is `monitor DPI / 96 × scale_percent / 100`. `100 %` follows Windows' display scaling exactly, and the menu offers 50/75/100/125/150/175 % **on top of** it: on a 150 % desktop, `150 %` means 2.25× the fonts of a 100 % desktop and `50 %` means three quarters of it. Fonts, row height, margins, corner radius and the reserved column widths all scale together, and the file accepts any `scale_percent` from 25 to 400.
- **Keep on screen.** On by default: the whole panel stays inside the monitor it is on — while it is dragged, after it grows, when it moves to another monitor, and at startup (a saved position can be stale by then). The panel is only ever moved, never resized, and one larger than the monitor is pinned to the top-left corner. Turning the setting off allows parking it partly off screen.
- **Dragging** with the left mouse button moves the panel; the position is saved as soon as it is dropped.

### Appearance

- **Six opacity steps** — 30 / 45 / 60 / 72 (the default) / 85 / 100 %. Only the background is translucent; the text is always drawn fully opaque, so the numbers stay readable at every step.
- **Speed format**: at most three integer digits and one decimal (`999.9 KB/s`); below one unit, two decimals (`0.98 KB/s`); roll-over to the next unit rather than a fourth digit (`1023.9 KB/s` → `1.00 MB/s`).
- **Language**: Chinese or English, picked from the Windows UI language on first run and switchable at any time.

## Menu and tray

The right-click menu and the tray icon open the same menu, built by the same code, so the two can never drift apart. The entries themselves are listed in the README's [Interface](README.md#interface) section; two details are worth adding:

- **Show / Hide** hides the panel without stopping the process, so it keeps sampling and reappears instantly.
- **The tray icon is always reachable**, even while the panel is hidden or parked off screen, which makes it the way back from any state.

## Configuration file

Settings live in `%APPDATA%\deskpulse\config.toml`. The menu rewrites this file on every change, so hand edits are best made while the overlay is closed; values outside a field's range are clamped when the file is read.

| Field | Meaning |
| --- | --- |
| `layout` | `vertical`, `horizontal` or `grid` (two-column) |
| `spacing` | `tight` (default) or `loose` |
| `align` | `left` (default), `center` or `right` |
| `position` | top-left corner in physical pixels; written when the panel is dragged |
| `keep_on_screen` | `true` (default) keeps the whole panel inside its monitor |
| `refresh_secs` | sampling interval in seconds — no menu entry |
| `opacity` | background alpha, 0.0–1.0; the menu offers six steps, the field accepts any value |
| `scale_percent` | UI scale in per cent, 25–400; `100` follows Windows' display scaling |
| `autostart` | mirrors the logon scheduled task |
| `lhm_port` | LibreHardwareMonitor's HTTP port, used only as the temperature fallback — no menu entry |
| `language` | `zh` or `en`; empty means auto-detect on the next start |
| `visible` | per-metric switches keyed `net_up` / `net_down` / `cpu` / `cpu_temp` / `mem` / `gpu` / `vram` / `gpu_temp`; a missing key means visible |

For example, to show only CPU and memory:

```toml
[visible]
net_up = false
net_down = false
cpu = true
cpu_temp = false
mem = true
gpu = false
vram = false
gpu_temp = false
```

## Performance, and why it is this cheap

Six metrics shown, one sample per second, Windows 11 at 150 % display scaling. The overlay costs roughly **3 ms of CPU time per second** — effectively invisible in Task Manager, and far below a single frame of a typical animated UI.

- **Nothing renders unless the data changes.** The sampler thread posts a message after each new snapshot (once per second by default); the window redraws only then. There is no continuous render loop and no animation.
- **The whole panel is one small bitmap.** The background and the text are composited into a ~160×180 px 32-bit DIB in ordinary memory and handed to Windows with a single `UpdateLayeredWindow` call.
- **No GPU API at all.** The UI never touches OpenGL / Direct3D / Vulkan, so it also works on machines with only the Microsoft Basic Display adapter, inside virtual machines and over Remote Desktop.

For comparison, the earlier `egui` / `wgpu` builds of the same overlay used ≈ 125 MB (OpenGL backend) and ≈ 420 MB (WARP backend) of working set, and an order of magnitude more CPU.

## Packaging and distribution

- **Single file**: `cargo build --release` produces `target\release\deskpulse.exe`, which runs on double-click and can be copied anywhere — there is no installer and no configuration to ship.
- **App icon and version info** are embedded by `build.rs` + `winresource` from `assets/icon.ico`; `pwsh -File .\assets\make-icon.ps1` regenerates the icon.
- **No VC++ runtime dependency**: `.cargo\config.toml` enables `+crt-static` for `x86_64-pc-windows-msvc`.
- **Size optimised**: the release profile uses `lto`, `codegen-units = 1`, `strip` and `panic = "abort"`.
- **No console window**: release builds use `windows_subsystem = "windows"`.
- **Diagnostic mode**: `deskpulse.exe --dump` opens no window, prints five samples and exits.
