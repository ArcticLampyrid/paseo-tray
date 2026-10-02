//! Slint dialogs. Each `open_*` builds a fresh window, wires its callbacks, and
//! returns the handle (the caller keeps it alive). Blocking work runs on worker
//! threads and reports back through the event loop.

use crate::{
    actions::Action,
    app::send,
    cli::Cli,
    config::Settings,
    daemon,
    daemon_config::{self, Field, Form, Group, Kind, FIELDS},
    pairing::{self, Offer},
    password,
    state::Event,
    DaemonDialog, FieldRow, GroupRow, PairDialog, PasswordDialog, SettingsDialog,
};
use anyhow::Result;
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel, Weak};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
};

// ---- password ---------------------------------------------------------------

pub fn open_password(home: PathBuf, daemon_running: bool) -> Result<PasswordDialog> {
    let dialog = PasswordDialog::new()?;
    dialog.set_daemon_running(daemon_running);
    dialog.on_dismiss(close_on(&dialog));
    dialog.on_restart_daemon(|| {
        let _ = send(Event::Chose(Action::Restart));
    });
    let weak = dialog.as_weak();
    dialog.on_submit(move |password, confirm| {
        let Some(dialog) = weak.upgrade() else { return };
        if let Err(problem) = validate_password(&password, &confirm) {
            dialog.set_message(problem.into());
            return;
        }
        dialog.set_busy(true);
        dialog.set_message(SharedString::new());
        let (home, weak, password) = (home.clone(), weak.clone(), password.to_string());
        thread::spawn(move || {
            let result = password::set_password(&home, &password);
            let _ = weak.upgrade_in_event_loop(move |dialog| finish_password(&dialog, result));
        });
    });
    dialog.show()?;
    Ok(dialog)
}

fn finish_password(dialog: &PasswordDialog, result: Result<()>) {
    dialog.set_busy(false);
    dialog.set_password(SharedString::new());
    dialog.set_confirm(SharedString::new());
    match result {
        Ok(()) => {
            dialog.set_success(true);
            dialog.set_message(
                if dialog.get_daemon_running() {
                    "Password saved. The running daemon needs a restart to use it."
                } else {
                    "Password saved."
                }
                .into(),
            );
        }
        Err(e) => dialog.set_message(format!("{e:#}").into()),
    }
}

fn validate_password(password: &str, confirm: &str) -> Result<(), &'static str> {
    if password.is_empty() {
        Err("Password cannot be empty")
    } else if password.len() > password::MAX_PASSWORD_BYTES {
        Err("Password is too long (at most 72 bytes)")
    } else if password != confirm {
        Err("Passwords do not match")
    } else {
        Ok(())
    }
}

// ---- pairing ----------------------------------------------------------------

pub fn open_pair(cli: Cli) -> Result<PairDialog> {
    let dialog = PairDialog::new()?;
    dialog.on_dismiss(close_on(&dialog));
    let weak = dialog.as_weak();
    let latest = Arc::new(AtomicU64::new(0));
    dialog.on_enable_relay({
        let (cli, weak, latest) = (cli.clone(), weak.clone(), latest.clone());
        move || load_offer(&cli, &weak, &latest, true)
    });
    dialog.show()?;
    load_offer(&cli, &weak, &latest, false);
    Ok(dialog)
}

/// Fetches an offer on a worker thread. Each request takes a ticket from `latest`, and a
/// response is shown only if its ticket is still the newest, so a slow earlier request
/// cannot overwrite a later one.
fn load_offer(cli: &Cli, weak: &Weak<PairDialog>, latest: &Arc<AtomicU64>, enable_relay: bool) {
    let ticket = latest.fetch_add(1, Ordering::SeqCst) + 1;
    let latest = latest.clone();
    if let Some(dialog) = weak.upgrade() {
        dialog.set_loading(true);
        dialog.set_message(SharedString::new());
    }
    let (cli, weak) = (cli.clone(), weak.clone());
    thread::spawn(move || {
        // The QR pixel buffer is `Send`; the `Image` wrapping it is built on the UI thread.
        let result = pairing::fetch(&cli, enable_relay).and_then(|offer| match offer {
            Offer::Ready { url } => {
                pairing::qr_pixels(&url).map(|qr| (Offer::Ready { url }, Some(qr)))
            }
            Offer::RelayDisabled => Ok((Offer::RelayDisabled, None)),
        });
        let _ = weak.upgrade_in_event_loop(move |dialog| {
            if latest.load(Ordering::SeqCst) == ticket {
                show_offer(&dialog, result);
            }
        });
    });
}

