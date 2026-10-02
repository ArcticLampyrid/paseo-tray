# Usage

## Tray icon

The Paseo logo on a rounded square. The colour shows the daemon status:

| Icon                    | Meaning                                             |
| ----------------------- | --------------------------------------------------- |
| green square            | daemon running                                      |
| grey outline, grey logo | daemon stopped                                      |
| amber square            | checking, starting, stopping, restarting, reloading |
| red square              | the `paseo` CLI is missing or failing               |

Only "stopped" differs by shape; the other states are told apart by colour alone.

## Clicking the icon

A left-click does the most useful next thing for the current state (the menu is on
right-click):

| Daemon state                      | Left-click                               |
| --------------------------------- | ---------------------------------------- |
| stopped                           | start the daemon                         |
| running, WebUI enabled            | open the WebUI                           |
| running, WebUI disabled           | nothing                                  |
| `paseo` missing or failing        | open the tray settings                   |
| busy, or before the first check   | nothing                                  |

## Menu

- **Status**: the daemon state, with a coloured dot (🟢 running, ⚪ stopped, 🟡 busy,
  🔴 unavailable).
- **Address / Relay** (marked ✎): the address the daemon is bound to (the configured one,
  marked "(configured)", while stopped) and whether the relay is enabled. Clicking either
  opens the daemon settings.
- **Start / Stop daemon**, **Restart daemon**, **Reload config**: restart and reload are
  available only while the daemon is running. While one of these commands runs, the status
  shows the transition (for example "restarting…") and the controls are disabled.
- **Open WebUI**: available when the WebUI is enabled in the daemon config. When it is
  not, the item is shown greyed out as "Open WebUI (disabled)", with an **Enable WebUI…**
  item below it that opens the daemon settings with the WebUI box already ticked.
- **Pair a device…**: a window with the pairing QR code and link. If the relay is off, it
  asks before enabling it, because that changes the daemon config.
- **Set password…**: a window with new and confirm fields. The password is saved to the
  daemon config; a running daemon has to be restarted to use it, and the window offers to
  do that.
- **Configuration**:
  - *Tray settings…*: see [configuration.md](configuration.md).
  - *Daemon settings…*: bind address, relay, WebUI and more (see below).
  - *View daemon log*.
- **Quit**: stops the daemon first if the "stop daemon when the tray quits" setting is on.
  SIGINT and SIGTERM do the same.

The most recent command failure appears as an `Error:` line at the top of the menu.

## Daemon settings

A window for the daemon's own configuration, with the groups listed on the left:

- **Network**: bind address (use `0.0.0.0:<port>` to accept other machines), relay, WebUI,
  extra accepted hostnames.
- **Agents**: the MCP server and its injection into agents, archiving after merge, the
  worktrees folder.
- **Logging**: console and file log levels, how many log files are kept.

Settings marked ↻ only take effect after a daemon restart. Emptying a text field removes the
key, so the daemon's default applies again. **Open config.json** edits the file directly for
anything not listed here.

Saving applies what a running daemon can take immediately (the relay) and tells you which
changes need a restart, with a button to restart now. If
the daemon is stopped, the changes take effect when it next starts.
