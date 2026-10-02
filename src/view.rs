//! Tray view model: a pure function of state. Menu layout and wording live in
//! `ui/tray.slint`; this decides what is shown, enabled, and which icon to use.

use crate::{
    daemon::{Snapshot, Status},
    icon::Tone,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub status_line: String,
    pub address_line: String,
    pub relay_line: String,
    /// Empty when there is nothing to report.
    pub error_line: String,
    /// Daemon is running or starting, so Stop (not Start) is offered.
    pub daemon_up: bool,
    pub can_control: bool,
    /// Running and idle: commands that need a live daemon are enabled.
    pub live: bool,
    pub web_ui: bool,
    /// The daemon's config was read, so its address, relay and WebUI can be edited.
    pub daemon_known: bool,
    pub has_log: bool,
    pub quit_stops_daemon: bool,
    pub tone: Tone,
    pub tooltip: String,
}

pub fn view(state: &AppState) -> View {
    let status = state.status();
    let snapshot = state.snapshot.as_ref();
    let idle = state.busy.is_none();
    let observed = state.observed();
    let error = state
        .last_error
        .as_ref()
        .or(snapshot.and_then(|s| s.error.as_ref()));

    View {
        status_line: format!("{} Status: {}", Tone::of(status).dot(), status.label()),
        address_line: snapshot.map_or_else(|| "Address: unknown".into(), address_line),
        relay_line: format!(
            "Relay: {}",
            if snapshot.is_some_and(|s| s.relay_enabled) {
                "enabled"
            } else {
                "disabled"
            }
        ),
        error_line: error.map_or_else(String::new, |e| format!("Error: {}", summarize(e))),
        daemon_up: matches!(observed, Some(Status::Running | Status::Starting)),
        can_control: idle
            && matches!(
                observed,
                Some(Status::Running | Status::Starting | Status::Stopped)
            ),
        live: idle && observed == Some(Status::Running),
        web_ui: snapshot.is_some_and(|s| s.web_ui_enabled),
        daemon_known: snapshot.is_some_and(Snapshot::is_known),
        has_log: snapshot.is_some_and(|s| s.log_path.is_some()),
        quit_stops_daemon: state.settings.auto_stop_daemon,
        tone: Tone::of(status),
        tooltip: format!("Paseo daemon: {}", status.label()),
    }
}

fn address_line(s: &Snapshot) -> String {
    match (&s.listen, &s.configured_listen) {
        (Some(live), _) => format!("Address: {live}"),
        (None, Some(configured)) => format!("Address: {configured} (configured)"),
        (None, None) => "Address: unknown".into(),
    }
}

/// First line only, capped, so a verbose CLI error cannot blow up the menu.
fn summarize(error: &str) -> String {
    const MAX: usize = 80;
    let line = error.lines().next().unwrap_or_default();
    match line.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn with(status: Status, web_ui_enabled: bool) -> AppState {
        let mut state = AppState::new(Settings::default(), false);
        state.snapshot = Some(Snapshot {
            status,
            listen: Some("127.0.0.1:6767".into()),
            configured_listen: None,
            relay_enabled: true,
            web_ui_enabled,
            home: None,
            log_path: None,
            error: None,
        });
        state
    }

    #[test]
    fn running_view_shows_status_address_relay_and_webui() {
        let v = view(&with(Status::Running, true));
        assert_eq!(v.status_line, "🟢 Status: running");
        assert_eq!(v.address_line, "Address: 127.0.0.1:6767");
        assert_eq!(v.relay_line, "Relay: enabled");
        assert!(v.daemon_up && v.live && v.can_control && v.web_ui && v.daemon_known);
        assert_eq!(v.tone, Tone::Ok);
    }

    #[test]
    fn webui_hidden_when_feature_disabled() {
        assert!(!view(&with(Status::Running, false)).web_ui);
    }

    #[test]
    fn stopped_view_offers_start_and_disables_live_commands() {
        let v = view(&with(Status::Stopped, true));
        assert!(!v.daemon_up && !v.live && v.can_control);
        assert_eq!(v.tone, Tone::Idle);
    }

    #[test]
    fn busy_overrides_status_and_locks_controls() {
        let mut state = with(Status::Running, true);
        state.busy = Some(Status::Restarting);
        let v = view(&state);
        assert_eq!(v.status_line, "🟡 Status: restarting…");
        assert!(!v.can_control && !v.live);
    }

    #[test]
    fn summarize_truncates_on_char_boundaries() {
        assert_eq!(summarize(&"错".repeat(200)).chars().count(), 81);
    }
}
