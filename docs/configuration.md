# Configuration

Use **Configuration → Settings…** in the tray menu. Changes apply immediately, without
restarting the tray.

Settings are stored in `~/.config/paseo-tray/config.toml` (on Linux; the platform's
config directory elsewhere). You can edit the file by hand, but it is only read when the
tray starts. Every key is optional.

```toml
cli_path = "/usr/local/bin/paseo"   # omit to search PATH
home = "/home/me/.paseo"            # passed as --home; omit for paseo's default (~/.paseo)
auto_start_daemon  = true           # start the daemon when the tray launches
auto_stop_daemon   = true           # stop the daemon when the tray quits
poll_interval_secs = 5              # how often to check the daemon, at least 1
```

## Daemon lifecycle

- `auto_start_daemon` starts the daemon after the first status check, only if it is
  stopped. A daemon that is already running is left alone.
- `auto_stop_daemon` stops the daemon on quit even if it was already running before the
  tray started.

## Launch at login

A checkbox in the Settings window. It registers the tray's current executable with the OS
(an XDG autostart entry on Linux, a LaunchAgent on macOS, a registry entry on Windows), so
install the binary somewhere stable first. The OS registration is the only record of this
setting; it is not stored in `config.toml`.
