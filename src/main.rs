#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod config;
mod diag;
mod elevate;
mod format;
mod i18n;
mod metrics;
mod tray;
mod window;

use std::time::Duration;

use eframe::egui;

fn main() -> eframe::Result {
    if std::env::args().any(|arg| arg == "--dump") {
        dump();
        return Ok(());
    }

    // CPU temperature needs the PawnIO driver, which requires elevation.
    // Relaunch elevated if we are not already (no-op under the logon task).
    if elevate::ensure_elevated() {
        return Ok(());
    }

    diag::reset("deskpulse started");

    let config = config::Config::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("deskpulse")
        .with_inner_size(config.layout.window_size())
        .with_min_inner_size([80.0, 40.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false);
    if let Some([x, y]) = config.position {
        viewport = viewport.with_position([x, y]);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "deskpulse",
        options,
        Box::new(move |cc| Ok(Box::new(app::DeskStatsApp::new(cc, config)))),
    )
}

/// Headless diagnostic: print five samples and exit. Useful for checking which
/// metrics are actually available on this machine without opening a window.
fn dump() {
    let shared = metrics::spawn(Duration::from_secs(1), config::Config::load().lhm_port);
    for _ in 0..5 {
        std::thread::sleep(Duration::from_secs(1));
        let Ok(snapshot) = shared.lock().map(|guard| guard.clone()) else {
            continue;
        };
        println!(
            "net-up={:<12} net-down={:<12} cpu={:<5} cpu-temp={:<6} mem={:<5} gpu={:<5} vram={:<5} gpu-temp={}",
            format::format_speed(snapshot.net_up_bps),
            format::format_speed(snapshot.net_down_bps),
            format::format_percent(snapshot.cpu_usage),
            format::format_temp(snapshot.cpu_temp_c),
            format::format_percent(snapshot.mem_percent()),
            format::format_percent(snapshot.gpu_usage),
            format::format_percent(snapshot.vram_percent()),
            format::format_temp(snapshot.gpu_temp_c),
        );
    }
}
