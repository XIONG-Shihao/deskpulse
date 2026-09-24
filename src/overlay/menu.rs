//! The control menu, shared by the right-click menu and the tray.

use std::sync::Arc;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::config::{Align, Layout, Spacing};
use crate::i18n::Language;

use super::metric::Metric;
use super::*;

/// Panel opacity presets offered by the menu. Six steps, keeping the historical
/// default (0.72) as one of them; the config field still accepts any value.
const OPACITY_LEVELS: [f32; 6] = [0.30, 0.45, 0.60, 0.72, 0.85, 1.00];

impl Overlay {
    pub(super) fn install_menu_handler(&self) {
        let hwnd = self.hwnd;
        let queue = Arc::clone(&self.menu_ids);
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Ok(mut queue) = queue.lock() {
                queue.push(event.id.0.clone());
            }
            // SAFETY: posting to our own window.
            unsafe {
                PostMessageW(hwnd, WM_APP_MENU, 0, 0);
            }
        }));
    }

    /// Builds the full control menu.
    ///
    /// The same builder feeds the window's right-click menu and the tray menu,
    /// so the two can never drift apart.
    pub(super) fn build_menu(&self) -> Menu {
        let t = self.text();
        let menu = Menu::new();
        let show = Submenu::new(t.metrics, true);
        for metric in Metric::ALL {
            let item = CheckMenuItem::with_id(
                metric.id(),
                metric.label(&t),
                true,
                self.is_visible_metric(metric),
                None,
            );
            let _ = show.append(&item);
        }
        let _ = menu.append(&show);

        let layout = Submenu::new(t.layout, true);
        for (id, label, selected) in [
            (
                "layout_vertical",
                t.vertical,
                self.config.layout == Layout::Vertical,
            ),
            (
                "layout_horizontal",
                t.horizontal,
                self.config.layout == Layout::Horizontal,
            ),
            ("layout_grid", t.grid, self.config.layout == Layout::Grid),
        ] {
            let _ = layout.append(&CheckMenuItem::with_id(id, label, true, selected, None));
        }
        let _ = menu.append(&layout);

        let spacing = Submenu::new(t.spacing, true);
        for (id, label, selected) in [
            (
                "spacing_loose",
                t.spacing_loose,
                self.config.spacing == Spacing::Loose,
            ),
            (
                "spacing_tight",
                t.spacing_tight,
                self.config.spacing == Spacing::Tight,
            ),
        ] {
            let _ = spacing.append(&CheckMenuItem::with_id(id, label, true, selected, None));
        }
        let _ = menu.append(&spacing);

        let align = Submenu::new(t.align, true);
        for (id, label, selected) in [
            ("align_left", t.align_left, self.config.align == Align::Left),
            (
                "align_center",
                t.align_center,
                self.config.align == Align::Center,
            ),
            (
                "align_right",
                t.align_right,
                self.config.align == Align::Right,
            ),
        ] {
            let _ = align.append(&CheckMenuItem::with_id(id, label, true, selected, None));
        }
        let _ = menu.append(&align);

        let opacity = Submenu::new(t.opacity, true);
        for level in OPACITY_LEVELS {
            let percent = (level * 100.0).round() as u32;
            let id = format!("opacity_{percent}");
            let label = format!("{percent}%");
            let selected = (self.config.opacity - level).abs() < 0.005;
            let _ = opacity.append(&CheckMenuItem::with_id(id, label, true, selected, None));
        }
        let _ = menu.append(&opacity);

        let language = Submenu::new(t.language, true);
        let _ = language.append(&CheckMenuItem::with_id(
            "lang_zh",
            "中文",
            true,
            self.language == Language::Zh,
            None,
        ));
        let _ = language.append(&CheckMenuItem::with_id(
            "lang_en",
            "English",
            true,
            self.language == Language::En,
            None,
        ));
        let _ = menu.append(&language);

        let _ = menu.append(&CheckMenuItem::with_id(
            "autostart",
            t.autostart,
            true,
            self.config.autostart,
            None,
        ));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("toggle", t.tray_toggle, true, None));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&MenuItem::with_id("quit", t.quit, true, None));

        menu
    }

    pub(super) fn show_context_menu(&self) {
        let menu = self.build_menu();
        // SAFETY: called on the thread that owns the window.
        unsafe {
            menu.show_context_menu_for_hwnd(self.hwnd, None);
        }
    }

    /// Rebuilds the tray menu so its check marks and labels match the state.
    pub(super) fn refresh_tray(&self) {
        if let Some(tray) = self.tray.as_ref() {
            tray.set_menu(self.build_menu());
        }
    }

    pub(super) fn apply_menu_ids(&mut self) {
        let ids = self
            .menu_ids
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();
        for id in ids {
            match id.as_str() {
                "toggle" => self.toggle_visible(),
                "layout_vertical" => self.set_layout(Layout::Vertical),
                "layout_horizontal" => self.set_layout(Layout::Horizontal),
                "layout_grid" => self.set_layout(Layout::Grid),
                "spacing_loose" => self.set_spacing(Spacing::Loose),
                "spacing_tight" => self.set_spacing(Spacing::Tight),
                "align_left" => self.set_align(Align::Left),
                "align_center" => self.set_align(Align::Center),
                "align_right" => self.set_align(Align::Right),
                "lang_zh" => self.set_language(Language::Zh),
                "lang_en" => self.set_language(Language::En),
                "autostart" => {
                    let enabled = !self.config.autostart;
                    self.set_autostart(enabled);
                }
                "quit" => {
                    // SAFETY: destroy our own window.
                    unsafe { DestroyWindow(self.hwnd) };
                    return;
                }
                other => {
                    if let Some(opacity) = opacity_from_menu_id(other) {
                        self.set_opacity(opacity);
                    } else if let Some(metric) = Metric::from_id(other) {
                        let visible = !self.is_visible_metric(metric);
                        self.set_visible_metric(metric, visible);
                    }
                }
            }
        }
        // Keep the tray menu's check marks and labels in sync with the change.
        self.refresh_tray();
    }

    pub(super) fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        // SAFETY: show/hide our own window.
        unsafe {
            ShowWindow(
                self.hwnd,
                if self.visible {
                    SW_SHOWNOACTIVATE
                } else {
                    SW_HIDE
                },
            );
        }
        if self.visible {
            self.ensure_topmost();
            self.refresh();
        }
    }

    pub(super) fn set_layout(&mut self, layout: Layout) {
        if self.config.layout == layout {
            return;
        }
        self.config.layout = layout;
        self.config.save();
        self.refresh();
    }

    pub(super) fn set_spacing(&mut self, spacing: Spacing) {
        if self.config.spacing == spacing {
            return;
        }
        self.config.spacing = spacing;
        self.config.save();
        self.refresh();
    }

    pub(super) fn set_align(&mut self, align: Align) {
        if self.config.align == align {
            return;
        }
        self.config.align = align;
        self.config.save();
        self.refresh();
    }

    pub(super) fn set_opacity(&mut self, opacity: f32) {
        if (self.config.opacity - opacity).abs() < 0.005 {
            return;
        }
        self.config.opacity = opacity;
        self.config.save();
        self.refresh();
    }

    pub(super) fn set_language(&mut self, language: Language) {
        if self.language == language {
            return;
        }
        self.language = language;
        self.config.language = Some(language);
        self.config.save();
        self.refresh();
    }

    pub(super) fn set_autostart(&mut self, enabled: bool) {
        if self.autostart.set(enabled) {
            self.config.autostart = enabled;
        }
        self.config.save();
    }

    // ------------------------------------------------------------ messages
}
/// Parses a menu id such as `opacity_72` into an alpha in `0.0..=1.0`.
pub(super) fn opacity_from_menu_id(id: &str) -> Option<f32> {
    let percent: u32 = id.strip_prefix("opacity_")?.parse().ok()?;
    (1..=100).contains(&percent).then(|| percent as f32 / 100.0)
}

#[cfg(test)]
mod tests {
    use super::opacity_from_menu_id;

    #[test]
    pub(super) fn parses_opacity_menu_ids() {
        assert_eq!(opacity_from_menu_id("opacity_72"), Some(0.72));
        assert_eq!(opacity_from_menu_id("opacity_30"), Some(0.30));
        assert_eq!(opacity_from_menu_id("opacity_100"), Some(1.0));
        // Out-of-range, malformed and unrelated ids are ignored.
        assert_eq!(opacity_from_menu_id("opacity_0"), None);
        assert_eq!(opacity_from_menu_id("opacity_101"), None);
        assert_eq!(opacity_from_menu_id("opacity_72x"), None);
        assert_eq!(opacity_from_menu_id("opacity_"), None);
        assert_eq!(opacity_from_menu_id("cpu"), None);
        assert_eq!(opacity_from_menu_id(""), None);
    }
}
