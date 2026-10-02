//! Observed daemon state: one `Snapshot` per poll, parsed from the paseo CLI.

use crate::{cli::Cli, daemon_config::Edit};
use serde::Deserialize;
use std::path::PathBuf;

/// What the tray shows. `Snapshot` only ever holds the first four; the
/// transitional ones are overlaid by `state::AppState` while a command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Checking,
    Running,
    Starting,
    Stopping,
    Restarting,
    Reloading,
    Stopped,
    Unavailable,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Checking => "checking…",
            Status::Running => "running",
            Status::Starting => "starting…",
            Status::Stopping => "stopping…",
            Status::Restarting => "restarting…",
            Status::Reloading => "reloading…",
            Status::Stopped => "stopped",
            Status::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub status: Status,
    pub listen: Option<String>,
    pub configured_listen: Option<String>,
    pub relay_enabled: bool,
    pub web_ui_enabled: bool,
    pub home: Option<PathBuf>,
    pub log_path: Option<PathBuf>,
    pub error: Option<String>,
}

impl Snapshot {
    fn unavailable(error: String) -> Self {
        Self {
            status: Status::Unavailable,
            listen: None,
            configured_listen: None,
            relay_enabled: false,
            web_ui_enabled: false,
            home: None,
            log_path: None,
            error: Some(error),
        }
    }

    pub fn web_ui_url(&self) -> Option<String> {
        if !self.web_ui_enabled || self.status != Status::Running {
            return None;
        }
        let (host, port) = self.listen.as_deref()?.rsplit_once(':')?;
        port.parse::<u16>().ok()?;
        let host = match host {
            "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
            h => h,
        };
        Some(if host.contains(':') && !host.starts_with('[') {
            format!("http://[{host}]:{port}")
        } else {
            format!("http://{host}:{port}")
        })
    }
}

