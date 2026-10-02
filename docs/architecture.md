# Architecture

The app is a small Elm-style loop around a pure state machine:

```
events ──► state::update (pure) ──► new state + effects ──► effects run ──► more events
```

All behavioural rules (auto-start on the first poll, stopping the daemon on quit, ignoring
commands while busy, applying saved settings) live in `state::update` and are unit-tested
without any UI or `paseo` process.

## Modules

| Module          | Role |
| --------------- | ---- |
| `app.rs`        | Wiring. Owns the tray, dialogs and poller on the UI thread and runs `Effect`s. `send(Event)` is the only way other threads reach it. |
| `state.rs`      | `AppState`, `Event`, `Effect` and the pure `update`. No I/O. |
| `view.rs`       | `view(&AppState) -> View`: a pure model of what the tray shows and enables. |
| `ui/tray.slint` | Menu layout and wording, bound to `View` fields. Item clicks send an `Action` id. |
| `ui/app.slint`  | The password, pairing and settings dialogs. |
| `dialogs.rs`    | Wires the dialogs. Blocking work runs on threads and returns through `upgrade_in_event_loop`. |
| `daemon.rs`     | `Status` and `Snapshot`, parsed from the CLI; reads and edits the daemon config. |
| `daemon_config.rs` | The table of editable daemon settings and the pure form ⇄ edits logic. Add a setting by adding a `FIELDS` row. |
| `fsutil.rs`     | Atomic, owner-only file replacement used for every file the tray writes. |
| `actions.rs`    | The `Action` enum (also the menu-item id), `Dialog`, and `execute` for actions with side effects. |
| `cli.rs`        | The only code that spawns `paseo`. Owns the program path and `--home`. |
| `password.rs`   | Hashes the daemon password and writes it into the daemon config. |
| `pairing.rs`    | Fetches the pairing offer and renders its QR code. |
| `config.rs`     | The tray's own settings file, the only persistent state the app owns. |
| `autostart.rs`  | `auto-launch` wrapper for "launch at login". |
| `icon.rs`       | Composes the logo SVG on a status-coloured square and rasterizes it with `resvg`. `Tone::of(Status)` maps status to icon. |

## Status has two layers

`Snapshot.status` is what the last poll observed. `AppState::busy` overlays a transitional
status (starting, stopping, …) while a command runs. Use `AppState::observed()` for
decisions about the daemon and `AppState::status()` only for display.

## Threading

Everything that touches Slint runs on the UI thread. Threads (the poller, command workers,
dialog workers) never touch UI objects directly: they call `app::send(Event)`, or use a
`slint::Weak` with `upgrade_in_event_loop`. Anything that shells out runs via
`Effect::Spawn` so the event loop never blocks.

## Adding a menu action

1. Add the `Action` variant (and to `Action::ALL`) in `actions.rs`.
2. Handle it in `state::choose` (state change or dialog) or `actions::execute` (side effect).
3. Add the item in `ui/tray.slint`, plus a `View` field if its visibility or enabled state
   varies.
4. Add a test in `state.rs` for any new behaviour.

## Slint notes

- A `SystemTrayIcon` root exposes none of its builtin properties or callbacks to Rust, so
  `ui/tray.slint` re-exposes them: `status-icon`, `status-tooltip` and `tray-clicked`.
- Menu items cannot show icons on any backend in Slint 1.18 (the Linux backend only passes
  label and enabled), so the status dot is a colour-circle glyph in the label.
- Its `Menu` may use `if` and `for`, but cannot itself sit inside one.
- Menu items are not used as checkboxes; toggles live in the Settings dialog.
