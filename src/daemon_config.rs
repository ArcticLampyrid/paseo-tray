//! The daemon settings the tray can edit: one table (`FIELDS`) drives the dialog, the
//! reading of `config.json` values and the `paseo daemon config` edits.
//! Paths, defaults and restart needs follow https://paseo.sh/docs/configuration.md and
//! the `paseo.config.v1` schema.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Network,
    Agents,
    Logging,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Network, Group::Agents, Group::Logging];

    pub fn title(self) -> &'static str {
        match self {
            Group::Network => "Network",
            Group::Agents => "Agents",
            Group::Logging => "Logging",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Free text; empty means "unset".
    Text,
    /// Checkbox; shows `default` while the key is unset.
    Flag { default: bool },
    /// One of `options`; shows `options[default]` while the key is unset.
    Choice {
        options: &'static [&'static str],
        default: usize,
    },
    /// Comma-separated strings stored as an array (a lone `true` stays the boolean).
    List,
    /// Positive whole number; empty means "unset".
    Count,
}

#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub group: Group,
    pub path: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub kind: Kind,
    /// Takes effect only after a daemon restart.
    pub restart: bool,
}

const LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error", "fatal"];

const fn field(
    group: Group,
    path: &'static str,
    label: &'static str,
    hint: &'static str,
    kind: Kind,
    restart: bool,
) -> Field {
    Field {
        group,
        path,
        label,
        hint,
        kind,
        restart,
    }
}

pub const FIELDS: [Field; 11] = [
    field(
        Group::Network,
        "daemon.listen",
        "Bind address",
        "host:port. Use 0.0.0.0:<port> to accept other machines.",
        Kind::Text,
        true,
    ),
    field(
        Group::Network,
        "daemon.relay.enabled",
        "Relay",
        "Reach this daemon through the Paseo relay.",
        Kind::Flag { default: false },
        false,
    ),
    field(
        Group::Network,
        "features.webUi.enabled",
        "WebUI",
        "Serve the bundled web UI from the daemon.",
        Kind::Flag { default: false },
        true,
    ),
    field(
        Group::Network,
        "daemon.hostnames",
        "Extra hostnames",
        "Comma-separated names the daemon accepts, or true for any.",
        Kind::List,
        false,
    ),
    field(
        Group::Agents,
        "daemon.mcp.enabled",
        "MCP server",
        "Expose Paseo's MCP endpoint.",
        Kind::Flag { default: true },
        false,
    ),
    field(
        Group::Agents,
        "daemon.mcp.injectIntoAgents",
        "Inject MCP into agents",
        "Give agents access to Paseo's MCP tools.",
        Kind::Flag { default: false },
        false,
    ),
    field(
        Group::Agents,
        "daemon.autoArchiveAfterMerge",
        "Archive after merge",
        "Archive an agent's worktree once its branch is merged.",
        Kind::Flag { default: false },
        false,
    ),
    field(
        Group::Agents,
        "worktrees.root",
        "Worktrees folder",
        "Empty = <home>/worktrees. Existing worktrees stay where they are.",
        Kind::Text,
        true,
    ),
    field(
        Group::Logging,
        "log.console.level",
        "Console level",
        "",
        Kind::Choice {
            options: LEVELS,
            default: 2,
        },
        true,
    ),
    field(
        Group::Logging,
        "log.file.level",
        "File level",
        "",
        Kind::Choice {
            options: LEVELS,
            default: 0,
        },
        true,
    ),
    field(
        Group::Logging,
        "log.file.rotate.maxFiles",
        "Log files kept",
        "Empty = 2 (the active file plus one rotated).",
        Kind::Count,
        true,
    ),
];

/// One text per field, in `FIELDS` order (flags are "true"/"false").
pub type Form = Vec<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Set {
        path: &'static str,
        value: String,
        literal: bool,
    },
    Unset {
        path: &'static str,
    },
}

impl Edit {
    /// `paseo` arguments. `literal` values are passed with `--string`: paseo would
    /// otherwise parse a value like `6767` as a number.
    pub fn args(&self) -> Vec<String> {
        match self {
            Edit::Set {
                path,
                value,
                literal,
            } => {
                let mut args: Vec<String> =
                    ["daemon", "config", "set", path].map(String::from).into();
                args.push(value.clone());
                if *literal {
                    args.push("--string".into());
                }
                args
            }
            Edit::Unset { path } => ["daemon", "config", "unset", path].map(String::from).into(),
        }
    }
}

fn lookup<'a>(config: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(config, |v, key| v.get(key))
}

fn text_of(kind: Kind, value: Option<&Value>) -> String {
    match (kind, value) {
        (Kind::Flag { .. }, Some(Value::Bool(b))) => b.to_string(),
        (Kind::Flag { default }, _) => default.to_string(),
        (Kind::Choice { options, .. }, Some(Value::String(s))) if options.contains(&s.as_str()) => {
            s.clone()
        }
        (Kind::Choice { options, default }, _) => options[default].to_string(),
        (Kind::Text, Some(Value::String(s))) => s.clone(),
        (Kind::List, Some(Value::Bool(true))) => "true".into(),
        (Kind::List, Some(Value::Array(items))) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        (Kind::Count, Some(Value::Number(n))) => n.to_string(),
        _ => String::new(),
    }
}

/// The form for a `config.json` value (as printed by `paseo daemon config get`).
pub fn read(config: &Value) -> Form {
    FIELDS
        .iter()
        .map(|f| text_of(f.kind, lookup(config, f.path)))
        .collect()
}

