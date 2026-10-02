//! Tray settings: the only persistent state owned by this app.
//! Daemon configuration stays in paseo's own `config.json`.

use crate::fsutil;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Explicit path to the `paseo` executable. `None` searches `PATH`.
    pub cli_path: Option<PathBuf>,
    /// Daemon home passed as `--home`. `None` uses paseo's default (`~/.paseo`).
    pub home: Option<PathBuf>,
    /// Start the daemon when the tray launches (if it is not already running).
    pub auto_start_daemon: bool,
    /// Stop the daemon when the tray quits.
    pub auto_stop_daemon: bool,
    /// Seconds between status polls.
    pub poll_interval_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cli_path: None,
            home: None,
            auto_start_daemon: true,
            auto_stop_daemon: true,
            poll_interval_secs: 5,
        }
    }
}

impl Settings {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs.max(1))
    }
}

pub fn path() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("paseo-tray").join("config.toml"))
        .context("cannot determine the user config directory")
}

/// Loads settings, falling back to defaults (and reporting why) on a broken file.
pub fn load() -> Settings {
    let loaded = path().and_then(|p| match fs::read_to_string(&p) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("parsing {}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", p.display())),
    });
    loaded.unwrap_or_else(|e| {
        eprintln!("paseo-tray: using default settings: {e:#}");
        Settings::default()
    })
}

pub fn save(settings: &Settings) -> Result<()> {
    write(&path()?, settings)
}

fn write(path: &Path, settings: &Settings) -> Result<()> {
    fsutil::write_atomically(path, &toml::to_string_pretty(settings)?)
        .with_context(|| format!("writing {}", path.display()))
}
