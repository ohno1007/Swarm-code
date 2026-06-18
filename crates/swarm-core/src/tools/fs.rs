//! Filesystem tools, sandboxed to the session workspace.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext};

/// Resolve a user-supplied relative path against the workspace, rejecting
/// anything that escapes it.
fn resolve(ctx: &ToolContext, rel: &str) -> anyhow::Result<PathBuf> {
    let candidate = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        ctx.workspace.join(rel)
    };
    let normalized = normalize(&candidate);
    let root = normalize(&ctx.workspace);
    if !normalized.starts_with(&root) {
        anyhow::bail!("path {rel:?} escapes the workspace");
    }
    Ok(normalized)
}

/// Lexical path normalization (no filesystem access, resolves `.`/`..`).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        use std::path::Component::*;
        match comp {
            ParentDir => {
                out.pop();
            }
            CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a UTF-8 text file from the workspace and return its contents."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path relative to the workspace root." }
            },
            "required": ["path"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let path = require_path(&args)?;
        let resolved = resolve(ctx, path)?;
        let content = tokio::fs::read_to_string(&resolved).await?;
        Ok(content)
    }
}

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Create or overwrite a text file in the workspace."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path relative to the workspace root." },
                "content": { "type": "string", "description": "Full file contents to write." }
            },
            "required": ["path", "content"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let path = require_path(&args)?;
        let content = args["content"].as_str().unwrap_or("");
        let resolved = resolve(ctx, path)?;
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&resolved, content).await?;
        Ok(format!("wrote {} bytes to {path}", content.len()))
    }
}

pub struct ListDir;

#[async_trait]
impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "List the entries of a directory in the workspace."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path relative to the workspace root (default '.')." }
            }
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let path = args["path"].as_str().unwrap_or(".");
        let resolved = resolve(ctx, path)?;
        let mut entries = tokio::fs::read_dir(&resolved).await?;
        let mut out = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let suffix = if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                "/"
            } else {
                ""
            };
            out.push(format!("{}{suffix}", entry.file_name().to_string_lossy()));
        }
        out.sort();
        Ok(out.join("\n"))
    }
}

/// Extract the required `path` string argument.
fn require_path(args: &Value) -> anyhow::Result<&str> {
    args["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing required argument: path"))
}
