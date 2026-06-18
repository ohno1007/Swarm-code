//! Coordination tools: symbol-level editing/locking and the commit workflow.

use async_trait::async_trait;
use serde_json::{json, Value};
use swarm_analyzer::{Analyzer, Symbol};

use crate::lock::LockResult;
use crate::tool::{Tool, ToolContext};
use crate::tools::fs::workspace_rel;

/// Find the first symbol matching `name` (depth-first) and return its line span.
fn find_symbol<'a>(node: &'a Symbol, name: &str) -> Option<&'a Symbol> {
    if node.name == name {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_symbol(child, name) {
            return Some(found);
        }
    }
    None
}

/// Edit a single symbol's source range, guarded by a symbol-level lock.
///
/// This is finer-grained than a file lock: two agents may edit two different
/// functions in the same file concurrently.
pub struct EditSymbol;

#[async_trait]
impl Tool for EditSymbol {
    fn name(&self) -> &str {
        "edit_symbol"
    }

    fn description(&self) -> &str {
        "Replace the full source of a named symbol (function, struct, impl, ...) \
         with new text, after acquiring a symbol-level lock. Fails if another \
         agent holds the lock. The edit is staged into the change buffer."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to the workspace." },
                "symbol": { "type": "string", "description": "Name of the symbol to replace." },
                "new_source": { "type": "string", "description": "Full replacement source for the symbol." }
            },
            "required": ["path", "symbol", "new_source"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let symbol = args["symbol"].as_str().ok_or_else(|| anyhow::anyhow!("missing symbol"))?;
        let new_source = args["new_source"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing new_source"))?;

        let rel = workspace_rel(ctx, path)?;
        let content = ctx
            .coordinator
            .buffer
            .read(&rel)?
            .ok_or_else(|| anyhow::anyhow!("file not found: {path}"))?;

        // Locate the symbol's line span.
        let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("source");
        let root = Analyzer::new()
            .analyze_source(name, &content)
            .map_err(|e| anyhow::anyhow!("analyze failed: {e}"))?;
        let target = find_symbol(&root, symbol)
            .ok_or_else(|| anyhow::anyhow!("symbol '{symbol}' not found in {path}"))?;
        let (start, end) = (target.start_line, target.end_line);

        // Acquire the symbol lock.
        match ctx.coordinator.locks.try_acquire(&rel, symbol, &ctx.agent) {
            LockResult::Acquired => {}
            LockResult::Held { owner } => {
                anyhow::bail!("symbol '{symbol}' is locked by {owner}");
            }
        }

        // Splice [start..=end] (1-based, inclusive) with the new source.
        let lines: Vec<&str> = content.lines().collect();
        let mut out = String::new();
        for line in &lines[..start.saturating_sub(1)] {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(new_source.trim_end_matches('\n'));
        out.push('\n');
        for line in &lines[end.min(lines.len())..] {
            out.push_str(line);
            out.push('\n');
        }

        ctx.coordinator.buffer.stage_write(&rel, out, &ctx.agent);
        Ok(format!(
            "staged edit of symbol '{symbol}' ({path}:{start}-{end}); lock held by {}",
            ctx.agent
        ))
    }
}

/// Inspect the pending changes (status + unified diff).
pub struct ViewChanges;

#[async_trait]
impl Tool for ViewChanges {
    fn name(&self) -> &str {
        "view_changes"
    }
    fn description(&self) -> &str {
        "Show pending staged changes in the change buffer as a unified diff."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let pending = ctx.coordinator.buffer.pending();
        if pending.is_empty() {
            return Ok("no pending changes".to_string());
        }
        let mut out = String::from("Pending changes:\n");
        for c in &pending {
            out.push_str(&format!(
                "  {} {} (by {})\n",
                c.kind,
                c.path.display(),
                c.authors.join(", ")
            ));
        }
        out.push('\n');
        out.push_str(&ctx.coordinator.buffer.diff());
        Ok(out)
    }
}

/// Apply staged changes to disk and run compile validation.
pub struct CommitChanges;

#[async_trait]
impl Tool for CommitChanges {
    fn name(&self) -> &str {
        "commit_changes"
    }
    fn description(&self) -> &str {
        "Apply all staged changes to disk, release your symbol locks, and run a \
         compile check (cargo check for Rust). Returns the validation result. \
         If validation fails, fix the code and commit again."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        if !ctx.coordinator.buffer.has_pending(&ctx.agent) {
            return Ok("nothing to commit (you have no staged changes)".to_string());
        }
        let (paths, validation) = ctx.coordinator.commit(&ctx.agent).await?;
        let mut out = format!("committed {} file(s):\n", paths.len());
        for p in &paths {
            out.push_str(&format!("  {}\n", p.display()));
        }
        match validation {
            Some(v) if v.ok => out.push_str("\n✅ compile check passed"),
            Some(v) => out.push_str(&format!("\n❌ compile check FAILED:\n{}", v.output)),
            None => out.push_str("\n(no validator for this workspace)"),
        }
        Ok(out)
    }
}

/// Throw away staged changes and release locks.
pub struct DiscardChanges;

#[async_trait]
impl Tool for DiscardChanges {
    fn name(&self) -> &str {
        "discard_changes"
    }
    fn description(&self) -> &str {
        "Discard all your staged changes and release your symbol locks."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        ctx.coordinator.discard(&ctx.agent);
        Ok("discarded staged changes and released locks".to_string())
    }
}

/// List currently held symbol locks.
pub struct ListLocks;

#[async_trait]
impl Tool for ListLocks {
    fn name(&self) -> &str {
        "list_locks"
    }
    fn description(&self) -> &str {
        "List all currently held symbol locks and their owners."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let locks = ctx.coordinator.locks.list();
        if locks.is_empty() {
            return Ok("no locks held".to_string());
        }
        Ok(locks
            .into_iter()
            .map(|(k, o)| format!("{k}  ->  {o}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
