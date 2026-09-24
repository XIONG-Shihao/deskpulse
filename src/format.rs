const SPEED_UNITS: [&str; 5] = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];

pub fn format_speed(bytes_per_sec: Option<f64>) -> String {
    let Some(mut value) = bytes_per_sec.filter(|v| v.is_finite() && *v >= 0.0) else {
        return "--".to_string();
    };

    // Keep at most three digits before the decimal point; roll over to the next
    // unit otherwise (e.g. 1023.9 KB/s -> ~1.00 MB/s).
    let mut unit = 0;
    while value >= 999.95 && unit < SPEED_UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    // Below one unit show two decimals ("0.98"), otherwise one ("999.9").
    if value < 1.0 {
        format!("{value:.2} {}", SPEED_UNITS[unit])
    } else {
        format!("{value:.1} {}", SPEED_UNITS[unit])
    }
}

pub fn format_percent(value: Option<f32>) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{v:.0}%"),
        _ => "--".to_string(),
    }
}

pub fn format_temp(celsius: Option<f32>) -> String {
    match celsius {
        Some(v) if v.is_finite() => format!("{v:.0}\u{00B0}C"),
        _ => "--".to_string(),
    }
}

/// Present rate of the foreground application, e.g. `144 FPS`.
pub fn format_frame_rate(fps: Option<f32>) -> String {
    match fps {
        Some(v) if v.is_finite() && v >= 0.0 => format!("{v:.0} FPS"),
        _ => "--".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_compacts_to_three_integer_digits() {
        assert_eq!(format_speed(Some(1024.0 * 5.9)), "5.9 KB/s");
        assert_eq!(format_speed(Some(1024.0 * 187.0)), "187.0 KB/s");
        assert_eq!(format_speed(Some(1024.0 * 999.9)), "999.9 KB/s");
        // 4 integer digits roll over to the next unit.
        assert_eq!(format_speed(Some(1024.0 * 1023.9)), "1.00 MB/s");
        // Below one unit: two decimals.
        assert_eq!(format_speed(Some(1024.0 * 0.98)), "0.98 KB/s");
        assert_eq!(format_speed(None), "--");
    }

    #[test]
    fn frame_rate_rounds_to_whole_frames() {
        assert_eq!(format_frame_rate(Some(143.6)), "144 FPS");
        assert_eq!(format_frame_rate(Some(0.0)), "0 FPS");
        assert_eq!(format_frame_rate(None), "--");
        assert_eq!(format_frame_rate(Some(f32::NAN)), "--");
    }
}
