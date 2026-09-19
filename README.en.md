# deskpulse

English | [中文](README.md)

A pure-Rust Windows desktop overlay that shows live system status:

- Upload / download speed
- CPU usage + CPU temperature
- Memory usage
- GPU usage, VRAM and GPU temperature (NVIDIA)

Extras: three layouts (vertical / horizontal / two-column), two spacing presets, selectable metrics, Chinese/English, system tray, launch at logon, persistent configuration.

Tech stack: pure Rust (GUI via `egui` / `eframe`), no C++ runtime, single-file exe.

> Tech stack, architecture and trade-offs: [ARCHITECTURE.en.md](ARCHITECTURE.en.md).

## Build and run

```powershell
cargo run --release
```

A floating window appears on the desktop; drag it with the mouse. **Right-click** opens the settings menu (top level: Show / Layout / Spacing / Language / Start with Windows / Quit; the first four expand into submenus). The tray menu offers the same actions.

### UI behaviour

- **The window hugs its content**: every frame measures the content and resizes the window to it, so there is no wasted transparent area and no stray click region.
- **Fixed cell widths**: changing digit counts (`9.9 KB/s` → `1.02 MB/s`) does not make the window width jitter.
- **Right-click popup menu**: the menu is its own small window (two-level hierarchy), tightly fitted with no wasted transparent area; the main window keeps its data size. Close it by choosing an item, right-clicking again, clicking outside, pressing Esc, or losing focus.
- **Dark theme**: a high-contrast dark palette is forced (near-black background, near-white text), independent of the system light theme.
- **Selectable metrics**: tick items under "Show"; hidden items take no space and the window shrinks accordingly.
- **Chinese/English**: on first run the language is chosen from the Windows UI language (English systems → English, otherwise Chinese); switch anytime under "Language".
- **Three layouts**: vertical (one item per row), horizontal (all items in one row), two-column (two items per row; default order Up/Down, CPU/CPU T, Mem/GPU, VRAM/GPU T).
- **Two spacing presets**: tight (default) uses narrow cells with labels right-aligned and values left-aligned, a ~4pt label↔value gap and no gap between the two columns; loose is the original centered wide cells (~47pt).
- **Speed format**: at most 3 integer digits and 1 decimal (`5.9 KB/s`, `999.9 KB/s`); when the integer part is 0, 2 decimals (`0.98 KB/s`); more than 3 integer digits rolls over to the next unit (`1023.9 KB/s` → `1.00 MB/s`).

Diagnostic mode (no window, prints 5 samples and exits):

```powershell
cargo run -- --dump
```

## Packaging a standalone exe

`cargo build --release` produces `target\release\deskpulse.exe`, a single distributable file:

- **App icon and version info** embedded by `build.rs` + `winresource` from `assets/icon.ico`.
- **No VC++ runtime dependency**: `.cargo\config.toml` enables `+crt-static` for `x86_64-pc-windows-msvc`.
- **Size optimised**: release uses `lto`, `codegen-units = 1`, `strip`, `panic = "abort"`; about 5.9 MB.
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
    ├── app.rs               # eframe App: UI, layout, menu, window fitting
    ├── config.rs            # config load/save + legacy-dir migration
    ├── i18n.rs              # zh/en strings + system-language detection
    ├── format.rs            # speed / percent / temperature formatting
    ├── tray.rs              # tray icon and menu
    ├── autostart.rs         # logon scheduled task (schtasks)
    ├── elevate.rs           # relaunch via UAC when not elevated
    ├── diag.rs              # %APPDATA%\deskpulse\diag.log
    └── metrics/
        ├── mod.rs           # Snapshot + collector thread + percentages
        ├── net.rs           # sysinfo network byte diff (filters virtual NICs)
        ├── cpu.rs           # sysinfo CPU usage
        ├── mem.rs           # sysinfo memory
        ├── gpu.rs           # NVIDIA NVML: usage / VRAM / temperature
        ├── pawnio.rs        # CPU temperature straight from the PawnIO driver
        └── temp.rs          # temperature: PawnIO first, LHM HTTP fallback
```

## Configuration

Path: `%APPDATA%\deskpulse\config.toml`.

| Field | Description |
| --- | --- |
| `layout` | `vertical`, `horizontal` or `grid` (two-column) |
| `spacing` | `tight` (default) or `loose` |
| `position` | Top-left window position; saved after dragging |
| `refresh_secs` | Sampling interval in seconds |
| `opacity` | Background alpha, 0.0–1.0 |
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

Details about CPU temperature (why a kernel driver is required, the PawnIO protocol, the elevation requirement) are in [ARCHITECTURE.en.md](ARCHITECTURE.en.md).

## Known limitations

- **Reading CPU temperature directly requires administrator rights** (a PawnIO device restriction); without elevation it falls back to LHM over HTTP.
- GPU metrics depend on the NVIDIA driver (NVML). On non-NVIDIA GPUs these items show `--`.
- Unknown metrics always show `--`; `0` is never substituted for an unknown value.

## License

[MIT](LICENSE) © 2026 XIONG Shihao