fn show_offer(
    dialog: &PairDialog,
    result: Result<(Offer, Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>)>,
) {
    dialog.set_loading(false);
    dialog.set_needs_relay(false);
    dialog.set_message_is_error(false);
    dialog.set_url(SharedString::new());
    match result {
        Ok((Offer::Ready { url }, qr)) => {
            dialog.set_url(url.into());
            dialog.set_qr(qr.map(Image::from_rgba8).unwrap_or_default());
            dialog.set_message(SharedString::new());
        }
        Ok((Offer::RelayDisabled, _)) => {
            dialog.set_needs_relay(true);
            dialog.set_message(
                format!(
                    "Pairing goes through the Paseo relay, which is disabled for this daemon. \
                     Enabling it changes the daemon's config. More: {}",
                    pairing::RELAY_DOCS_URL
                )
                .into(),
            );
        }
        Err(e) => {
            dialog.set_message_is_error(true);
            dialog.set_message(format!("{e:#}").into());
        }
    }
}

// ---- daemon settings --------------------------------------------------------

pub fn open_daemon(cli: Cli, enable_web_ui: bool, daemon_running: bool) -> Result<DaemonDialog> {
    let dialog = DaemonDialog::new()?;
    dialog.on_dismiss(close_on(&dialog));
    dialog.on_open_config(|| {
        let _ = send(Event::Chose(Action::OpenDaemonConfig));
    });
    dialog.on_restart_daemon(|| {
        let _ = send(Event::Chose(Action::Restart));
    });
    dialog.show()?;
    let weak = dialog.as_weak();
    thread::spawn(move || {
        let result = daemon::fetch_config(&cli).map(|config| daemon_config::read(&config));
        let _ = weak.upgrade_in_event_loop(move |dialog| {
            load_daemon(&dialog, result, cli, enable_web_ui, daemon_running)
        });
    });
    Ok(dialog)
}

/// Fills the form once the config has been read and wires the callbacks that need it.
fn load_daemon(
    dialog: &DaemonDialog,
    result: Result<Form>,
    cli: Cli,
    enable_web_ui: bool,
    daemon_running: bool,
) {
    let initial = match result {
        Ok(form) => form,
        Err(e) => {
            dialog.set_loading(false);
            dialog.set_failed(true);
            dialog.set_message(format!("{e:#}").into());
            return;
        }
    };
    let wanted = if enable_web_ui {
        daemon_config::with_flag_on(initial.clone(), "features.webUi.enabled")
    } else {
        initial.clone()
    };
    // `groups[g][f]` is the index into `FIELDS`/`Form`.
    let layout: Vec<Vec<usize>> = Group::ALL
        .iter()
        .map(|g| {
            (0..FIELDS.len())
                .filter(|&i| FIELDS[i].group == *g)
                .collect()
        })
        .collect();
    let groups: Vec<GroupRow> = Group::ALL
        .iter()
        .zip(&layout)
        .map(|(group, indices)| GroupRow {
            title: group.title().into(),
            fields: ModelRc::new(VecModel::from_iter(
                indices.iter().map(|&i| field_row(&FIELDS[i], &wanted[i])),
            )),
        })
        .collect();
    dialog.set_groups(ModelRc::new(VecModel::from(groups)));
    dialog.set_loading(false);

    let state = Rc::new(RefCell::new(wanted));
    dialog.on_edited({
        let state = state.clone();
        move |g, f, text, checked| {
            let index = layout[g as usize][f as usize];
            let value = if matches!(FIELDS[index].kind, Kind::Flag { .. }) {
                checked.to_string()
            } else {
                text.to_string()
            };
            state.borrow_mut()[index] = value;
        }
    });
    let weak = dialog.as_weak();
    dialog.on_save(move || {
        let Some(dialog) = weak.upgrade() else { return };
        let wanted = state.borrow().clone();
        let edits = match daemon_config::edits(&initial, &wanted) {
            Ok(edits) => edits,
            Err(problem) => return dialog.set_message(problem.into()),
        };
        let undo = daemon_config::edits(&wanted, &initial).unwrap_or_default();
        if edits.is_empty() {
            let _ = dialog.hide();
            return;
        }
        dialog.set_busy(true);
        dialog.set_message(SharedString::new());
        let (cli, weak) = (cli.clone(), weak.clone());
        thread::spawn(move || {
            let result = daemon::apply_edits(&cli, &edits, &undo);
            let _ = weak.upgrade_in_event_loop(move |dialog| {
                finish_daemon(&dialog, result, daemon_running)
            });
        });
    });
}

fn field_row(field: &Field, text: &str) -> FieldRow {
    let (kind, options): (i32, Vec<SharedString>) = match field.kind {
        Kind::Flag { .. } => (1, vec![]),
        Kind::Choice { options, .. } => (2, options.iter().map(|o| (*o).into()).collect()),
        Kind::Text | Kind::List | Kind::Count => (0, vec![]),
    };
    FieldRow {
        label: field.label.into(),
        hint: field.hint.into(),
        kind,
        text: text.into(),
        checked: text == "true",
        options: ModelRc::new(VecModel::from(options)),
        restart: field.restart,
    }
}