impl Snapshot {
    /// Whether the daemon's config could be read (so it makes sense to edit it).
    pub fn is_known(&self) -> bool {
        self.status != Status::Unavailable
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSetResult {
    #[serde(default)]
    restart_required_paths: Vec<String>,
}

/// Saves the edits (a running daemon applies what it can live). Returns the config
/// paths that only take effect after a daemon restart. `undo[i]` reverses `edits[i]`;
/// if an edit fails, the ones already saved are rolled back so a save is all or nothing.
pub fn apply_edits(cli: &Cli, edits: &[Edit], undo: &[Edit]) -> anyhow::Result<Vec<String>> {
    apply_with_rollback(edits, undo, |edit| {
        let mut args = edit.args();
        args.push("--json".into());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        Ok(cli.json::<RawSetResult>(&args)?.restart_required_paths)
    })
}

fn apply_with_rollback(
    edits: &[Edit],
    undo: &[Edit],
    mut run: impl FnMut(&Edit) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<Vec<String>> {
    let mut restart_required = Vec::new();
    for (done, edit) in edits.iter().enumerate() {
        match run(edit) {
            Ok(paths) => {
                for path in paths {
                    if !restart_required.contains(&path) {
                        restart_required.push(path);
                    }
                }
            }
            Err(error) => {
                // Best effort, newest first. The failed edit itself was rejected whole by paseo.
                let failed_rollback = undo
                    .iter()
                    .take(done)
                    .rev()
                    .filter(|edit| run(edit).is_err())
                    .count();
                return Err(if failed_rollback == 0 {
                    error.context("nothing was changed")
                } else {
                    error.context("some changes could not be rolled back; check config.json")
                });
            }
        }
    }
    Ok(restart_required)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStatus {
    home: Option<PathBuf>,
    listen: Option<String>,
    configured_listen: Option<String>,
    local_daemon: String,
    log_path: Option<PathBuf>,
    relay: Option<RawRelay>,
}

#[derive(Deserialize)]
struct RawRelay {
    enabled: bool,
}

#[derive(Deserialize)]
struct RawConfigValue {
    value: Option<bool>,
}

#[derive(Deserialize)]
struct RawConfig {
    value: serde_json::Value,
}

/// The daemon's whole persisted config (`{}` if it has none yet).
pub fn fetch_config(cli: &Cli) -> anyhow::Result<serde_json::Value> {
    Ok(cli
        .json::<RawConfig>(&["daemon", "config", "get", "--json"])?
        .value)
}

pub fn fetch(cli: &Cli) -> Snapshot {
    match cli.json::<RawStatus>(&["daemon", "status", "--json"]) {
        Ok(raw) => from_raw(cli, raw),
        Err(e) => Snapshot::unavailable(format!("{e:#}")),
    }
}

fn parse_state(state: &str) -> Option<Status> {
    match state {
        "running" => Some(Status::Running),
        "not_ready" => Some(Status::Starting),
        "stopped" => Some(Status::Stopped),
        _ => None,
    }
}

fn from_raw(cli: &Cli, raw: RawStatus) -> Snapshot {
    // An unrecognised state must not read as "stopped": that would trigger an auto-start.
    let Some(status) = parse_state(&raw.local_daemon) else {
        return Snapshot::unavailable(format!(
            "unrecognised daemon state \"{}\" from paseo",
            raw.local_daemon
        ));
    };
    // A live daemon reports its effective relay state; otherwise ask the config.
    let relay_enabled = raw
        .relay
        .map_or_else(|| config_flag(cli, "daemon.relay.enabled"), |r| r.enabled);
    Snapshot {
        status,
        listen: raw.listen,
        configured_listen: raw.configured_listen,
        relay_enabled,
        web_ui_enabled: config_flag(cli, "features.webUi.enabled"),
        home: raw.home,
        log_path: raw.log_path,
        error: None,
    }
}

/// Unset or unreadable flags count as disabled, matching paseo's defaults.
fn config_flag(cli: &Cli, path: &str) -> bool {
    cli.json::<RawConfigValue>(&["daemon", "config", "get", path])
        .ok()
        .and_then(|v| v.value)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_on(listen: &str, web_ui: bool) -> Snapshot {
        Snapshot {
            status: Status::Running,
            listen: Some(listen.into()),
            web_ui_enabled: web_ui,
            ..Snapshot::unavailable(String::new())
        }
    }

    #[test]
    fn web_ui_url_rewrites_wildcard_hosts() {
        assert_eq!(
            running_on("127.0.0.1:6767", true).web_ui_url().as_deref(),
            Some("http://127.0.0.1:6767")
        );
        assert_eq!(
            running_on("0.0.0.0:80", true).web_ui_url().as_deref(),
            Some("http://127.0.0.1:80")
        );
        assert_eq!(
            running_on("::1:6767", true).web_ui_url().as_deref(),
            Some("http://[::1]:6767")
        );
    }

    #[test]
    fn a_failed_edit_rolls_back_the_earlier_ones() {
        let set = |path, value: &str| Edit::Set {
            path,
            value: value.into(),
            literal: true,
        };
        let edits = [set("a", "new"), set("b", "new"), set("c", "new")];
        let undo = [set("a", "old"), set("b", "old"), set("c", "old")];
        let mut log = Vec::new();
        let result = apply_with_rollback(&edits, &undo, |edit| {
            let Edit::Set { path, value, .. } = edit else {
                unreachable!()
            };
            log.push(format!("{path}={value}"));
            if *path == "b" && value == "new" {
                anyhow::bail!("rejected")
            }
            Ok(vec![])
        });
        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("rejected") && message.contains("nothing was changed"));
        assert_eq!(log, ["a=new", "b=new", "a=old"]);
    }

    #[test]
    fn restart_paths_are_collected_once() {
        let edit = Edit::Unset { path: "x" };
        let paths = apply_with_rollback(&[edit.clone(), edit], &[], |_| Ok(vec!["p".into()]));
        assert_eq!(paths.unwrap(), ["p"]);
    }

    #[test]
    fn only_known_states_are_understood() {
        assert_eq!(parse_state("stopped"), Some(Status::Stopped));
        assert_eq!(parse_state("not_ready"), Some(Status::Starting));
        assert_eq!(parse_state("zombie"), None);
    }

    #[test]
    fn web_ui_url_requires_feature_and_tcp_address() {
        assert_eq!(running_on("127.0.0.1:6767", false).web_ui_url(), None);
        assert_eq!(running_on("/run/paseo.sock", true).web_ui_url(), None);
    }
}

#[cfg(test)]
mod interop {
    use super::*;
    use crate::config::Settings;

    /// Needs a running daemon on a scratch home:
    /// `PASEO_TRAY_TEST_HOME=/tmp/scratch cargo test -- --ignored daemon::interop`
    #[test]
    #[ignore]
    fn relay_applies_live_but_address_and_webui_need_a_restart() {
        use crate::daemon_config::{edits, read};
        let home = std::env::var("PASEO_TRAY_TEST_HOME").expect("PASEO_TRAY_TEST_HOME");
        let cli = Cli::new(&Settings {
            home: Some(home.into()),
            ..Settings::default()
        });
        let apply = |cli: &Cli, edits: &[Edit]| apply_edits(cli, edits, &[]);
        let before = read(&fetch_config(&cli).unwrap());
        let change = |paths: &[(&str, &str)]| {
            let mut to = before.clone();
            for (path, text) in paths {
                let i = crate::daemon_config::FIELDS
                    .iter()
                    .position(|f| f.path == *path)
                    .unwrap();
                to[i] = text.to_string();
            }
            edits(&before, &to).unwrap()
        };
        let relay_on = if before[1] == "true" { "false" } else { "true" };
        assert_eq!(
            apply(&cli, &change(&[("daemon.relay.enabled", relay_on)])).unwrap(),
            Vec::<String>::new()
        );
        let restart = apply(
            &cli,
            &change(&[
                ("features.webUi.enabled", "true"),
                ("daemon.listen", "127.0.0.1:6797"),
            ]),
        )
        .unwrap();
        assert!(
            restart.contains(&"daemon.listen".to_string()),
            "{restart:?}"
        );
        // put it back
        let now = read(&fetch_config(&cli).unwrap());
        apply(&cli, &edits(&now, &before).unwrap()).unwrap();
        assert_eq!(read(&fetch_config(&cli).unwrap()), before);
    }
}
