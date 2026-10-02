//! "Launch at login". The OS-level registration (XDG entry, LaunchAgent,
//! registry key) is the only source of truth for the toggle.

use anyhow::{Context, Result};
use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};

fn handle() -> Result<AutoLaunch> {
    let exe = std::env::current_exe().context("cannot determine own executable path")?;
    AutoLaunchBuilder::new()
        .set_app_name("paseo-tray")
        .set_app_path(&exe.to_string_lossy())
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
        .build()
        .context("configuring launch at login")
}

pub fn is_enabled() -> bool {
    handle().and_then(|h| Ok(h.is_enabled()?)).unwrap_or(false)
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let handle = handle()?;
    if enabled {
        handle.enable()
    } else {
        handle.disable()
    }
    .context("updating launch at login")
}
