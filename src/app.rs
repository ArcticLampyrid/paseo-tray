//! Wires the pure state machine to Slint: owns the tray, dialogs and poller,
//! and runs the effects `state::update` asks for. All of it lives on the UI
//! thread; other threads reach it only through `send`.

use crate::{
    actions::{self, Dialog},
    autostart,
    cli::Cli,
    config, daemon, dialogs,
    icon::{self, Tone},
    password,
    state::{update, AppState, Effect, Event},
    view::{self, View},
    DaemonDialog, PairDialog, PasswordDialog, SettingsDialog, Tray,
};
use anyhow::{Context, Result};
use std::{
    cell::RefCell,
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread,
    time::Duration,
};

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Delivers an event to the app from any thread. Fails once the event loop has ended.
pub fn send(event: Event) -> Result<(), slint::EventLoopError> {
    slint::invoke_from_event_loop(move || {
        APP.with_borrow_mut(|app| {
            if let Some(app) = app {
                app.handle(event);
            }
        })
    })
}

enum PollMsg {
    Now,
    Reconfigure(Cli, Duration, u64),
}

/// Dialog windows must stay alive while open; opening a dialog again replaces it.
#[derive(Default)]
struct Dialogs {
    password: Option<PasswordDialog>,
    pair: Option<PairDialog>,
    daemon: Option<DaemonDialog>,
    settings: Option<SettingsDialog>,
}

struct App {
    state: AppState,
    tray: Tray,
    drawn_tone: Option<Tone>,
    poller: Sender<PollMsg>,
    dialogs: Dialogs,
}

pub fn run() -> Result<()> {
    let settings = config::load();
    let state = AppState::new(settings, autostart::is_enabled());

    let tray = Tray::new()?;
    tray.on_choose(|id| {
        if let Some(action) = actions::Action::from_id(&id) {
            let _ = send(Event::Chose(action));
        }
    });
    tray.on_tray_clicked(|| {
        let _ = send(Event::TrayClicked);
    });
    let poller = spawn_poller(
        Cli::new(&state.settings),
        state.settings.poll_interval(),
        state.generation,
    );

    let mut app = App {
        state,
        tray,
        drawn_tone: None,
        poller,
        dialogs: Dialogs::default(),
    };
    app.render();
    app.tray.show()?;
    APP.set(Some(app));

    // SIGINT/SIGTERM follow the same shutdown path as the Quit item.
    if let Err(e) = ctrlc::set_handler(|| {
        let _ = send(Event::Terminate);
    }) {
        eprintln!("paseo-tray: cannot install signal handler: {e}");
    }

    slint::run_event_loop_until_quit()?;
    APP.set(None); // removes the tray icon
    Ok(())
}

impl App {
    fn handle(&mut self, event: Event) {
        let (next, effects) = update(&self.state, event);
        self.state = next;
        for effect in effects {
            self.run(effect);
        }
        self.render();
    }

    fn run(&mut self, effect: Effect) {
        match effect {
            Effect::Spawn(action) => {
                let cli = Cli::new(&self.state.settings);
                let snapshot = self.state.snapshot.clone();
                thread::spawn(move || {
                    let error = actions::execute(action, &cli, snapshot.as_ref())
                        .err()
                        .map(|e| format!("{e:#}"));
                    let _ = send(Event::Finished { action, error });
                });
            }
            Effect::Open(dialog) => {
                if let Err(e) = self.open(dialog) {
                    fail(format!("cannot open {dialog:?} dialog: {e:#}"));
                }
            }
            Effect::Poll => {
                let _ = self.poller.send(PollMsg::Now);
            }
            Effect::Reconfigure(settings, generation) => {
                let _ = self.poller.send(PollMsg::Reconfigure(
                    Cli::new(&settings),
                    settings.poll_interval(),
                    generation,
                ));
            }
            Effect::Persist(settings) => {
                if let Err(e) = config::save(&settings) {
                    fail(format!("cannot save settings: {e:#}"));
                }
            }
            Effect::SetLaunchAtLogin(enabled) => {
                if let Err(e) = autostart::set_enabled(enabled) {
                    fail(format!("cannot change launch at login: {e:#}"));
                }
                let _ = send(Event::LaunchAtLoginChanged(autostart::is_enabled()));
            }
            Effect::Exit => {
                let _ = self.tray.hide();
                let _ = slint::quit_event_loop();
            }
        }
    }

