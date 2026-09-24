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

/// Which settings set is in use.
///
/// The two sets are independent, so a game preset (fewer metrics, another
/// corner, more opaque) can coexist with the desktop one and be switched with
/// two clicks instead of being retyped every time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Desktop,
    Game,
}

/// Default panel opacity, kept from before the modes existed.
pub const DEFAULT_OPACITY: f32 = 0.72;

/// The settings that differ between the modes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeSettings {
    pub layout: Layout,
    /// Cell spacing preset.
    pub spacing: Spacing,
    /// Alignment of the metric text inside each cell.
    pub align: Align,
    /// Top-left window position in physical pixels, if it has been moved yet.
    pub position: Option<[f32; 2]>,
    /// Background alpha, 0.0 (invisible) ..= 1.0 (opaque).
    pub opacity: f32,
    /// Per-metric visibility, keyed by `Metric::id`. Missing keys fall back to
    /// each metric's own default (visible, except the frame rate).
    pub visible: BTreeMap<String, bool>,
}

impl Default for ModeSettings {
    fn default() -> Self {
        Self {
            layout: Layout::Vertical,
            spacing: Spacing::Tight,
            align: Align::Left,
            position: None,
            opacity: DEFAULT_OPACITY,
            visible: BTreeMap::new(),
        }
    }
}

