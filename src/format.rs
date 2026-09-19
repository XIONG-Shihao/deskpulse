const SPEED_UNITS: [&str; 5] = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];

fn scale(value: f64, units: &'static [&'static str; 5]) -> (f64, &'static str) {
    let mut v = value;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    (v, units[i])
}

pub fn format_speed(bytes_per_sec: Option<f64>) -> String {
    match bytes_per_sec {
        Some(v) if v.is_finite() && v >= 0.0 => {
            let (scaled, unit) = scale(v, &SPEED_UNITS);
            if scaled < 10.0 {
                format!("{scaled:.2} {unit}")
            } else if scaled < 100.0 {
                format!("{scaled:.1} {unit}")
            } else {
                format!("{scaled:.0} {unit}")
            }
        }
        _ => "--".to_string(),
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
