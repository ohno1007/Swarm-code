//! The `spawn_agent` tool: lets the lead agent delegate to worker agents.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext};

/// Delegate a self-contained subtask to a fresh worker agent running
/// concurrently with its own context window.
pub struct SpawnAgent;

#[async_trait]
impl Tool for SpawnAgent {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Delegate a focused, self-contained subtask to a worker agent. The \
         worker has the same tools and works in its own context window, then \
         returns a final result. Use this to parallelize research or split a \
         large task. Provide a clear role and a complete task description."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "role": {
                    "type": "string",
                    "description": "Short role for the worker, e.g. 'code-reader' or 'test-writer'."
                },
                "task": {
                    "type": "string",
                    "description": "Complete, standalone description of what the worker must do and return."
                }
            },
            "required": ["role", "task"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let role = args["role"].as_str().unwrap_or("worker");
        let task = args["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: task"))?;

        let Some(spawner) = &ctx.spawner else {
            anyhow::bail!("sub-agents cannot spawn further agents (depth limit reached)");
        };
        spawner.spawn(role, task).await
    }
}
