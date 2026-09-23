use tray_icon::menu::Menu;
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Owns the tray icon.
///
/// The menu itself is built by the app (the same builder produces the window's
/// right-click menu), and is swapped in again whenever a check mark or the
/// language changes.
pub struct Tray {
    icon: TrayIcon,
}

impl Tray {
    pub fn new(menu: Menu) -> Option<Self> {
        let icon = make_icon()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("deskpulse")
            .with_icon(icon)
            .build()
            .ok()?;

        Some(Self { icon: tray })
    }

    /// Replaces the menu, e.g. after a check mark or the language changed.
    pub fn set_menu(&self, menu: Menu) {
        self.icon.set_menu(Some(Box::new(menu)));
    }
}

/// A simple cyan disc so the app has a recognizable tray icon without shipping
/// an asset file.
fn make_icon() -> Option<Icon> {
    const SIZE: u32 = 32;
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = center;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > radius {
                continue;
            }
            let offset = ((y * SIZE + x) * 4) as usize;
            rgba[offset] = 40;
            rgba[offset + 1] = 200;
            rgba[offset + 2] = 210;
            rgba[offset + 3] = if distance > radius - 2.0 { 110 } else { 255 };
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).ok()
}
