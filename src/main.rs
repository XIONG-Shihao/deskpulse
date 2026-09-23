#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod config;
mod diag;
mod elevate;
mod format;
mod i18n;
mod metrics;
mod overlay;
mod tray;

use std::time::Duration;

fn main() {
    if std::env::args().any(|arg| arg == "--dump") {
        dump();
        return;
    }

    // CPU temperature needs the PawnIO driver, which requires elevation.
    // Relaunch elevated if we are not already (no-op under the logon task).
    if elevate::ensure_elevated() {
        return;
    }

    diag::reset("deskpulse started");
    overlay::enable_dpi_awareness();

    let config = config::Config::load();
    // The window procedure reaches the instance through a global pointer.
    let instance = Box::into_raw(Box::new(overlay::Overlay::new(config)));
    // SAFETY: single-threaded setup; the pointer lives until the loop exits.
    unsafe {
        overlay::set_instance(instance);
        (*instance).init_window();
        (*instance).start_metrics();
        (*instance).run();
        drop(Box::from_raw(instance));
    }
}

/// Headless diagnostic: print five samples and exit.
fn dump() {
    let shared = metrics::spawn(
        Duration::from_secs(1),
        config::Config::load().lhm_port,
        || {},
    );
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
