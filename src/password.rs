//! Daemon password: hashed here and written into paseo's `config.json`, using
//! paseo's own scheme (bcrypt, cost 12, plain password; stored at
//! `daemon.auth.password`). The daemon must be restarted to pick it up.

use crate::fsutil;
use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const BCRYPT_COST: u32 = 12;
/// bcrypt ignores input past this many bytes.
pub const MAX_PASSWORD_BYTES: usize = 72;

/// Where the daemon's home is: what paseo reports, else the configured `--home`, else `~/.paseo`.
pub fn resolve_home(reported: Option<PathBuf>, configured: Option<PathBuf>) -> Option<PathBuf> {
    reported
        .or(configured)
        .or_else(|| dirs::home_dir().map(|h| h.join(".paseo")))
}

pub fn set_password(home: &Path, password: &str) -> Result<()> {
    // Hash first (it takes a few hundred ms): the read-modify-write below then only
    // spans microseconds, which keeps the window for racing a `paseo daemon config` edit tiny.
    let hash = bcrypt::hash(password, BCRYPT_COST).context("hashing the password")?;
    let path = home.join("config.json");
    let existing = match fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({ "version": 1 }),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let contents = serde_json::to_string_pretty(&with_password_hash(existing, &hash))?;
    fsutil::write_atomically(&path, &format!("{contents}\n"))
}

/// Sets `daemon.auth.password`, leaving every other key untouched.
fn with_password_hash(mut config: Value, hash: &str) -> Value {
    let mut auth = &mut config;
    for key in ["daemon", "auth"] {
        if !auth.is_object() {
            *auth = Value::Object(Map::new());
        }
        auth = auth
            .as_object_mut()
            .expect("just ensured an object")
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !auth.is_object() {
        *auth = Value::Object(Map::new());
    }
    auth["password"] = Value::String(hash.into());
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_the_password_and_keeps_key_order() {
        let before = json!({
            "version": 1,
            "daemon": { "listen": "127.0.0.1:6767", "auth": { "password": "old", "other": 1 } },
            "features": { "webUi": { "enabled": true } }
        });
        let after = with_password_hash(before.clone(), "NEW");
        assert_eq!(
            after["daemon"]["auth"],
            json!({ "password": "NEW", "other": 1 })
        );
        assert_eq!(after["daemon"]["listen"], before["daemon"]["listen"]);
        assert_eq!(after["features"], before["features"]);
        assert_eq!(
            after.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["version", "daemon", "features"]
        );
    }

    #[test]
    fn creates_missing_sections() {
        let after = with_password_hash(json!({ "version": 1 }), "H");
        assert_eq!(after["daemon"]["auth"]["password"], "H");
    }

    #[test]
    fn writes_a_verifiable_hash_to_disk() {
        let home = std::env::temp_dir().join(format!("paseo-tray-test-{}", std::process::id()));
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join("config.json"),
            r#"{"version":1,"daemon":{"listen":"127.0.0.1:1"}}"#,
        )
        .unwrap();
        set_password(&home, "s3cret").unwrap();
        let config: Value =
            serde_json::from_str(&fs::read_to_string(home.join("config.json")).unwrap()).unwrap();
        let hash = config["daemon"]["auth"]["password"].as_str().unwrap();
        assert!(hash.starts_with("$2b$12$"), "{hash}");
        assert!(bcrypt::verify("s3cret", hash).unwrap());
        assert_eq!(config["daemon"]["listen"], "127.0.0.1:1");
        fs::remove_dir_all(&home).unwrap();
    }
}

#[cfg(test)]
mod interop {
    /// `PASEO_TRAY_TEST_HOME=/tmp/scratch PASEO_TRAY_TEST_PASSWORD=pw cargo test -- --ignored`
    /// writes a hash into a scratch home so a real daemon can be tried against it.
    #[test]
    #[ignore]
    fn write_password_into_scratch_home() {
        let home = std::env::var("PASEO_TRAY_TEST_HOME").expect("PASEO_TRAY_TEST_HOME");
        let password = std::env::var("PASEO_TRAY_TEST_PASSWORD").expect("PASEO_TRAY_TEST_PASSWORD");
        super::set_password(std::path::Path::new(&home), &password).unwrap();
    }
}
