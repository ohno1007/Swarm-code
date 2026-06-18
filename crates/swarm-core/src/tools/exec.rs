//! Terminal access for agents.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext};
use crate::validate;

/// Run a shell command in the workspace and capture its output.
///
/// This is the agent's terminal. Commands run with the workspace as the working
/// directory. Output is captured (stdout + stderr) and truncated.
pub struct RunCommand;

#[async_trait]
impl Tool for RunCommand {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Run a shell command in the workspace and return its stdout, stderr and \
         exit code. Use for builds, tests, git, grep, etc. Long-running or \
         interactive commands are not supported (60s timeout)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command line to execute." },
                "timeout_secs": { "type": "integer", "description": "Optional timeout in seconds (default 60, max 300)." }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: command"))?;
        let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(60).min(300);

        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(&ctx.workspace)
            .kill_on_drop(true);

        let fut = cmd.output();
        let out = match tokio::time::timeout(Duration::from_secs(timeout_secs), fut).await {
            Ok(res) => res?,
            Err(_) => anyhow::bail!("command timed out after {timeout_secs}s: {command}"),
        };

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let mut result = format!("exit: {}\n", out.status.code().unwrap_or(-1));
        if !stdout.is_empty() {
            result.push_str("--- stdout ---\n");
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            result.push_str("--- stderr ---\n");
            result.push_str(&stderr);
        }
        Ok(truncate(&result, 16_000))
    }
}

/// Run the workspace's compile check on demand (without committing).
pub struct CargoCheckTool;

#[async_trait]
impl Tool for CargoCheckTool {
    fn name(&self) -> &str {
        "cargo_check"
    }
    fn description(&self) -> &str {
        "Run a fast compile check on the workspace as it exists on disk (cargo \
         check for Rust). Note: this checks committed files, not staged changes."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        match validate::for_workspace(&ctx.workspace) {
            Some(v) => {
                let r = v.validate(&ctx.workspace).await;
                Ok(format!(
                    "{} -> {}\n{}",
                    v.name(),
                    if r.ok { "ok" } else { "FAILED" },
                    r.output
                ))
            }
            None => Ok("no compile validator applies to this workspace".to_string()),
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}\n…[truncated]", &s[..max])
    }
}
