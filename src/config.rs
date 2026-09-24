use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::Language;

/// Overlay layout, selectable from the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Horizontal,
    Grid,
    #[default]
    Vertical,
}

/// Cell spacing preset, selectable from the menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spacing {
    Loose,
    #[default]
    Tight,
}

/// How each metric's name and value are aligned inside their cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// User settings persisted to `%APPDATA%\deskpulse\config.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub layout: Layout,
    /// Cell spacing preset.
    pub spacing: Spacing,
    /// Alignment of the metric text inside each cell.
    pub align: Align,
    /// Top-left window position in physical pixels, if it has been moved yet.
    pub position: Option<[f32; 2]>,
    /// Refresh interval for the metric sampler, in seconds.
    pub refresh_secs: u64,
    /// Background alpha, 0.0 (invisible) ..= 1.0 (opaque).
    pub opacity: f32,
    pub autostart: bool,
    /// Port of LibreHardwareMonitor's HTTP server (used for CPU temperature).
    pub lhm_port: u16,
    /// `None` until resolved, so the system language can be detected on first run.
    pub language: Option<Language>,
    /// Per-metric visibility, keyed by `Metric::id`. Missing keys fall back to
    /// each metric's own default (visible, except the frame rate).
    pub visible: BTreeMap<String, bool>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            layout: Layout::Vertical,
            spacing: Spacing::Tight,
            align: Align::Left,
            position: None,
            refresh_secs: 1,
            opacity: 0.72,
            autostart: false,
            lhm_port: 8085,
            language: None,
            visible: BTreeMap::new(),
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
        let (mut config, migrated) = match Self::read(Self::path()) {
            Some(config) => (config, false),
            // First run under the new name: adopt the pre-rename config.
            None => match Self::read(Self::legacy_path()) {
                Some(config) => (config, true),
                None => (Self::default(), false),
            },
        };

        if config.language.is_none() {
            config.language = Some(Language::system_default());
        }

        if migrated {
            config.save();
        }
        config
    }

    fn read(path: Option<PathBuf>) -> Option<Self> {
        let path = path?;
        let text = std::fs::read_to_string(path).ok()?;
        toml::from_str(&text).ok()
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

    #[test]
    fn round_trips_all_fields() {
        let mut visible = BTreeMap::new();
        visible.insert("gpu".to_owned(), false);
        let config = Config {
            layout: Layout::Horizontal,
            spacing: Spacing::Loose,
            align: Align::Center,
            position: Some([12.0, 34.0]),
            refresh_secs: 2,
            opacity: 0.5,
            autostart: true,
            lhm_port: 8085,
            language: Some(Language::En),
            visible: visible.clone(),
        };
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.layout, Layout::Horizontal);
        assert_eq!(back.align, Align::Center);
        assert_eq!(back.position, Some([12.0, 34.0]));
        assert_eq!(back.refresh_secs, 2);
        assert!(back.autostart);
        assert_eq!(back.language, Some(Language::En));
        assert_eq!(back.visible, visible);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let back: Config = toml::from_str("layout = \"horizontal\"\n").unwrap();
        assert_eq!(back.layout, Layout::Horizontal);
        assert_eq!(back.align, Align::Left);
        assert_eq!(back.refresh_secs, 1);
        assert!(back.position.is_none());
        assert!(back.language.is_none());
        assert!(back.visible.is_empty());
    }
}
