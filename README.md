# deskpulse

[中文](docs/README.zh.md)

A pure-Rust Windows desktop overlay that runs entirely on the CPU and shows live system status:

- Upload / download speed
- CPU usage + CPU temperature
- Memory usage
- GPU usage + GPU temperature (NVIDIA, via NVML)
- VRAM

Tech stack: pure Rust, drawn on a native Win32 layered window with GDI.

> Detailed features: [FUNCTIONALITIES.md](FUNCTIONALITIES.md) · tech stack, architecture and trade-offs: [ARCHITECTURE.md](ARCHITECTURE.md).

## Interface

Everything is configured from the right-click menu (the tray icon opens the same menu, built by the same code):

![Settings menu](docs/menu.png)

| Entry | What it does |
| --- | --- |
| **Show** ▸ | tick the metrics to display; a hidden metric takes no space, the panel shrinks |
| **Layout** ▸ | vertical / horizontal / two-column |
| **Spacing** ▸ | tight / loose gap between rows |
| **Align** ▸ | left / center / right for the name and the value inside each cell |
| **Opacity** ▸ | 30 / 45 / 60 / 72 / 85 / 100 % background alpha |
| **Scale** ▸ | 50 / 75 / 100 / 125 / 150 / 175 %, **on top of** the monitor's display scaling |
| **Language** ▸ | 中文 / English |
| **Keep on screen** | the panel cannot be dragged out of its monitor, and is pulled back in when it grows or the monitors change |
| **Start with Windows** | a logon scheduled task, so the CPU temperature keeps working without a UAC prompt |
| **Show / Hide** | hide the panel without quitting |
| **Quit** | exit |

### Opacity

Only the panel background is translucent; the text is always drawn fully opaque, so the numbers stay readable at every step. Here at 30 %, 45 %, 72 % (the default) and 100 %:

| 30 % | 45 % | 72 % (default) | 100 % |
| --- | --- | --- | --- |
| ![30 % opacity](docs/opacity-30.png) | ![45 % opacity](docs/opacity-45.png) | ![72 % opacity](docs/opacity-72.png) | ![100 % opacity](docs/opacity-100.png) |

## Build and run

```powershell
cargo run --release
```