fn edit_for(field: &Field, text: &str) -> Result<Edit, String> {
    let text = text.trim();
    let set = |value: String, literal| {
        Ok(Edit::Set {
            path: field.path,
            value,
            literal,
        })
    };
    let unset = Ok(Edit::Unset { path: field.path });
    match field.kind {
        Kind::Text if text.is_empty() => unset,
        Kind::Text | Kind::Choice { .. } => set(text.to_string(), true),
        Kind::Flag { .. } => set(text.to_string(), false),
        Kind::List => {
            let items: Vec<&str> = text
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            match items.as_slice() {
                [] => unset,
                ["true"] => set("true".into(), false),
                _ => set(serde_json::to_string(&items).unwrap_or_default(), false),
            }
        }
        Kind::Count if text.is_empty() => unset,
        Kind::Count => match text.parse::<u32>() {
            Ok(n) if n > 0 => set(n.to_string(), false),
            _ => Err(format!("{} must be a positive whole number", field.label)),
        },
    }
}

/// The edits that turn `from` into `to`; an error names the first invalid field.
pub fn edits(from: &Form, to: &Form) -> Result<Vec<Edit>, String> {
    FIELDS
        .iter()
        .zip(from.iter().zip(to))
        .filter(|(_, (a, b))| a.trim() != b.trim())
        .map(|(field, (_, wanted))| edit_for(field, wanted))
        .collect()
}

/// `form` with the flag at `path` switched on (used by "Enable WebUI…").
pub fn with_flag_on(mut form: Form, path: &str) -> Form {
    if let Some(i) = FIELDS.iter().position(|f| f.path == path) {
        form[i] = "true".into();
    }
    form
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn index(path: &str) -> usize {
        FIELDS.iter().position(|f| f.path == path).unwrap()
    }

    #[test]
    fn paths_are_unique() {
        for (i, f) in FIELDS.iter().enumerate() {
            assert!(
                FIELDS[i + 1..].iter().all(|g| g.path != f.path),
                "{}",
                f.path
            );
        }
    }

    #[test]
    fn unset_keys_show_their_defaults() {
        let form = read(&json!({}));
        assert_eq!(form[index("daemon.mcp.enabled")], "true");
        assert_eq!(form[index("features.webUi.enabled")], "false");
        assert_eq!(form[index("log.console.level")], "info");
        assert_eq!(form[index("log.file.level")], "trace");
        assert_eq!(form[index("daemon.listen")], "");
    }

    #[test]
    fn reads_configured_values() {
        let form = read(&json!({
            "daemon": { "listen": "0.0.0.0:80", "hostnames": ["a", "b"], "relay": { "enabled": true } },
            "log": { "file": { "rotate": { "maxFiles": 5 } } }
        }));
        assert_eq!(form[index("daemon.listen")], "0.0.0.0:80");
        assert_eq!(form[index("daemon.hostnames")], "a, b");
        assert_eq!(form[index("daemon.relay.enabled")], "true");
        assert_eq!(form[index("log.file.rotate.maxFiles")], "5");
        assert_eq!(
            read(&json!({ "daemon": { "hostnames": true } }))[index("daemon.hostnames")],
            "true"
        );
    }

    fn changed(path: &str, text: &str) -> Result<Vec<Edit>, String> {
        let from = read(&json!({}));
        let mut to = from.clone();
        to[index(path)] = text.into();
        edits(&from, &to)
    }

    #[test]
    fn only_changed_fields_become_edits() {
        let form = read(&json!({}));
        assert_eq!(edits(&form, &form), Ok(vec![]));
        assert_eq!(
            changed("daemon.listen", " 0.0.0.0:6767 "),
            Ok(vec![Edit::Set {
                path: "daemon.listen",
                value: "0.0.0.0:6767".into(),
                literal: true
            }])
        );
        assert_eq!(
            changed("daemon.relay.enabled", "true"),
            Ok(vec![Edit::Set {
                path: "daemon.relay.enabled",
                value: "true".into(),
                literal: false
            }])
        );
    }

    #[test]
    fn emptied_fields_are_unset() {
        let from = read(&json!({ "worktrees": { "root": "/w" } }));
        let mut to = from.clone();
        to[index("worktrees.root")].clear();
        assert_eq!(
            edits(&from, &to),
            Ok(vec![Edit::Unset {
                path: "worktrees.root"
            }])
        );
    }

    #[test]
    fn lists_become_json_arrays() {
        assert_eq!(
            changed("daemon.hostnames", "a.test, b.test,"),
            Ok(vec![Edit::Set {
                path: "daemon.hostnames",
                value: r#"["a.test","b.test"]"#.into(),
                literal: false
            }])
        );
        assert_eq!(
            changed("daemon.hostnames", "true"),
            Ok(vec![Edit::Set {
                path: "daemon.hostnames",
                value: "true".into(),
                literal: false
            }])
        );
    }

    #[test]
    fn counts_must_be_positive() {
        assert!(changed("log.file.rotate.maxFiles", "0").is_err());
        assert!(changed("log.file.rotate.maxFiles", "x").is_err());
        assert!(changed("log.file.rotate.maxFiles", "3").is_ok());
    }

    #[test]
    fn edit_args_quote_text_as_strings() {
        let set = Edit::Set {
            path: "daemon.listen",
            value: "6767".into(),
            literal: true,
        };
        assert_eq!(
            set.args(),
            [
                "daemon",
                "config",
                "set",
                "daemon.listen",
                "6767",
                "--string"
            ]
        );
        assert_eq!(
            Edit::Unset {
                path: "worktrees.root"
            }
            .args(),
            ["daemon", "config", "unset", "worktrees.root"]
        );
    }

    #[test]
    fn enabling_a_flag() {
        let form = with_flag_on(read(&json!({})), "features.webUi.enabled");
        assert_eq!(form[index("features.webUi.enabled")], "true");
    }
}