    fn open(&mut self, dialog: Dialog) -> Result<()> {
        let cli = Cli::new(&self.state.settings);
        match dialog {
            Dialog::Password => {
                let running = self.state.observed() == Some(daemon::Status::Running);
                let reported = self.state.snapshot.as_ref().and_then(|s| s.home.clone());
                let home = password::resolve_home(reported, self.state.settings.home.clone())
                    .context("cannot determine the daemon home")?;
                self.dialogs.password = Some(dialogs::open_password(home, running)?);
            }
            Dialog::Daemon { enable_web_ui } => {
                let snapshot = self.state.snapshot.as_ref().filter(|s| s.is_known());
                let snapshot = snapshot.context("the daemon's configuration is not available")?;
                let running = snapshot.status == daemon::Status::Running;
                self.dialogs.daemon = Some(dialogs::open_daemon(cli, enable_web_ui, running)?);
            }
            Dialog::Pair => self.dialogs.pair = Some(dialogs::open_pair(cli)?),
            Dialog::Settings => {
                let settings =
                    dialogs::open_settings(&self.state.settings, self.state.launch_at_login)?;
                self.dialogs.settings = Some(settings);
            }
        }
        Ok(())
    }

    fn render(&mut self) {
        let View {
            status_line,
            address_line,
            relay_line,
            error_line,
            daemon_up,
            can_control,
            live,
            web_ui,
            daemon_known,
            has_log,
            quit_stops_daemon,
            tone,
            tooltip,
        } = view::view(&self.state);
        let tray = &self.tray;
        tray.set_status_line(status_line.into());
        tray.set_address_line(address_line.into());
        tray.set_relay_line(relay_line.into());
        tray.set_error_line(error_line.into());
        tray.set_daemon_up(daemon_up);
        tray.set_can_control(can_control);
        tray.set_live(live);
        tray.set_web_ui(web_ui);
        tray.set_daemon_known(daemon_known);
        tray.set_has_log(has_log);
        tray.set_quit_stops_daemon(quit_stops_daemon);
        tray.set_status_tooltip(tooltip.into());
        if self.drawn_tone != Some(tone) {
            tray.set_status_icon(icon::render(tone));
            self.drawn_tone = Some(tone);
        }
    }
}

/// Reports a background failure in the menu (and on stderr).
fn fail(message: String) {
    eprintln!("paseo-tray: {message}");
    let _ = send(Event::Failed(message));
}

/// Polls on an interval; `PollMsg::Now` polls immediately, `Reconfigure` swaps CLI,
/// interval and the generation that tags each poll.
fn spawn_poller(mut cli: Cli, mut interval: Duration, mut generation: u64) -> Sender<PollMsg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || loop {
        let snapshot = daemon::fetch(&cli);
        if send(Event::Polled {
            snapshot,
            generation,
        })
        .is_err()
        {
            break;
        }
        match rx.recv_timeout(interval) {
            Err(RecvTimeoutError::Disconnected) => break,
            Ok(PollMsg::Reconfigure(new_cli, new_interval, new_generation)) => {
                (cli, interval, generation) = (new_cli, new_interval, new_generation)
            }
            Ok(PollMsg::Now) | Err(RecvTimeoutError::Timeout) => {}
        }
        // Coalesce queued requests so a burst causes one extra poll, not many.
        while let Ok(msg) = rx.try_recv() {
            if let PollMsg::Reconfigure(new_cli, new_interval, new_generation) = msg {
                (cli, interval, generation) = (new_cli, new_interval, new_generation);
            }
        }
    });
    tx
}
