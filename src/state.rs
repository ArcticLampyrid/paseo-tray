//! Application state and its pure transition function.
//! `update` performs no I/O: it returns `Effect`s for the event loop to run.

use crate::{
    actions::{Action, Dialog},
    config::Settings,
    daemon::{Snapshot, Status},
};

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub settings: Settings,
    /// `None` until the first poll completes.
    pub snapshot: Option<Snapshot>,
    /// Transitional status while a daemon command is running.
    pub busy: Option<Status>,
    pub launch_at_login: bool,
    pub last_error: Option<String>,
    /// Bumped whenever the CLI path or daemon home changes; polls from an older
    /// generation describe a different daemon and are dropped.
    pub generation: u64,
    bootstrapped: bool,
    quitting: bool,
}

impl AppState {
    pub fn new(settings: Settings, launch_at_login: bool) -> Self {
        Self {
            settings,
            snapshot: None,
            busy: None,
            launch_at_login,
            last_error: None,
            generation: 0,
            bootstrapped: false,
            quitting: false,
        }
    }

    /// Status for display: a running command overrides what was last observed.
    pub fn status(&self) -> Status {
        self.busy
            .or_else(|| self.snapshot.as_ref().map(|s| s.status))
            .unwrap_or(Status::Checking)
    }

    pub fn observed(&self) -> Option<Status> {
        self.snapshot.as_ref().map(|s| s.status)
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Polled {
        snapshot: Snapshot,
        generation: u64,
    },
    Chose(Action),
    Finished {
        action: Action,
        error: Option<String>,
    },
    /// The settings dialog was saved.
    SettingsSaved {
        settings: Settings,
        launch_at_login: bool,
    },
    LaunchAtLoginChanged(bool),
    /// The tray icon itself was clicked (not its menu).
    TrayClicked,
    /// Something changed outside the tray's own commands; look at the daemon again.
    Refresh,
    /// A background step (opening a dialog, saving settings) failed.
    Failed(String),
    Terminate,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Run the action's side effect on a worker thread.
    Spawn(Action),
    Open(Dialog),
    /// Poll the daemon now rather than at the next interval.
    Poll,
    /// Make the poller use the CLI path, home and interval from these settings,
    /// tagging its polls with the generation.
    Reconfigure(Settings, u64),
    Persist(Settings),
    SetLaunchAtLogin(bool),
    Exit,
}

pub fn update(state: &AppState, event: Event) -> (AppState, Vec<Effect>) {
    let mut next = state.clone();
    let mut effects = Vec::new();
    match event {
        Event::Polled {
            snapshot,
            generation,
        } if generation == next.generation => {
            next.snapshot = Some(snapshot);
            if !next.bootstrapped {
                next.bootstrapped = true;
                if next.settings.auto_start_daemon && next.observed() == Some(Status::Stopped) {
                    dispatch(&mut next, &mut effects, Action::Start);
                }
            }
        }
        Event::Polled { .. } => {}
        Event::Chose(action) => choose(&mut next, &mut effects, action),
        Event::Finished { action, error } => {
            if action.busy_status().is_some() {
                next.busy = None;
            }
            next.last_error = error;
            if next.quitting && action.busy_status().is_some() {
                // The command that delayed shutdown is done; stop the daemon unless that
                // was the stop itself (a failed stop must not keep the tray alive).
                stop_and_exit(&mut next, &mut effects, action);
            } else {
                effects.push(Effect::Poll);
            }
        }
        Event::SettingsSaved {
            settings,
            launch_at_login,
        } => {
            if (&settings.cli_path, &settings.home)
                != (&next.settings.cli_path, &next.settings.home)
            {
                // Everything observed so far belongs to the previous target.
                next.generation += 1;
                next.snapshot = None;
            }
            next.settings = settings.clone();
            effects.extend([
                Effect::Persist(settings.clone()),
                Effect::Reconfigure(settings, next.generation),
            ]);
            if launch_at_login != next.launch_at_login {
                effects.push(Effect::SetLaunchAtLogin(launch_at_login));
            }
        }
        Event::LaunchAtLoginChanged(on) => next.launch_at_login = on,
        Event::TrayClicked => {
            if let Some(action) = primary_action(&next) {
                choose(&mut next, &mut effects, action);
            }
        }
        Event::Refresh => effects.push(Effect::Poll),
        Event::Failed(message) => next.last_error = Some(message),
        Event::Terminate => begin_quit(&mut next, &mut effects),
    }
    (next, effects)
}

/// What a left-click on the tray icon does: the most useful next step for the daemon's state.
pub fn primary_action(state: &AppState) -> Option<Action> {
    let snapshot = state.snapshot.as_ref().filter(|_| state.busy.is_none())?;
    match snapshot.status {
        Status::Running if snapshot.web_ui_enabled => Some(Action::OpenWebUi),
        Status::Stopped => Some(Action::Start),
        Status::Unavailable => Some(Action::OpenSettings),
        _ => None,
    }
}

fn choose(next: &mut AppState, effects: &mut Vec<Effect>, action: Action) {
    if let Some(dialog) = action.dialog() {
        effects.push(Effect::Open(dialog));
        return;
    }
    match action {
        Action::Quit => begin_quit(next, effects),
        _ if next.busy.is_some() && action.busy_status().is_some() => {}
        _ => dispatch(next, effects, action),
    }
}

fn dispatch(next: &mut AppState, effects: &mut Vec<Effect>, action: Action) {
    next.busy = action.busy_status().or(next.busy);
    next.last_error = None;
    effects.push(Effect::Spawn(action));
}

fn begin_quit(next: &mut AppState, effects: &mut Vec<Effect>) {
    if next.quitting {
        return;
    }
    next.quitting = true;
    // A running command must finish first: stopping concurrently would race with it.
    // `Finished` continues the shutdown.
    if next.busy.is_some() {
        return;
    }
    let may_be_up = matches!(next.observed(), Some(Status::Running | Status::Starting));
    if may_be_up {
        stop_and_exit(next, effects, Action::Quit);
    } else {
        effects.push(Effect::Exit);
    }
}

/// Shutdown step after `after` (the command that just ended, or `Quit` if none was
/// running): stop the daemon if configured and not just stopped, else exit.
fn stop_and_exit(next: &mut AppState, effects: &mut Vec<Effect>, after: Action) {
    if next.settings.auto_stop_daemon && after != Action::Stop {
        next.busy = Some(Status::Stopping);
        effects.push(Effect::Spawn(Action::Stop));
    } else {
        effects.push(Effect::Exit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(status: Status) -> Snapshot {
        Snapshot {
            status,
            listen: None,
            configured_listen: None,
            relay_enabled: false,
            web_ui_enabled: false,
            home: None,
            log_path: None,
            error: None,
        }
    }

    fn polled(status: Status) -> Event {
        Event::Polled {
            snapshot: snapshot(status),
            generation: 0,
        }
    }

    fn state(settings: Settings) -> AppState {
        AppState::new(settings, false)
    }

    #[test]
    fn first_poll_autostarts_a_stopped_daemon_once() {
        let (s, fx) = update(&state(Settings::default()), polled(Status::Stopped));
        assert_eq!(fx, [Effect::Spawn(Action::Start)]);
        assert_eq!(s.status(), Status::Starting);

        let (s, _) = update(
            &s,
            Event::Finished {
                action: Action::Start,
                error: None,
            },
        );
        let (_, fx) = update(&s, polled(Status::Stopped));
        assert!(fx.is_empty(), "only the first poll auto-starts");
    }

    #[test]
    fn first_poll_leaves_running_daemon_alone_or_respects_setting() {
        let (_, fx) = update(&state(Settings::default()), polled(Status::Running));
        assert!(fx.is_empty());
        let off = Settings {
            auto_start_daemon: false,
            ..Settings::default()
        };
        let (_, fx) = update(&state(off), polled(Status::Stopped));
        assert!(fx.is_empty());
    }

    #[test]
    fn quit_stops_daemon_then_exits_when_configured() {
        let (s, _) = update(&state(Settings::default()), polled(Status::Running));
        let (s, fx) = update(&s, Event::Chose(Action::Quit));
        assert_eq!(fx, [Effect::Spawn(Action::Stop)]);
        let (_, fx) = update(
            &s,
            Event::Finished {
                action: Action::Stop,
                error: Some("boom".into()),
            },
        );
        assert_eq!(fx, [Effect::Exit], "exits even if the stop failed");
    }

    #[test]
    fn quit_exits_immediately_without_auto_stop() {
        let settings = Settings {
            auto_stop_daemon: false,
            ..Settings::default()
        };
        let (s, _) = update(&state(settings), polled(Status::Running));
        let (_, fx) = update(&s, Event::Terminate);
        assert_eq!(fx, [Effect::Exit]);
    }

    #[test]
    fn saving_settings_applies_persists_and_reconfigures() {
        let new = Settings {
            auto_stop_daemon: false,
            poll_interval_secs: 9,
            ..Settings::default()
        };
        let event = Event::SettingsSaved {
            settings: new.clone(),
            launch_at_login: true,
        };
        let (s, fx) = update(&state(Settings::default()), event);
        assert_eq!(s.settings, new);
        assert_eq!(
            fx,
            [
                Effect::Persist(new.clone()),
                Effect::Reconfigure(new, 0),
                Effect::SetLaunchAtLogin(true)
            ]
        );
    }

    #[test]
    fn dialog_actions_open_dialogs_without_going_busy() {
        let (s, fx) = update(
            &state(Settings::default()),
            Event::Chose(Action::SetPassword),
        );
        assert_eq!(fx, [Effect::Open(Dialog::Password)]);
        assert_eq!(s.busy, None);
    }

    fn with_status(status: Status, web_ui_enabled: bool) -> AppState {
        let mut state = state(Settings::default());
        state.snapshot = Some(Snapshot {
            web_ui_enabled,
            ..snapshot(status)
        });
        state
    }

    #[test]
    fn clicking_the_icon_does_the_most_useful_thing() {
        let click = |s: &AppState| update(s, Event::TrayClicked).1;
        assert_eq!(
            click(&with_status(Status::Running, true)),
            [Effect::Spawn(Action::OpenWebUi)]
        );
        assert_eq!(
            click(&with_status(Status::Stopped, true)),
            [Effect::Spawn(Action::Start)]
        );
        assert_eq!(
            click(&with_status(Status::Unavailable, false)),
            [Effect::Open(Dialog::Settings)]
        );
        assert!(click(&with_status(Status::Running, false)).is_empty());
        assert!(
            click(&state(Settings::default())).is_empty(),
            "nothing before the first poll"
        );
        let (busy, _) = update(
            &with_status(Status::Stopped, true),
            Event::Chose(Action::Start),
        );
        assert!(click(&busy).is_empty(), "nothing while a command runs");
    }

    #[test]
    fn enabling_webui_opens_the_daemon_dialog_ticked() {
        let (_, fx) = update(
            &state(Settings::default()),
            Event::Chose(Action::EnableWebUi),
        );
        assert_eq!(
            fx,
            [Effect::Open(Dialog::Daemon {
                enable_web_ui: true
            })]
        );
    }

    #[test]
    fn ignores_daemon_commands_while_busy() {
        let (s, _) = update(&state(Settings::default()), Event::Chose(Action::Restart));
        let (_, fx) = update(&s, Event::Chose(Action::Stop));
        assert!(fx.is_empty());
    }

    #[test]
    fn quit_while_busy_waits_for_the_command_before_stopping() {
        let (s, _) = update(&state(Settings::default()), polled(Status::Running));
        let (s, _) = update(&s, Event::Chose(Action::Restart));
        let (s, fx) = update(&s, Event::Chose(Action::Quit));
        assert!(fx.is_empty(), "no concurrent stop");
        let (s, fx) = update(
            &s,
            Event::Finished {
                action: Action::Restart,
                error: None,
            },
        );
        assert_eq!(fx, [Effect::Spawn(Action::Stop)]);
        let (_, fx) = update(
            &s,
            Event::Finished {
                action: Action::Stop,
                error: None,
            },
        );
        assert_eq!(fx, [Effect::Exit]);
    }

    #[test]
    fn quit_while_stopping_exits_without_a_second_stop() {
        let (s, _) = update(&state(Settings::default()), polled(Status::Running));
        let (s, _) = update(&s, Event::Chose(Action::Stop));
        let (s, _) = update(&s, Event::Chose(Action::Quit));
        let (_, fx) = update(
            &s,
            Event::Finished {
                action: Action::Stop,
                error: None,
            },
        );
        assert_eq!(fx, [Effect::Exit]);
    }

    #[test]
    fn changing_the_target_drops_stale_observations() {
        let (s, _) = update(&state(Settings::default()), polled(Status::Running));
        let same = Settings {
            poll_interval_secs: 9,
            ..Settings::default()
        };
        let (kept, _) = update(
            &s,
            Event::SettingsSaved {
                settings: same,
                launch_at_login: false,
            },
        );
        assert!(kept.snapshot.is_some());

        let moved = Settings {
            home: Some("/elsewhere".into()),
            ..Settings::default()
        };
        let (s, fx) = update(
            &s,
            Event::SettingsSaved {
                settings: moved.clone(),
                launch_at_login: false,
            },
        );
        assert_eq!(s.snapshot, None);
        assert!(fx.contains(&Effect::Reconfigure(moved, 1)));
        let (s, _) = update(&s, polled(Status::Running)); // generation 0: stale
        assert_eq!(s.snapshot, None);
        let fresh = Event::Polled {
            snapshot: snapshot(Status::Stopped),
            generation: 1,
        };
        assert!(update(&s, fresh).0.snapshot.is_some());
    }

    #[test]
    fn failures_surface_in_the_menu() {
        let (s, _) = update(&state(Settings::default()), Event::Failed("nope".into()));
        assert_eq!(s.last_error.as_deref(), Some("nope"));
    }
}
