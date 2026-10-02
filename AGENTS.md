# AGENTS.md

Guidance for AI agents and contributors working on `paseo-tray`.

## What this is

A Rust system tray app (Slint UI and `SystemTrayIcon`) that observes and controls the
Paseo daemon through the `paseo` CLI. Linux is the primary target.

## Commands

```sh
cargo build
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings   # keep warning-free (CI enforces it)
```

Never test against the user's real daemon (`~/.paseo`, port 6767); use a scratch home on
another port ([docs/development.md](docs/development.md)).

## Principles

- **Data flows one way:** events → pure `state::update` → new state + effects → effects
  run → more events. Behaviour belongs in `update`, with a unit test; keep I/O out of
  `state.rs` and `view.rs`.
- **Single source of truth.** Daemon facts come from the CLI, never cached in settings.
  The tray's own settings file is its only persistent state. Every `paseo` invocation goes
  through `Cli::argv`.
- **Immutable by default.** State is cloned and replaced. Other threads reach the UI only
  by sending events or via `upgrade_in_event_loop`.
- **Never block the event loop.** Anything that shells out runs via `Effect::Spawn`.
- **Mirror paseo, don't reinvent it.** Where the tray reproduces paseo's behaviour (for
  example the password scheme), follow upstream and note the source in the docs.
- **Fail safe.** Unrecognised daemon output is "unavailable", never "stopped". Shutdown
  waits for a running command. Multi-step saves roll back on failure. Replies to superseded
  requests are dropped. Files are replaced atomically via `fsutil`.
- Never log secrets.
- Comment constraints and reasons, not what the code already says.

## Documentation

- `README.md`: for end users; keep it short and link to `docs/`.
- `AGENTS.md`: general rules and concepts only.
- `docs/`: everything detailed, one topic per file, indexed in
  [docs/README.md](docs/README.md). Update the relevant file when behaviour changes.
