use auto_launch::{AutoLaunch, AutoLaunchBuilder};

const APP_NAME: &str = "deskpulse";

/// Thin wrapper around the `Run` registry key entry used for launching at login.
pub struct Autostart {
    inner: Option<AutoLaunch>,
}

impl Autostart {
    pub fn new() -> Self {
        let inner = std::env::current_exe().ok().and_then(|exe| {
            AutoLaunchBuilder::new()
                .set_app_name(APP_NAME)
                .set_app_path(&exe.to_string_lossy())
                .build()
                .ok()
        });
        Self { inner }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner
            .as_ref()
            .and_then(|auto| auto.is_enabled().ok())
            .unwrap_or(false)
    }

    pub fn set(&self, enabled: bool) -> bool {
        let Some(auto) = self.inner.as_ref() else {
            return false;
        };
        let result = if enabled { auto.enable() } else { auto.disable() };
        result.is_ok()
    }
}
