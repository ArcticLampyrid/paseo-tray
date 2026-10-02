# Development

```sh
cargo build
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

CI (`.github/workflows/ci.yml`) runs the last three on every push. It installs the Slint
backend's system libraries on Ubuntu; that package list has not been exercised yet.

Requirements are the same as for installing: a Rust toolchain and a system tray. The UI is
compiled from `ui/*.slint` by `build.rs`.

## Test against a scratch daemon, never your real one

The tray starts and stops daemons. Point it at a throwaway home on a different port:

```sh
paseo daemon config set --home /tmp/paseo-scratch daemon.listen 127.0.0.1:6799

mkdir -p /tmp/cfg/paseo-tray
printf 'home = "/tmp/paseo-scratch"\npoll_interval_secs = 2\n' > /tmp/cfg/paseo-tray/config.toml
XDG_CONFIG_HOME=/tmp/cfg cargo run
```

With the default settings it should start the daemon, and SIGTERM should stop it again and
exit.

## Password interoperability test

An ignored test writes a hash into a scratch home so a real daemon can be tried against it:

```sh
PASEO_TRAY_TEST_HOME=/tmp/paseo-scratch PASEO_TRAY_TEST_PASSWORD=pw \
  cargo test -- --ignored
paseo daemon start --home /tmp/paseo-scratch
PASEO_PASSWORD=pw paseo --host 127.0.0.1:6799 ls      # accepted
PASEO_PASSWORD=wrong paseo --host 127.0.0.1:6799 ls   # "Incorrect password"
```

## Daemon config interoperability test

With a daemon running on a scratch home, an ignored test checks that the daemon settings
edits are reported correctly (relay applies live; address and WebUI need a restart) and
restores the config afterwards:

```sh
PASEO_TRAY_TEST_HOME=/tmp/paseo-scratch cargo test -- --ignored daemon::interop
```

## Inspecting the tray without looking at it

On Linux the Slint tray is a D-Bus StatusNotifierItem. Its menu, tooltip and icon can be
read with `gdbus` against `org.kde.StatusNotifierItem-<pid>-1` (menu at `/MenuBar`, item
at `/StatusNotifierItem`). Menu items can be clicked by sending
`com.canonical.dbusmenu.Event`, and a left-click is
`org.kde.StatusNotifierItem.Activate`.

## Known limits

- Only Linux (KDE/Wayland) is primarily supported. Running on other platforms may encounter issues.
- Dialog rendering has not been verified visually in automation. Their logic (validation,
  password hashing and writing, QR rendering, settings mapping) is covered by unit tests.
- The settings file is read at startup; the Settings dialog applies changes live.
