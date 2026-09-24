# deskpulse

English | [中文](README.zh.md)

A pure-Rust Windows desktop overlay that runs entirely on the CPU and shows live system status:

- Upload / download speed
- CPU usage + CPU temperature
- Memory usage
- GPU usage, VRAM and GPU temperature (NVIDIA)

Extras: three layouts (vertical / horizontal / two-column), two spacing presets, left/center/right text alignment, selectable metrics, Chinese/English, system tray, launch at logon, persistent configuration.

Tech stack: pure Rust, drawn on a native Win32 layered window with GDI. **No GPU API** (no OpenGL / Direct3D / Vulkan) and no C++ runtime, shipped as a single-file exe.

> Tech stack, architecture and trade-offs: [ARCHITECTURE.md](ARCHITECTURE.md).

## Build and run

```powershell
cargo run --release
```

A floating window appears on the desktop; drag it with the mouse. **Right-click** opens the settings menu: Show/Hide, a metric picker, Layout, Spacing, Align, Opacity, Language, Start with Windows and Quit (the metric picker and the five settings groups are submenus). The tray icon opens the very same menu, built by the same code.

## Performance

Measured on the development machine, with the desktop otherwise idle:

| Test machine | |
| --- | --- |
| CPU | AMD Ryzen 5 9600X (6 cores / 12 threads) |
| Memory | 31 GB |
| OS | Windows 11, **150 %** display scaling |
| Overlay | 6 metrics shown, 1 s refresh |

| Overlay cost | Value |
| --- | --- |
| CPU | **≈ 0.29 % of one core** (≈ 0.02 % of the whole CPU) |
| Private memory | **≈ 27 MB** |
| Working set | ≈ 43 MB |
| Threads | 4–7 |
| Handles | ≈ 290 |
| `deskpulse.exe` | **0.97 MB** |

That is roughly **3 ms of CPU time per second** — effectively invisible in Task Manager, and far below a single frame of a typical animated UI.

Why it is this cheap:

- **Nothing renders unless the data changes.** The sampler thread posts a message after each new snapshot (once per second by default); the window redraws only then. There is no continuous render loop and no animation.
- **The whole panel is one small bitmap.** The background and the text are composited into a ~160×180 px 32-bit DIB in ordinary memory and handed to Windows with a single `UpdateLayeredWindow` call.
- **No GPU API at all.** The UI never touches OpenGL / Direct3D / Vulkan, so it also works on machines with only the Microsoft Basic Display adapter, inside virtual machines and over Remote Desktop.

For comparison, the earlier `egui` / `wgpu` builds of the same overlay used ≈ 125 MB (OpenGL backend) and ≈ 420 MB (WARP backend) of working set, and an order of magnitude more CPU.

## UI behaviour

- **Fixed width sized to the worst case**: the name column is sized from the labels and the value column from the widest value each visible metric can render (`999.9 MB/s`, `100%`, `100°C`), both measured with `GetTextExtentPoint32W`. The width is decided once by the metric set, so the panel never grows or twitches as the data changes, and it is never wider than the worst case it can display.
- **DPI aware**: the process declares per-monitor-v2 DPI awareness. The panel and fonts are laid out at the monitor's native pixel grid, and everything re-scales when the window is dragged to a monitor with a different scaling factor.
- **Point-based gap**: the name↔value gap is a physical 2 pt (rounded up to a whole pixel), so it keeps the same physical size at any resolution and display scaling.
- **Text alignment**: left / center / right, applied to each metric's name and value inside its cell.
- **Three layouts**: vertical (one item per row), horizontal (all items in one row), two-column (two items per row; default order Up/Down, CPU/CPU T, Mem/GPU, VRAM/GPU T).
- **Two spacing presets**: tight / loose — the vertical gap between rows.
- **Dark and translucent**: near-black panel with a configurable alpha (default `0.72`); the metric text always stays opaque.
- **Selectable metrics**: tick items under "Show"; hidden items take no space and the panel shrinks accordingly.
- **Chinese/English**: on first run the language follows the Windows UI language (English systems → English, otherwise Chinese); switch anytime under "Language".
- **Speed format**: at most 3 integer digits and 1 decimal (`5.9 KB/s`, `999.9 KB/s`); when the integer part is 0, 2 decimals (`0.98 KB/s`); more than 3 integer digits rolls over to the next unit (`1023.9 KB/s` → `1.00 MB/s`).

