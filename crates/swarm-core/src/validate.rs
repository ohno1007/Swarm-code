//! Compile validation.
//!
//! After committing changes we run a fast compile check so the swarm gets
//! immediate feedback. For Rust projects that's `cargo check`, which is much
//! faster than a full build. Other ecosystems can implement [`Validator`].

use std::path::Path;

use async_trait::async_trait;

/// Result of a validation run.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    /// Combined, truncated tool output.
    pub output: String,
}

impl ValidationResult {
    /// A short one-line summary for events/logs.
    pub fn summary(&self) -> String {
        if self.ok {
            "passed".to_string()
        } else {
            let first = self
                .output
                .lines()
                .find(|l| l.contains("error"))
                .unwrap_or("compile errors");
            first.trim().chars().take(120).collect()
        }
    }
}

#[async_trait]
pub trait Validator: Send + Sync {
    fn name(&self) -> &str;
    /// Whether this validator applies to the given workspace.
    fn applies(&self, workspace: &Path) -> bool;
    async fn validate(&self, workspace: &Path) -> ValidationResult;
}

/// `cargo check` validator for Rust workspaces.
pub struct CargoCheck;

#[async_trait]
impl Validator for CargoCheck {
    fn name(&self) -> &str {
        "cargo check"
    }

    fn applies(&self, workspace: &Path) -> bool {
        workspace.join("Cargo.toml").exists()
    }

    async fn validate(&self, workspace: &Path) -> ValidationResult {
        let output = tokio::process::Command::new("cargo")
            .arg("check")
            .arg("--message-format=short")
            .current_dir(workspace)
            .output()
            .await;

        match output {
            Ok(out) => {
                let mut text = String::new();
                text.push_str(&String::from_utf8_lossy(&out.stdout));
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                ValidationResult {
                    ok: out.status.success(),
                    output: truncate(&text, 8000),
                }
            }
            Err(e) => ValidationResult {
                ok: false,
                output: format!("failed to run cargo: {e}"),
            },
        }
    }
}

/// Pick the first applicable validator for a workspace.
pub fn for_workspace(workspace: &Path) -> Option<Box<dyn Validator>> {
    let candidates: Vec<Box<dyn Validator>> = vec![Box::new(CargoCheck)];
    candidates.into_iter().find(|v| v.applies(workspace))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}\n…[truncated]", &s[..max])
    }
}
