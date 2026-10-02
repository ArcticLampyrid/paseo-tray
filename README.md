# paseo-tray

A system tray app for the [Paseo](https://github.com/getpaseo/paseo) daemon. It shows
whether the daemon is running and lets you start, stop, restart and configure it from
the tray. It can start the daemon when it launches, stop it when it quits, and start
itself at login.

It works through the `paseo` command-line tool, which must be installed.

## Features

- Daemon status, bound address and relay state at a glance
- Start, stop, restart and reload
- Open the WebUI, or enable it from the tray
- Edit common daemon settings: bind address, relay, WebUI, logging and more
- Pair a device with a QR code
- Set the daemon password
- Settings window: `paseo` location, auto-start/stop of the daemon, launch at login

## Install

You need a desktop with a system tray (on Linux, one that supports StatusNotifierItem,
such as KDE Plasma or GNOME with the AppIndicator extension).

On Arch Linux, install [`paseo-tray-git`](https://aur.archlinux.org/packages/paseo-tray-git)
from the AUR:

```sh
paru paseo-tray-git
```

Elsewhere, build from source with a Rust toolchain:

```sh
cargo install --path .
paseo-tray
```

Open **Configuration → Tray settings…** from the tray menu to point it at a `paseo` that
is not on your `PATH`, or to change what happens when the tray starts and quits.

## Documentation

See [docs/](docs/README.md) for usage, configuration and development notes.

## License

GPL-3.0-only (the UI toolkit, [Slint](https://slint.dev), is used under its GPLv3 terms).
The bundled Paseo logo is Apache-2.0; see [assets/NOTICE](assets/NOTICE).
