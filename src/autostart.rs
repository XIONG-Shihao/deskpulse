use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const TASK_NAME: &str = "deskpulse";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Manages the logon scheduled task that launches the app with highest
/// privileges.
///
/// Elevation is required for CPU temperature: the PawnIO kernel driver only
/// accepts requests from an elevated process. A scheduled task with
/// `RunLevel Highest` gives that without a UAC prompt at every logon, which a
/// plain `HKCU\...\Run` entry cannot do.
pub struct Autostart {
    exe: Option<PathBuf>,
}

impl Autostart {
    pub fn new() -> Self {
        Self {
            exe: std::env::current_exe().ok(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        Command::new("schtasks")
            .args(["/Query", "/TN", TASK_NAME])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    pub fn set(&self, enabled: bool) -> bool {
        let Some(exe) = self.exe.as_ref() else {
            return false;
        };

        let status = if enabled {
            Command::new("schtasks")
                .args([
                    "/Create",
                    "/TN",
                    TASK_NAME,
                    "/TR",
                    &format!("\"{}\"", exe.display()),
                    "/SC",
                    "ONLOGON",
                    "/RL",
                    "HIGHEST",
                    "/F",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
        } else {
            Command::new("schtasks")
                .args(["/Delete", "/TN", TASK_NAME, "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
        };

        status.map(|status| status.success()).unwrap_or(false)
    }
}
