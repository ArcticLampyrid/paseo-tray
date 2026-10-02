# Paseo integration

The tray talks to the daemon only through the `paseo` CLI, except for the password, which
is written to the daemon's `config.json` directly. Every invocation goes through
`Cli::argv`, which adds the configured program path and `--home`.

## Reading status

Every `poll_interval_secs`, and right after each command finishes, the tray runs:

- `paseo daemon status --json`
  - `localDaemon`: `running`, `not_ready` (shown as starting) or `stopped`
  - `listen`: the bound address; `configuredListen` is shown instead while stopped
  - `relay.enabled`: present only while the daemon is live
  - `home`, `logPath`: used by the config and log menu items
- `paseo daemon config get features.webUi.enabled`: decides whether "Open WebUI" is
  available or shown as disabled with an "Enable WebUI…" item.
- `paseo daemon config get daemon.relay.enabled`: only when the status JSON has no `relay`
  block (the stopped case). Unset or unreadable values count as disabled, matching the
  daemon's default.

If the command fails or `paseo` is not found, the status becomes "unavailable" with the
error text.

Each poll starts one or more Node processes. If that becomes a problem, raise the interval.

## Commands

| Action  | Command |
| ------- | ------- |
| Start   | `paseo daemon start` |
| Stop    | `paseo daemon stop` |
| Restart | `paseo daemon restart` |
| Reload  | `paseo reload` |
| Pair    | `paseo daemon pair --json` (`--relay` after the user consents) |

## Editing the daemon config

The daemon settings window reads the whole config with `paseo daemon config get --json`
once when it opens, then runs `config set` (or `config unset` for an emptied field) for each
changed value. The editable fields are one table, `FIELDS` in `src/daemon_config.rs`: path,
label, kind, default shown while the key is unset, and whether a restart is needed. Defaults
and paths follow <https://paseo.sh/docs/configuration.md> and the `paseo.config.v1` schema.

Value handling: text is passed with `--string` (otherwise a value like `6767` is parsed as a
number); flags as `true`/`false`; the hostnames list as a JSON array (a lone `true` stays the
boolean); counts as numbers.

Run with `--json`, the command saves the value, and a running daemon reloads it. The result
lists `appliedPaths` and `restartRequiredPaths`; the window offers a restart only for the
latter (the daemon decides; for example the bind address and the WebUI need one, the relay does not). `daemon.auth`
cannot be edited this way; that is why the password is written separately (see below).

## Open WebUI

The URL is built in `Snapshot::web_ui_url` from the live `listen` address, and only while
the daemon is running with the WebUI feature enabled:

- Split at the last `:` into host and port; no URL if the port is not a valid `u16` or the
  address is not `host:port` (for example a unix socket).
- Wildcard hosts (empty, `0.0.0.0`, `::`, `[::]`) become `127.0.0.1`, since a wildcard
  address cannot be browsed to.
- An IPv6 literal without brackets is bracketed.
- The scheme is always `http://`.

Known gaps: `[::]` assumes the daemon binds dual-stack (the Linux default), and forms such
as `[::ffff:0.0.0.0]` are not treated as wildcards.

## Pairing

`paseo daemon pair --json` returns a `url`. With the relay disabled it fails with
`RELAY_DISABLED`; the dialog then asks for consent and re-runs the command with `--relay`,
which enables the relay in the daemon config. The QR code is rendered from `url` by the
tray (the CLI's own `qr` field is terminal art).

## Password

`paseo daemon set-password` needs a TTY, so the tray reproduces what it does:

- bcrypt, cost 12, over the plain password (no pre-hashing), stored at
  `daemon.auth.password` in `<home>/config.json`.
- All other keys, and their order, are preserved. The file is written to a temporary file
  and renamed; new files get mode `0600`.
- bcrypt ignores input past 72 bytes, so longer passwords are rejected.
- The daemon must be restarted to use the new password.

The scheme mirrors upstream (`packages/cli/src/commands/daemon/set-password.ts` and
`packages/server/src/server/auth.ts` in the Paseo repo). If paseo changes it, update
`password.rs`. `paseo daemon config set` refuses to edit `daemon.auth`, which is why this
is not done through the CLI. The password is never logged.

The daemon home is what `paseo` reports, else the `home` setting, else `~/.paseo`.

## Limits

Paseo has `PASEO_*` environment overrides (for example `PASEO_WEB_UI_ENABLED`) that the
tray cannot see, so the WebUI and relay display reflects the persisted config.