Diagnostic mode (no window, prints 5 samples and exits):

```powershell
cargo run -- --dump
```

## Packaging a standalone exe

`cargo build --release` produces `target\release\deskpulse.exe`, a single distributable file:

- **App icon and version info** embedded by `build.rs` + `winresource` from `assets/icon.ico`.
- **No VC++ runtime dependency**: `.cargo\config.toml` enables `+crt-static` for `x86_64-pc-windows-msvc`.
- **Size optimised**: release uses `lto`, `codegen-units = 1`, `strip`, `panic = "abort"`.
- **No console window**: release builds use `windows_subsystem = "windows"`.

```powershell
cargo build --release
Copy-Item .\target\release\deskpulse.exe .\dist\deskpulse.exe
```

`dist\deskpulse.exe` runs on double-click or can be copied elsewhere. To regenerate the icon: `pwsh -File .\assets\make-icon.ps1`.

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
    │   └── dpi.rs           # DPI awareness and per-monitor re-scaling
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
        ├── gpu.rs           # NVIDIA NVML: usage / VRAM / temperature
        ├── pawnio.rs        # CPU temperature straight from the PawnIO driver
        └── temp.rs          # temperature: PawnIO first, LHM HTTP fallback
```

## Configuration

Path: `%APPDATA%\deskpulse\config.toml`.

| Field | Description |
| --- | --- |
| `layout` | `vertical`, `horizontal` or `grid` (two-column) |
| `spacing` | `tight` (default) or `loose` — the vertical gap between rows |
| `align` | `left` (default), `center` or `right` — text alignment inside each cell |
| `position` | Top-left window position; saved after dragging |
| `refresh_secs` | Sampling interval in seconds |
| `opacity` | Panel alpha, 0.0–1.0; default `0.72` (translucent). The menu offers six steps; the field still accepts any value. Text stays opaque. |
| `autostart` | Launch at logon (maps to the `deskpulse` scheduled task) |
| `lhm_port` | LibreHardwareMonitor HTTP port (fallback), default `8085` |
| `language` | `zh` or `en`; empty means auto-detect from the system language on first run |
| `visible` | Per-metric visibility table; keys `net_up` / `net_down` / `cpu` / `cpu_temp` / `mem` / `gpu` / `vram` / `gpu_temp`; missing keys are shown |

`visible` example (show only CPU and memory):

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

## Metrics and data sources

| Metric | Source | Measured locally |
| --- | --- | --- |
| Upload/download speed | `sysinfo` network byte-counter diff | OK |
| CPU usage | `sysinfo` | OK |
| CPU temperature | PawnIO driver first, else LHM HTTP | OK (Tctl/Tdie) |
| Memory usage | `sysinfo` | OK |
| GPU usage / VRAM / temperature | NVIDIA NVML | OK |

Details about CPU temperature (why a kernel driver is required, the PawnIO protocol, the elevation requirement) are in [ARCHITECTURE.md](ARCHITECTURE.md).

## Known limitations

- **Reading CPU temperature directly requires administrator rights** (a PawnIO device restriction); without elevation it falls back to LHM over HTTP.
- GPU metrics depend on the NVIDIA driver (NVML). On non-NVIDIA GPUs these items show `--`.
- Unknown metrics always show `--`; `0` is never substituted for an unknown value.

## License

[MIT](LICENSE) © 2026 XIONG Shihao
