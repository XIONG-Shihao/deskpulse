use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::Layout;

/// User settings persisted to `%APPDATA%\deskpulse\config.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub layout: Layout,
    /// Top-left window position in physical pixels, if it has been moved yet.
    pub position: Option<[f32; 2]>,
    /// Refresh interval for the metric sampler, in seconds.
    pub refresh_secs: u64,
    /// Background alpha, 0.0 (invisible) ..= 1.0 (opaque).
    pub opacity: f32,
    pub autostart: bool,
    /// Port of LibreHardwareMonitor's HTTP server (used for CPU temperature).
    pub lhm_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            layout: Layout::Vertical,
            position: None,
            refresh_secs: 1,
            opacity: 0.72,
            autostart: false,
            lhm_port: 8085,
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("deskpulse").join("config.toml"))
    }

    /// Location used before the project was renamed; read once for migration.
    fn legacy_path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("desk-stats").join("config.toml"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::path()
            && let Ok(text) = std::fs::read_to_string(path)
            && let Ok(config) = toml::from_str::<Self>(&text)
        {
            return config;
        }

        // First run under the new name: adopt the pre-rename config if present.
        if let Some(legacy) = Self::legacy_path()
            && let Ok(text) = std::fs::read_to_string(legacy)
            && let Ok(config) = toml::from_str::<Self>(&text)
        {
            config.save();
            return config;
        }

        Self::default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Layout;

    #[test]
    fn round_trips_all_fields() {
        let config = Config {
            layout: Layout::Horizontal,
            position: Some([12.0, 34.0]),
            refresh_secs: 2,
            opacity: 0.5,
            autostart: true,
            lhm_port: 8085,
        };
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.layout, Layout::Horizontal);
        assert_eq!(back.position, Some([12.0, 34.0]));
        assert_eq!(back.refresh_secs, 2);
        assert!(back.autostart);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let back: Config = toml::from_str("layout = \"horizontal\"\n").unwrap();
        assert_eq!(back.layout, Layout::Horizontal);
        assert_eq!(back.refresh_secs, 1);
        assert!(back.position.is_none());
    }
}

