//! In-CLI credential management, so users never have to fiddle with env vars.
//!
//! Keys are read from (in order): the process environment, then a small config
//! file at `~/.config/swarm-code/config`. If neither has a key, the CLI prompts
//! for one interactively and offers to save it.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

const KEY_VAR: &str = "DEEPSEEK_API_KEY";

/// Path to the config file (`~/.config/swarm-code/config`).
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("swarm-code").join("config")
}

/// Parse the config file into a map (missing file → empty).
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

/// Load any config values into the environment without overriding existing vars.
pub fn load_into_env() {
    for (k, v) in read_config() {
        if std::env::var(&k).is_err() {
            std::env::set_var(&k, v);
        }
    }
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

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}

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

/// Ensure a DeepSeek API key is available, prompting interactively if needed.
pub fn ensure_api_key() -> anyhow::Result<()> {
    load_into_env();
    if std::env::var(KEY_VAR).map(|v| !v.is_empty()).unwrap_or(false) {
        return Ok(());
    }

    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        anyhow::bail!(
            "no DeepSeek API key found. Set {KEY_VAR}, run `swarm config set-key`, \
             or add it to {}",
            config_path().display()
        );
    }

    eprintln!("No DeepSeek API key configured.");
    let key = prompt("Enter your DeepSeek API key: ")?;
    if key.is_empty() {
        anyhow::bail!("no key entered");
    }
    std::env::set_var(KEY_VAR, &key);

    let save_it = prompt("Save it to ~/.config/swarm-code/config for next time? [Y/n] ")?;
    if !save_it.eq_ignore_ascii_case("n") {
        save(KEY_VAR, &key)?;
        eprintln!("saved to {}", config_path().display());
    }
    Ok(())
}

/// Interactive set-key (for `swarm config set-key`).
pub fn set_key_interactive() -> anyhow::Result<()> {
    let key = prompt("Enter your DeepSeek API key: ")?;
    if key.is_empty() {
        anyhow::bail!("no key entered");
    }
    save(KEY_VAR, &key)?;
    eprintln!("saved to {}", config_path().display());
    Ok(())
}

/// Print the current (masked) configuration.
pub fn show() {
    load_into_env();
    let path = config_path();
    println!("config file: {}", path.display());
    match std::env::var(KEY_VAR) {
        Ok(k) if !k.is_empty() => println!("{KEY_VAR}: {}", mask(&k)),
        _ => println!("{KEY_VAR}: (not set)"),
    }
    if let Ok(model) = std::env::var("DEEPSEEK_MODEL") {
        println!("DEEPSEEK_MODEL: {model}");
    }
    if let Ok(base) = std::env::var("DEEPSEEK_BASE_URL") {
        println!("DEEPSEEK_BASE_URL: {base}");
    }
}

fn prompt(msg: &str) -> anyhow::Result<String> {
    eprint!("{msg}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}