/// User settings persisted to `%APPDATA%\deskpulse\config.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The mode whose settings are in use; remembered across restarts.
    pub mode: Mode,
    pub desktop: ModeSettings,
    pub game: ModeSettings,
    /// Refresh interval for the metric sampler, in seconds.
    pub refresh_secs: u64,
    pub autostart: bool,
    /// Port of LibreHardwareMonitor's HTTP server (used for CPU temperature).
    pub lhm_port: u16,
    /// `None` until resolved, so the system language can be detected on first run.
    pub language: Option<Language>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Desktop,
            desktop: ModeSettings::default(),
            game: ModeSettings::default(),
            refresh_secs: 1,
            autostart: false,
            lhm_port: 8085,
            language: None,
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

    /// The settings of the mode currently in use.
    pub fn active(&self) -> &ModeSettings {
        match self.mode {
            Mode::Desktop => &self.desktop,
            Mode::Game => &self.game,
        }
    }

    /// The settings of the mode currently in use, for editing.
    pub fn active_mut(&mut self) -> &mut ModeSettings {
        match self.mode {
            Mode::Desktop => &mut self.desktop,
            Mode::Game => &mut self.game,
        }
    }

    pub fn load() -> Self {
        let (mut config, save_needed) = match Self::read(Self::path()) {
            Some((config, migrated)) => (config, migrated),
            // First run under the new name: adopt the pre-rename config.
            None => match Self::read(Self::legacy_path()) {
                Some((config, _)) => (config, true),
                None => (Self::default(), false),
            },
        };

        if config.language.is_none() {
            config.language = Some(Language::system_default());
        }

        if save_needed {
            config.save();
        }
        config
    }

    fn read(path: Option<PathBuf>) -> Option<(Self, bool)> {
        let path = path?;
        let text = std::fs::read_to_string(path).ok()?;
        Self::parse(&text)
    }

    /// Parses a config file. The flag reports whether the file was still in the
    /// flat, pre-modes format and therefore needs saving back.
    fn parse(text: &str) -> Option<(Self, bool)> {
        let mut config: Config = toml::from_str(text).ok()?;
        let flat: Flat = toml::from_str(text).unwrap_or_default();
        let migrated = flat.any();
        if migrated {
            flat.apply(&mut config.desktop);
            // The game preset starts as a copy of the desktop one, so switching
            // modes never changes what is shown until the user edits it.
            config.game = config.desktop.clone();
        }
        Some((config, migrated))
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

/// The settings as they were before the modes existed: all of them lived at the
/// top level of the file and now belong to the desktop mode. Every field is
/// optional so a file with none of them is simply "not legacy".
#[derive(Default, Deserialize)]
#[serde(default)]
struct Flat {
    layout: Option<Layout>,
    spacing: Option<Spacing>,
    align: Option<Align>,
    position: Option<[f32; 2]>,
    opacity: Option<f32>,
    visible: Option<BTreeMap<String, bool>>,
}

impl Flat {
    fn any(&self) -> bool {
        self.layout.is_some()
            || self.spacing.is_some()
            || self.align.is_some()
            || self.position.is_some()
            || self.opacity.is_some()
            || self.visible.is_some()
    }

    fn apply(self, settings: &mut ModeSettings) {
        if let Some(value) = self.layout {
            settings.layout = value;
        }
        if let Some(value) = self.spacing {
            settings.spacing = value;
        }
        if let Some(value) = self.align {
            settings.align = value;
        }
        if let Some(value) = self.position {
            settings.position = Some(value);
        }
        if let Some(value) = self.opacity {
            settings.opacity = value;
        }
        if let Some(value) = self.visible {
            settings.visible = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        let mut visible = BTreeMap::new();
        visible.insert("gpu".to_owned(), false);
        Config {
            mode: Mode::Game,
            desktop: ModeSettings {
                opacity: 0.5,
                ..ModeSettings::default()
            },
            game: ModeSettings {
                layout: Layout::Horizontal,
                spacing: Spacing::Loose,
                align: Align::Center,
                position: Some([12.0, 34.0]),
                opacity: 0.85,
                visible: visible.clone(),
            },
            refresh_secs: 2,
            autostart: true,
            lhm_port: 8085,
            language: Some(Language::En),
        }
    }

    #[test]
    fn round_trips_all_fields() {
        let config = sample();
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.mode, Mode::Game);
        assert_eq!(back.desktop.opacity, 0.5);
        assert_eq!(back.game.layout, Layout::Horizontal);
        assert_eq!(back.game.align, Align::Center);
        assert_eq!(back.game.position, Some([12.0, 34.0]));
        assert_eq!(back.refresh_secs, 2);
        assert!(back.autostart);
        assert_eq!(back.language, Some(Language::En));
        assert_eq!(back.game.visible, sample().game.visible);
    }

    #[test]
    fn active_follows_the_mode() {
        let mut config = sample();
        assert_eq!(config.active().opacity, 0.85);
        config.mode = Mode::Desktop;
        assert_eq!(config.active().opacity, 0.5);
        config.active_mut().opacity = 0.9;
        assert_eq!(config.desktop.opacity, 0.9);
        assert_eq!(config.game.opacity, 0.85);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let (back, migrated) = Config::parse("game = { layout = \"grid\" }\n").unwrap();
        assert!(!migrated);
        assert_eq!(back.mode, Mode::Desktop);
        assert_eq!(back.game.layout, Layout::Grid);
        assert_eq!(back.game.opacity, DEFAULT_OPACITY);
        assert_eq!(back.desktop.layout, Layout::Vertical);
        assert_eq!(back.refresh_secs, 1);
        assert!(back.desktop.position.is_none());
        assert!(back.language.is_none());
        assert!(back.desktop.visible.is_empty());
    }

    /// A file written before the modes existed keeps its settings: they move
    /// into the desktop mode, and the game mode starts as a copy of them.
    #[test]
    fn migrates_the_flat_format_into_the_desktop_mode() {
        let text = r#"
layout = "horizontal"
align = "right"
position = [40.0, 50.0]
opacity = 0.45
refresh_secs = 3

[visible]
cpu = false
"#;
        let (config, migrated) = Config::parse(text).unwrap();
        assert!(migrated);
        assert_eq!(config.desktop.layout, Layout::Horizontal);
        assert_eq!(config.desktop.align, Align::Right);
        assert_eq!(config.desktop.position, Some([40.0, 50.0]));
        assert_eq!(config.desktop.opacity, 0.45);
        assert_eq!(config.desktop.visible.get("cpu"), Some(&false));
        assert_eq!(config.refresh_secs, 3);
        // the game mode is a copy, so nothing changes on a mode switch
        assert_eq!(config.game.layout, Layout::Horizontal);
        assert_eq!(config.game.visible, config.desktop.visible);
        // and a mode that was never mentioned stays on its default
        assert_eq!(config.mode, Mode::Desktop);

        // Saving writes the new shape, so the migration happens only once.
        let saved = toml::to_string_pretty(&config).unwrap();
        assert!(!Config::parse(&saved).unwrap().1);
    }
}
