# deskpulse — Functionalities

English | [中文](docs/FUNCTIONALITIES.zh.md)

The complete description of what deskpulse does: every metric it reads, how the panel behaves, what each setting does, where it keeps its state, what it costs, and how it is shipped. [README.md](README.md) is the two-minute version; [ARCHITECTURE.md](ARCHITECTURE.md) covers the internals and the trade-offs behind them.

- [1. Metrics](#1-metrics)
- [2. The panel](#2-the-panel)
- [3. Menu and tray](#3-menu-and-tray)
- [4. Configuration file](#4-configuration-file)
- [5. Startup, elevation and the logon task](#5-startup-elevation-and-the-logon-task)
- [6. Performance](#6-performance)
- [7. Packaging and distribution](#7-packaging-and-distribution)
- [8. Requirements and limits](#8-requirements-and-limits)
- [9. Diagnostics and files on disk](#9-diagnostics-and-files-on-disk)

## 1. Metrics

Eight metrics can be shown; the menu's **Show** submenu ticks them one by one, and a hidden metric costs nothing — its row disappears and the panel narrows.

| Metric | What it reports | Where it comes from |
| --- | --- | --- |
| Up / Down | Current network throughput, upload and download | `sysinfo` interface byte counters, time-differenced |
| CPU | Total CPU usage across all cores | `sysinfo` |
| Temp | CPU temperature (package / Tctl on AMD) | PawnIO driver, LibreHardwareMonitor as a fallback |
| Mem | Used physical memory, as a percentage of the total | `sysinfo` |
| GPU | GPU utilisation | NVIDIA NVML |
| VRAM | Video memory in use | Windows GPU performance counters, NVML as a fallback |
| GPU T | GPU temperature | NVIDIA NVML |

**Network speed counts physical adapters only.** Loopback, virtual adapters, VPN tunnels and Bluetooth links are filtered out by name, so a running tunnel does not inflate the numbers.

**VRAM counts borrowed memory too.** The figure is the device-wide committed amount: dedicated video memory **plus** whatever the GPU is borrowing from system RAM. That is why it can legitimately read above 100 %, and why that is the interesting case — it means the GPU has run out of its own memory and is spilling into system RAM, which costs bandwidth and frame time. NVML reports only the dedicated part, which mathematically can never exceed 100 %; it is used only when the performance counters are unavailable (non-NVIDIA adapters included).

**GPU usage and GPU temperature are the two metrics that need an NVIDIA card.** They come from NVML (the driver's own library). Every other metric, VRAM included, works on any adapter, and the two NVML-only metrics simply show `--` elsewhere.

**CPU temperature is the only metric that needs elevation.** Windows has no public API for it: the value lives in MSR (Intel) or SMN (AMD) registers, which can only be read from ring 0. deskpulse reads them through the PawnIO kernel driver when it is installed; otherwise it falls back to asking a running LibreHardwareMonitor over HTTP on `lhm_port`.

**A value turns red** once it crosses its threshold: temperature above 80 °C, memory above 80 %, video memory above 100 %. The names stay muted grey so the values keep the visual lead; nothing else changes colour.

**Unknown is `--`, never `0`.** Every metric is optional internally. A value that cannot be read is displayed as `--` rather than a zero that looks like real data — the same reason an idle network shows `0.00 B/s` (a real measurement) while a missing sensor shows `--`.

**Not shown, on purpose:** disk I/O, fan speeds, voltages, per-core CPU, per-process anything, and any kind of history. Each would either cost a driver, a service, or a much larger sampling surface than the light-weight sources above.

## 2. The panel

### 2.1 Layout

- **Three layouts.** *Vertical* puts one metric per row (the default). *Horizontal* puts every metric in a single row, name above value. *Two-column* puts two metrics per row, in the order Up/Down, CPU/Temp, Mem/GPU, VRAM/GPU T.
- **Two spacing presets.** *Tight* (default) leaves one logical pixel between rows, *loose* two. It is a vertical gap only; the columns are always tight.
- **Alignment** — left (default), centred or right — applies to the name and to the value inside each cell, so a right-aligned panel reads like a table.
- **Fixed width, sized to the worst case.** The name column is the widest visible label and the value column the widest string each visible metric can render (`999.9 MB/s`, `100 %`, `100 °C`), both measured with `GetTextExtentPoint32W` in the fonts actually used. The width therefore depends on *which* metrics are shown and on the language — not on the live data. Numbers changing never make the panel grow, shrink or twitch, and a value can never be clipped.
- **Text alignment is per cell**, so in the horizontal layout the name and the value share the column and stay centred on each other.

### 2.2 Size and position

- **DPI aware.** The process declares per-monitor-v2 DPI awareness before any window exists. Windows therefore never bitmap-stretches the panel — everything inside is drawn at the native pixel size — and the layout is recalculated when the window crosses to a monitor with a different scaling factor, so the panel keeps the same physical size everywhere.
- **Scale.** The layout scale is `monitor DPI / 96 × scale_percent / 100`. `100 %` follows Windows' display scaling exactly; the menu offers 50 / 75 / 100 / 125 / 150 / 175 % **on top of it**, and the file accepts 25–400.

  | `scale_percent` | On a 100 % desktop | On a 150 % desktop (this project's dev machine) |
  | --- | --- | --- |
  | 50 % | 0.50× | 0.75× |
  | 100 % | 1.00× | 1.50× |
  | 150 % | 1.50× | 2.25× |
  | 175 % | 1.75× | 2.63× |

  Everything derived from the scale moves together: font sizes (label 12 / value 14 logical pixels become 18 / 21 at 150 %), row height, margins, the name↔value gap (defined in typographic points, so it stays 2 pt physically), the rounded-corner radius and the reserved column widths.
- **Keep on screen** (on by default). The whole panel is held inside the monitor it is on, never just its top-left corner:
  - while it is dragged, the panel stops at the edge instead of sliding off;
  - after it grows (a higher scale, or more metrics);
  - after the window moves to another monitor;
  - at startup, because a saved position can be stale by then (the panel may have grown, or the monitor arrangement may have changed).
  The panel is only ever *moved*, never resized, and one larger than the monitor is pinned to the top-left corner rather than pushed around. Turning the setting off restores free placement, including parking the panel half off screen; the tray icon remains reachable, so there is always a way back.
- **Dragging** with the left mouse button moves the panel and writes the new position immediately, so the placement survives a crash, not just a clean exit.

### 2.3 Appearance

- **Six opacity steps**: 30 / 45 / 60 / 72 (the default) / 85 / 100 %. Only the panel background is translucent — the text is always composited fully opaque, which is what keeps the numbers readable over a bright wallpaper at 30 %.
- **Colours.** Metric names are muted grey (`#C0C0C0`); values are white, or red (`RGB 255,80,80`) once they cross a threshold. The background is black at the configured alpha.
- **Font** is Microsoft YaHei at weight 600, requested at the scaled pixel size, so CJK and Latin text are rendered by the same font and stay sharp at every scale.
- **Shape**: a rounded rectangle (radius 10 logical pixels, i.e. 15 physical at 150 %) with per-pixel alpha; no border, no shadow, no drop-in animation.
- **Language**: Chinese or English, chosen from the Windows UI language on first run, switchable from the menu at any time. Only the labels change; the layout then re-measures, because Chinese labels are narrower than their English counterparts.

## 3. Menu and tray

The right-click menu and the tray menu are the same menu, produced by one builder in one pass, so they cannot drift apart. The tray version exists so that the panel can be recovered and configured even when it is hidden or parked off screen.

| Entry | Values | Effect |
| --- | --- | --- |
| **Show** ▸ | one tick per metric | Toggles the metric; the panel re-measures and resizes immediately |
| **Layout** ▸ | Vertical / Horizontal / 2 cols | Switches the arrangement |
| **Spacing** ▸ | Tight / Loose | Vertical gap between rows |
| **Align** ▸ | Left / Centre / Right | Name and value alignment inside each cell |
| **Opacity** ▸ | 30 / 45 / 60 / 72 / 85 / 100 % | Background alpha |
| **Scale** ▸ | 50 / 75 / 100 / 125 / 150 / 175 % | Multiplies the monitor's display scaling |
| **Language** ▸ | 中文 / English | Switches every label |
| **Keep on screen** | tick | Locks the panel inside its monitor |
| **Start with Windows** | tick | Creates or removes the logon task |
| **Show / Hide** | — | Hides the panel; sampling continues |
| **Quit** | — | Exits, releasing the single-instance lock |

Every change is written to the configuration file immediately, and the check marks in both menus are rebuilt on the spot.

**Show / Hide keeps the process alive.** Hiding is not quitting: the samplers keep running, so the panel is instantly up to date when it reappears, and the tray icon stays. That also means a hidden panel still costs the figures in section 6.

## 4. Configuration file

Settings live in a single file, `%APPDATA%\deskpulse\config.toml`.

- **Nothing is written until something changes.** A fresh installation has no file at all; the first menu click (or drag) creates it with the current values.
- **The menu rewrites the file** on every change, so hand edits are best made while the overlay is closed — otherwise the next click overwrites them.
- **Out-of-range values are clamped when read**, and the file is rewritten with the clamped value, so a typo cannot produce an unusable panel (nor an unstartable one).

| Field | Type / values | Meaning |
| --- | --- | --- |
| `layout` | `vertical` (default), `horizontal`, `grid` | Arrangement |
| `spacing` | `tight` (default), `loose` | Row gap |
| `align` | `left` (default), `center`, `right` | Cell alignment |
| `position` | `[x, y]` in physical pixels | Top-left corner; written on drag |
| `keep_on_screen` | `true` (default) / `false` | Panel stays inside its monitor |
| `refresh_secs` | integer, default `1` | Sampling interval. **No menu entry** |
| `opacity` | `0.0`–`1.0`, default `0.72` | Background alpha; the menu offers six steps but the field accepts any value |
| `scale_percent` | integer 25–400, default `100` | UI scale on top of the display scaling |
| `autostart` | `true` / `false` | Mirrors the logon task's state |
| `lhm_port` | integer, default `8085` | LibreHardwareMonitor's HTTP port, used only as the temperature fallback. **No menu entry** |
| `language` | `zh` / `en`, or empty | Empty means "pick from the Windows UI language on the next start" |
| `visible` | table of `true` / `false` | One switch per metric, keyed `net_up`, `net_down`, `cpu`, `cpu_temp`, `mem`, `gpu`, `vram`, `gpu_temp`; a missing key means visible |

Two of those — `refresh_secs` and `lhm_port` — have no menu entry and can only be changed by editing the file. For example, to sample every two seconds and to point the fallback temperature source at another port:

```toml
refresh_secs = 2
lhm_port = 8086
```

*(`refresh_secs` is a whole number of seconds; 1 is the practical minimum.)*

And to show only CPU and memory:

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

A configuration written by an older build under the pre-rename directory (`%APPDATA%\desk-stats`) is adopted once on first run, and the file is then rewritten in the new location.

## 5. Startup, elevation and the logon task

- **One instance per logon session.** A named mutex guards the process; a second launch exits quietly instead of drawing a second panel.
- **Direct CPU temperature needs administrator rights**, which is a limitation of the PawnIO device. Started normally, deskpulse therefore relaunches itself through UAC. That prompt appears once per start, not per boot.
- **The autostart task avoids that prompt.** *Start with Windows* creates a scheduled task named `deskpulse` with `RunLevel Highest`, triggered at logon. The task runs elevated without a UAC prompt, which is why autostart is worth enabling even if you would otherwise start the overlay by hand. Turning the tick off deletes the task.
- **Everything else works unelevated.** If the elevation prompt is dismissed, the overlay still runs: CPU temperature falls back to LibreHardwareMonitor (or `--`), and the other seven metrics are unaffected.
- **The task is the only change made to the system.** No services, no drivers installed by deskpulse itself (PawnIO is a separate download), no registry keys, no shell hooks.

## 6. Performance

**What was measured.** The development machine, desktop otherwise idle, six metrics visible, one sample per second, Windows 11 with display scaling at 150 % — that last detail matters, because a higher scale means a larger bitmap and larger fonts to rasterise.

| | |
| --- | --- |
| Processor | AMD Ryzen 5 9600X (6 cores / 12 threads) |
| CPU usage | **≈ 0.29 % of one core** (≈ 0.02 % of the whole CPU) |
| Private memory | **≈ 27 MB** |
| Working set | ≈ 43 MB |
| `deskpulse.exe` | **1.05 MB** |
| Threads / handles | 4–7 / ≈ 290 |

**Reading the numbers.** *CPU usage* is CPU time consumed per second: 0.29 % of one core is about **3 ms per second** — of the order of a single frame of an animated UI, spread across a second. *Private memory* is memory that belongs to the process alone (≈ 27 MB, mostly the runtime and the GDI objects). *Working set* is what the process has resident in RAM, including the memory it shares with the system — the higher number, and the one Task Manager shows by default.

**Why it is this cheap.**

- **Nothing renders unless the data changes.** The sampler thread posts a message after each new snapshot (once per second by default); the window redraws only then, and does nothing at all in between. There is no render loop, no animation, and no repaint on a timer.
- **The whole panel is one small bitmap.** The background, the rounded corners and the text are composited into a ~160×180 pixel 32-bit DIB in ordinary memory — pure CPU work — and handed to the compositor with a single `UpdateLayeredWindow` call. One buffer allocation, one call per update.
- **No GPU API at all.** The UI never touches OpenGL, Direct3D or Vulkan. That is what makes it work on machines with only the Microsoft Basic Display adapter, inside virtual machines and over Remote Desktop, and it is also why it does not compete with a game for GPU time.

**For comparison**, the earlier `egui` / `wgpu` builds of the same overlay used ≈ 125 MB (OpenGL backend) and ≈ 420 MB (WARP backend) of working set and an order of magnitude more CPU. The reason is not that those frameworks are bad: it is that they render every frame and pull in a GPU API to draw a rectangle and five lines of text. Drawing one small DIB by hand does neither.

**What makes it cost more.** CPU rises roughly with `1 / refresh_secs` (twice a second means twice the sampling work) and with the number of visible metrics. Memory grows with the panel's *area*, so a large `scale_percent` costs more than the same metrics at 100 %. Nothing here is expensive in absolute terms, but two samples per second with eight metrics at 175 % is not the configuration the figures above describe.

## 7. Packaging and distribution

**Building a single file:**

```powershell
cargo build --release
Copy-Item .\target\release\deskpulse.exe .\dist\deskpulse.exe
```

`target\release\deskpulse.exe` is the whole product: a single ~1 MB executable with no installer, no companion DLLs and no runtime to install. `dist\deskpulse.exe` is just the copy used for everyday deployments; running it directly out of `target\release` works the same. Copy it anywhere — a USB stick, another machine, a different directory — and double-click it.

**What is baked in, and why:**

| Property | How | Why it matters |
| --- | --- | --- |
| App icon and version info | `build.rs` runs `winresource` over `assets/icon.ico` | The exe shows up properly in Explorer, Task Manager and the file's properties |
| No VC++ redistributable | `.cargo\config.toml` enables `+crt-static` for `x86_64-pc-windows-msvc` | The C runtime is linked in, so a clean Windows install can run the exe as-is |
| Small binary | release profile: `lto`, `codegen-units = 1`, `strip`, `panic = "abort"` | Link-time optimisation and single-unit codegen give the optimiser the whole program; stripping and abort-on-panic drop debug info and unwinding tables |
| No console window | `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` | A GUI-subsystem exe: double-clicking it never flashes a console. Debug builds keep the console for `--dump` |
| Diagnostic mode | `deskpulse.exe --dump` | Prints five samples and exits without opening a window; the output goes to the console it was started from |

**Regenerating the icon** (only needed after changing the artwork):

```powershell
pwsh -File .\assets\make-icon.ps1
```

**Uninstalling** is the reverse of the list above: quit from the menu, untick *Start with Windows* (or run `schtasks /Delete /TN deskpulse /F`), delete the exe, and delete `%APPDATA%\deskpulse` if the settings should go too.

## 8. Requirements and limits

- **Windows 10/11, 64-bit.** Built and tested on Windows 11 x64; the APIs used have been available since Windows 10 1703.
- **No GPU API and no GPU driver requirement** beyond the optional NVIDIA one for two metrics — see section 1.
- **Administrator rights** are needed only for reading the CPU temperature directly; without them that one metric falls back to LibreHardwareMonitor, or shows `--`.
- **Multiple GPUs.** Usage and temperature are read from adapter 0 (NVML); VRAM is read from whichever adapter the performance counters show as using the most dedicated memory, which is normally the one running the game.
- **An exclusive-fullscreen game cannot be overlaid, and not every "full screen" is the same.** The panel is a per-pixel-alpha layered window, so it exists only where DWM composites. A *real* exclusive fullscreen — the state Windows flags as `QUNS_RUNNING_D3D_FULL_SCREEN` — takes the display away from the desktop and stops DWM compositing that screen. Nothing that is not injected into the game can be drawn there: Task Manager's "always on top" is not visible either, and neither is the Windows volume bar. Every tool that does show numbers over it (RTSS / MSI Afterburner, Steam, Discord, the WeGame overlay) injects a DLL, hooks the game's Present and draws inside the game's own frame; deskpulse does not inject, because the anti-cheat systems in those games block injection and treat it as a ban risk.
  - **Shows:** Black Myth: Wukong (UE5 / DX12) and Apex (`r5apex_dx12.exe`, DX12). A DX12 "full screen" is still a DWM-composited borderless window — DX12 has no true exclusive mode in practice — so the panel stays visible.
  - **Does not show:** League of Legends (the old D3D9 true-exclusive path) and VALORANT (installed configured for exclusive fullscreen). In those modes the panel really is off screen; switching the game to borderless or windowed puts it back.
- **Not implemented:** history graphs, logging of past sessions, tray tooltips with live values, automatic switching of settings per game, and any language other than Chinese and English.

## 9. Diagnostics and files on disk

| Path | Content |
| --- | --- |
| The exe itself | Everything needed to run; it is portable and can live anywhere |
| `%APPDATA%\deskpulse\config.toml` | All settings (section 4); created on the first change |
| `%APPDATA%\deskpulse\diag.log` | Diagnostics: startup, DPI awareness, window and hook creation, tray state, temperature backend, and any error (a failed `UpdateLayeredWindow`, a missing foreground hook) |

`diag.log` is truncated on every start and appended to while running; when something looks wrong — the panel did not appear, the tray menu is missing, the temperature backend fell back — it is the first place to look.

There is one command-line flag, `--dump`: it starts the samplers, prints five lines of metrics to the console it was started from and exits. It opens no window and does not touch the running instance, which makes it the quickest way to check whether a metric is working on a machine.