A floating window appears on the desktop; drag it with the mouse. **Right-click** opens the settings menu (see [Interface](#interface)): which metrics to show, layout, spacing, alignment, opacity, scale, language, keep-on-screen and launch-at-logon switches, show/hide and quit.

Diagnostic mode (no window, prints five samples and exits):

```powershell
cargo run -- --dump
```

## Performance

Measured on the development machine, with the desktop otherwise idle:

| | |
| --- | --- |
| Processor | AMD Ryzen 5 9600X (6 cores / 12 threads) |
| CPU usage | **≈ 0.29 % of one core** (≈ 0.02 % of the whole CPU) |
| Private memory | **≈ 27 MB** |
| Working set | ≈ 43 MB |
| `deskpulse.exe` | **1.05 MB** |

## Packaging a standalone exe

```powershell
cargo build --release
Copy-Item .\target\release\deskpulse.exe .\dist\deskpulse.exe
```

## File structure

```
deskpulse/
├── Cargo.toml               # dependencies and release profile
├── build.rs                 # embeds icon / version info (winresource)
├── .cargo/config.toml       # x86_64-pc-windows-msvc: +crt-static
├── assets/
│   ├── icon.ico             # app icon
│   ├── make-icon.ps1        # icon generator
│   ├── AMDFamily17.bin      # PawnIO module (AMD SMN)
│   └── IntelMSR.bin         # PawnIO module (Intel MSR)
├── docs/                    # screenshots and the Chinese documentation
└── src/
    ├── main.rs              # entry, --dump diagnostic, self-elevation
    ├── overlay.rs           # module root: window lifecycle, message loop, drag
    ├── overlay/             # the UI, split by concern
    │   ├── win32.rs         # raw Win32 declarations (FFI blocks and value types)
    │   ├── metric.rs        # the metrics that can be shown
    │   ├── canvas.rs        # GDI bitmap/fonts and text measurement
    │   ├── layout.rs        # where each name/value rectangle goes
    │   ├── paint.rs         # one repaint
    │   ├── menu.rs          # control menu (right-click + tray) and its actions
    │   ├── topmost.rs       # keep the overlay above other topmost windows
    │   └── dpi.rs           # DPI awareness, layout scale, staying on screen
    ├── config.rs            # config load/save + legacy-dir migration
    ├── i18n.rs              # zh/en strings + system-language detection
    ├── format.rs            # speed / percent / temperature formatting
    ├── tray.rs              # tray icon; the menu comes from `overlay`
    ├── autostart.rs         # logon scheduled task (schtasks)
    ├── elevate.rs           # relaunch via UAC when not elevated
    ├── single_instance.rs   # one overlay per logon session
    ├── diag.rs              # %APPDATA%\deskpulse\diag.log
    ├── wide.rs              # NUL-terminated UTF-16 helper
    └── metrics/
        ├── mod.rs           # Snapshot + collector thread + percentages
        ├── net.rs           # sysinfo network byte diff (filters virtual NICs)
        ├── system.rs        # sysinfo CPU usage + memory (one System)
        ├── gpu.rs           # NVIDIA NVML: usage / temperature
        ├── gpu_mem.rs       # video memory from the GPU Adapter Memory counters
        ├── pawnio.rs        # CPU temperature straight from the PawnIO driver
        └── temp.rs          # temperature: PawnIO first, LHM HTTP fallback
```

## Metrics and data sources

| Metric | Source | Measured locally |
| --- | --- | --- |
| Upload/download speed | `sysinfo` network byte-counter diff | OK |
| CPU usage | `sysinfo` | OK |
| CPU temperature | PawnIO driver first, else LHM HTTP | OK (Tctl/Tdie) |
| Memory usage | `sysinfo` | OK |
| GPU usage / temperature | NVIDIA NVML | OK |
| VRAM | GPU Adapter Memory performance counters: dedicated + borrowed, so it can exceed 100 % (NVML dedicated-only as fallback) | OK (65 % vs 55 % dedicated-only on the same frame) |

Details about CPU temperature (why a kernel driver is required, the PawnIO protocol, the elevation requirement) are in [ARCHITECTURE.md](ARCHITECTURE.md).

## Known limitations

- **Reading CPU temperature directly requires administrator rights** (a PawnIO device restriction); without elevation it falls back to LHM over HTTP.
- **Exclusive fullscreen cannot be overlaid, and not every "full screen" is the same.** The panel is a per-pixel-alpha layered window, so it exists only where DWM composites. A real exclusive fullscreen — the one Windows flags as `QUNS_RUNNING_D3D_FULL_SCREEN` — takes the display away from the desktop and stops DWM compositing that screen, and then **no non-injected window** can be drawn there: Task Manager with "always on top" is not visible either, and neither is the Windows volume bar. Every tool that does show numbers over it (RTSS / MSI Afterburner, Steam, Discord, the WeGame overlay) injects a DLL into the game, hooks its Present and draws inside the game's own frame; deskpulse does not inject (anti-cheat blocks it, and it risks bans).
  - **Shows:** Black Myth: Wukong (UE5 / DX12), Apex (`r5apex_dx12.exe`, DX12). A DX12 "full screen" is still a DWM-composited borderless window, so the panel stays visible.
  - **Does not show:** League of Legends (old D3D9 true-exclusive path), VALORANT (configured for exclusive fullscreen). In those fullscreen modes the panel really is not on screen; borderless or windowed puts it back.
- GPU usage and temperature depend on the NVIDIA driver (NVML); a non-NVIDIA GPU shows `--` for those two. Video memory is read from the GPU performance counters, which exist for any adapter.
- Unknown metrics always show `--`; `0` is never substituted for an unknown value.

## License

[MIT](LICENSE) © 2026 XIONG Shihao
