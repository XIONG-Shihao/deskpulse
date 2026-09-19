use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::app::Layout;

/// Owns the tray icon and translates menu clicks into app actions.
pub struct Tray {
    _icon: TrayIcon,
    autostart_item: CheckMenuItem,
}

pub enum TrayAction {
    ToggleVisible,
    SetLayout(Layout),
    SetAutostart(bool),
    Quit,
}

impl Tray {
    pub fn new(autostart_checked: bool) -> Option<Self> {
        let menu = Menu::new();

        let toggle = MenuItem::with_id("toggle", "显示 / 隐藏", true, None);
        let vertical = MenuItem::with_id("layout_v", "竖排", true, None);
        let horizontal = MenuItem::with_id("layout_h", "横排", true, None);
        let layout_menu = Submenu::new("布局", true);
        let autostart_item =
            CheckMenuItem::with_id("autostart", "开机自启", true, autostart_checked, None);
        let quit = MenuItem::with_id("quit", "退出", true, None);

        menu.append(&toggle).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        layout_menu.append(&vertical).ok()?;
        layout_menu.append(&horizontal).ok()?;
        menu.append(&layout_menu).ok()?;
        menu.append(&autostart_item).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit).ok()?;

        let icon = make_icon()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("deskpulse")
            .with_icon(icon)
            .build()
            .ok()?;

        Some(Self {
            _icon: tray,
            autostart_item,
        })
    }

    /// Drain pending menu events. Requires the caller's thread to be pumping
    /// the Win32 message queue, which eframe's event loop does.
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id.0.as_str() {
                "toggle" => actions.push(TrayAction::ToggleVisible),
                "layout_v" => actions.push(TrayAction::SetLayout(Layout::Vertical)),
                "layout_h" => actions.push(TrayAction::SetLayout(Layout::Horizontal)),
                "autostart" => {
                    actions.push(TrayAction::SetAutostart(self.autostart_item.is_checked()))
                }
                "quit" => actions.push(TrayAction::Quit),
                _ => {}
            }
        }
        actions
    }

    pub fn set_autostart_checked(&self, checked: bool) {
        self.autostart_item.set_checked(checked);
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