fn finish_daemon(dialog: &DaemonDialog, result: Result<Vec<String>>, daemon_running: bool) {
    dialog.set_busy(false);
    match result {
        Ok(restart_required) => {
            dialog.set_success(true);
            let restart_needed = daemon_running && !restart_required.is_empty();
            dialog.set_restart_needed(restart_needed);
            dialog.set_message(
                if restart_needed {
                    format!(
                        "Saved. A daemon restart is needed for: {}.",
                        restart_required.join(", ")
                    )
                } else if daemon_running {
                    "Saved and applied.".to_string()
                } else {
                    "Saved. It takes effect when the daemon starts.".to_string()
                }
                .into(),
            );
            let _ = send(Event::Refresh);
        }
        Err(e) => dialog.set_message(format!("{e:#}").into()),
    }
}

// ---- settings ---------------------------------------------------------------

pub fn open_settings(current: &Settings, launch_at_login: bool) -> Result<SettingsDialog> {
    let dialog = SettingsDialog::new()?;
    dialog.set_cli_path(path_text(&current.cli_path));
    dialog.set_home(path_text(&current.home));
    dialog.set_poll_interval(current.poll_interval_secs.to_string().into());
    dialog.set_auto_start(current.auto_start_daemon);
    dialog.set_auto_stop(current.auto_stop_daemon);
    dialog.set_launch_at_login(launch_at_login);
    dialog.on_dismiss(close_on(&dialog));
    let weak = dialog.as_weak();
    dialog.on_save(move || {
        let Some(dialog) = weak.upgrade() else { return };
        let form = SettingsForm {
            cli_path: dialog.get_cli_path().to_string(),
            home: dialog.get_home().to_string(),
            poll_interval: dialog.get_poll_interval().to_string(),
            auto_start: dialog.get_auto_start(),
            auto_stop: dialog.get_auto_stop(),
        };
        match apply_form(form) {
            Ok(settings) => {
                let _ = send(Event::SettingsSaved {
                    settings,
                    launch_at_login: dialog.get_launch_at_login(),
                });
                let _ = dialog.hide();
            }
            Err(problem) => dialog.set_message(problem.into()),
        }
    });
    dialog.show()?;
    Ok(dialog)
}

struct SettingsForm {
    cli_path: String,
    home: String,
    poll_interval: String,
    auto_start: bool,
    auto_stop: bool,
}

fn apply_form(form: SettingsForm) -> Result<Settings, &'static str> {
    let poll_interval_secs = form
        .poll_interval
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|&n| n >= 1)
        .ok_or("Poll interval must be a whole number of seconds, at least 1")?;
    Ok(Settings {
        cli_path: non_empty_path(&form.cli_path),
        home: non_empty_path(&form.home),
        auto_start_daemon: form.auto_start,
        auto_stop_daemon: form.auto_stop,
        poll_interval_secs,
    })
}

fn non_empty_path(text: &str) -> Option<PathBuf> {
    Some(text.trim())
        .filter(|t| !t.is_empty())
        .map(PathBuf::from)
}

fn path_text(path: &Option<PathBuf>) -> SharedString {
    path.as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
        .into()
}

// ---- shared -----------------------------------------------------------------

fn close_on<C: ComponentHandle + 'static>(dialog: &C) -> impl Fn() + 'static {
    let weak = dialog.as_weak();
    move || {
        if let Some(dialog) = weak.upgrade() {
            let _ = dialog.hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(poll: &str, cli: &str) -> SettingsForm {
        SettingsForm {
            cli_path: cli.into(),
            home: " ".into(),
            poll_interval: poll.into(),
            auto_start: false,
            auto_stop: true,
        }
    }

    #[test]
    fn password_validation() {
        assert_eq!(validate_password("", ""), Err("Password cannot be empty"));
        assert_eq!(validate_password("a", "b"), Err("Passwords do not match"));
        assert_eq!(validate_password("a", "a"), Ok(()));
        let long = "x".repeat(73);
        assert!(validate_password(&long, &long).is_err());
    }

    #[test]
    fn form_maps_blank_paths_to_none_and_validates_interval() {
        let s = apply_form(form(" 7 ", "/opt/paseo")).unwrap();
        assert_eq!(s.cli_path, Some(PathBuf::from("/opt/paseo")));
        assert_eq!(s.home, None);
        assert_eq!(
            (
                s.poll_interval_secs,
                s.auto_start_daemon,
                s.auto_stop_daemon
            ),
            (7, false, true)
        );
        assert!(apply_form(form("0", "")).is_err());
        assert!(apply_form(form("abc", "")).is_err());
    }
}
