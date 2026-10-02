//! Thin wrapper over the `paseo` executable. Every invocation goes through
//! `argv`, so the program path and `--home` handling live in one place.

use crate::config::Settings;
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use std::{
    ffi::OsString,
    io::ErrorKind,
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Debug, Clone)]
pub struct Cli {
    program: OsString,
    home: Option<PathBuf>,
}

impl Cli {
    pub fn new(settings: &Settings) -> Self {
        Self {
            program: settings
                .cli_path
                .clone()
                .map_or_else(|| "paseo".into(), Into::into),
            home: settings.home.clone(),
        }
    }

    /// Full command line, program first. `--home` goes last because it is
    /// declared on each subcommand rather than reliably inherited.
    pub fn argv(&self, args: &[&str]) -> Vec<OsString> {
        let home = self
            .home
            .iter()
            .flat_map(|h| [OsString::from("--home"), h.clone().into_os_string()]);
        std::iter::once(self.program.clone())
            .chain(args.iter().map(OsString::from))
            .chain(home)
            .collect()
    }

    /// Runs to completion and returns stdout; a non-zero exit becomes an error
    /// carrying paseo's own message.
    pub fn output(&self, args: &[&str]) -> Result<String> {
        let argv = self.argv(args);
        let out = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| match e.kind() {
                ErrorKind::NotFound => anyhow::anyhow!(
                    "paseo CLI not found ({}); set `cli_path` in the tray config or add it to PATH",
                    argv[0].to_string_lossy()
                ),
                _ => anyhow::Error::from(e).context("running paseo"),
            })?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            return Ok(stdout);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let message = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("no output");
        bail!("`paseo {}` failed: {message}", args.join(" "))
    }

    pub fn json<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T> {
        let text = self.output(args)?;
        serde_json::from_str(&text)
            .with_context(|| format!("parsing `paseo {}` output", args.join(" ")))
    }
}
