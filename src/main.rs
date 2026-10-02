mod actions;
mod app;
mod autostart;
mod cli;
mod config;
mod daemon;
mod daemon_config;
mod dialogs;
mod fsutil;
mod icon;
mod pairing;
mod password;
mod state;
mod view;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    app::run()
}
