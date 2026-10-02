//! User-triggerable actions and their side effects.

use crate::{
    cli::Cli,
    daemon::{Snapshot, Status},
};
use anyhow::{anyhow, bail, Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
    Reload,
    SetPassword,
    Pair,
    OpenSettings,
    OpenDaemonSettings,
    EnableWebUi,
    OpenWebUi,
    OpenDaemonConfig,
    OpenLog,
    Quit,
}

/// Windows the tray can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Password,
    Pair,
    /// The tray's own settings.
    Settings,
    /// The daemon's address, relay and WebUI. Opens with the WebUI box ticked when asked to.
    Daemon {
        enable_web_ui: bool,
    },
}

impl Action {
    const ALL: [Action; 13] = [
        Action::Start,
        Action::Stop,
        Action::Restart,
        Action::Reload,
        Action::SetPassword,
        Action::Pair,
        Action::OpenSettings,
        Action::OpenDaemonSettings,
        Action::EnableWebUi,
        Action::OpenWebUi,
        Action::OpenDaemonConfig,
        Action::OpenLog,
        Action::Quit,
    ];

    /// Stable menu-item id (the Slint tray sends these); the inverse of `from_id`.
    pub fn id(self) -> String {
        format!("{self:?}")
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    /// The transitional status shown while this action's command runs.
    pub fn busy_status(self) -> Option<Status> {
        match self {
            Action::Start => Some(Status::Starting),
            Action::Stop => Some(Status::Stopping),
            Action::Restart => Some(Status::Restarting),
            Action::Reload => Some(Status::Reloading),
            _ => None,
        }
    }

    pub fn dialog(self) -> Option<Dialog> {
        match self {
            Action::SetPassword => Some(Dialog::Password),
            Action::Pair => Some(Dialog::Pair),
            Action::OpenSettings => Some(Dialog::Settings),
            Action::OpenDaemonSettings => Some(Dialog::Daemon {
                enable_web_ui: false,
            }),
            Action::EnableWebUi => Some(Dialog::Daemon {
                enable_web_ui: true,
            }),
            _ => None,
        }
    }
}

/// Runs an action's side effect. Dialog and quit actions are handled by the app.
pub fn execute(action: Action, cli: &Cli, snapshot: Option<&Snapshot>) -> Result<()> {
    match action {
        Action::Start => cli.output(&["daemon", "start"]).map(drop),
        Action::Stop => cli.output(&["daemon", "stop"]).map(drop),
        Action::Restart => cli.output(&["daemon", "restart"]).map(drop),
        Action::Reload => cli.output(&["reload"]).map(drop),
        Action::OpenWebUi => {
            let url = snapshot
                .and_then(Snapshot::web_ui_url)
                .ok_or_else(|| anyhow!("the daemon is not serving a WebUI"))?;
            open_detached(&url)
        }
        Action::OpenDaemonConfig => open_detached(&daemon_file(snapshot, |s| {
            s.home.clone().map(|h| h.join("config.json"))
        })?),
        Action::OpenLog => open_detached(&daemon_file(snapshot, |s| s.log_path.clone())?),
        Action::SetPassword
        | Action::Pair
        | Action::OpenSettings
        | Action::OpenDaemonSettings
        | Action::EnableWebUi
        | Action::Quit => {
            bail!("{action:?} is handled by the app, not executed")
        }
    }
}

fn daemon_file(
    snapshot: Option<&Snapshot>,
    pick: impl Fn(&Snapshot) -> Option<PathBuf>,
) -> Result<PathBuf> {
    snapshot
        .and_then(pick)
        .context("daemon location unknown (is the paseo CLI available?)")
}

fn open_detached(target: &(impl AsRef<std::ffi::OsStr> + ?Sized)) -> Result<()> {
    open::that_detached(target).context("opening with the system handler")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for action in Action::ALL {
            assert_eq!(Action::from_id(&action.id()), Some(action));
        }
    }
}
