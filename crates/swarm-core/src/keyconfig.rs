//! Shared credential storage (used by both the CLI and the GUI).
//!
//! Keys are read from the process environment, falling back to a small config
//! file at `~/.config/swarm-code/config`. The CLI adds an interactive prompt on
//! top of this; the GUI exposes a settings dialog.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub const KEY_VAR: &str = "DEEPSEEK_API_KEY";

/// Path to the config file (`~/.config/swarm-code/config`).
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("swarm-code")
        .join("config")
}

fn read_config() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(config_path()) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    map
}

/// Read a single config value from the file (not the environment).
pub fn get(key: &str) -> Option<String> {
    read_config().get(key).cloned().filter(|v| !v.is_empty())
}

/// Load config values into the environment without overriding existing vars.
pub fn load_into_env() {
    for (k, v) in read_config() {
        if std::env::var(&k).is_err() {
            std::env::set_var(&k, v);
        }
    }
}

/// Whether a non-empty API key is available (env or config file).
pub fn has_key() -> bool {
    load_into_env();
    std::env::var(KEY_VAR).map(|v| !v.is_empty()).unwrap_or(false)
}

/// Persist a single key/value into the config file (creating it, mode 0600).
pub fn save(key: &str, value: &str) -> std::io::Result<()> {
    let mut map = read_config();
    map.insert(key.to_string(), value.to_string());

    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut body = String::from("# Swarm-code configuration\n");
    for (k, v) in &map {
        body.push_str(&format!("{k}={v}\n"));
    }
    std::fs::write(&path, body)?;
    restrict_permissions(&path);
    Ok(())
}

/// Save the API key and load it into the environment for immediate use.
pub fn save_key(key: &str) -> std::io::Result<()> {
    save(KEY_VAR, key)?;
    std::env::set_var(KEY_VAR, key);
    Ok(())
}

/// Mask a secret for display: `sk-a…wxyz`.
pub fn mask(secret: &str) -> String {
    let n = secret.chars().count();
    if n <= 8 {
        "•".repeat(n)
    } else {
        let head: String = secret.chars().take(4).collect();
        let tail: String = secret.chars().skip(n - 4).collect();
        format!("{head}…{tail}")
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}
