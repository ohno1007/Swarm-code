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
        spawner.spawn(role, task, ctx.observer.clone()).await
    }
}

/// Fan out several worker agents that run concurrently.
pub struct SpawnAgents;

#[async_trait]
impl Tool for SpawnAgents {
    fn name(&self) -> &str {
        "spawn_agents"
    }

    fn description(&self) -> &str {
        "Delegate MULTIPLE independent subtasks at once; the workers run in \
         parallel (their LLM calls and tool I/O overlap) and you get all results \
         back together. They share the change buffer and symbol locks, so \
         concurrent edits are coordinated. Use this to parallelize work; prefer \
         it over calling spawn_agent repeatedly."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agents": {
                    "type": "array",
                    "description": "The workers to run concurrently.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": { "type": "string", "description": "Short role label." },
                            "task": { "type": "string", "description": "Complete, standalone task for this worker." }
                        },
                        "required": ["role", "task"]
                    }
                }
            },
            "required": ["agents"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let agents = args["agents"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: agents (array)"))?;
        let tasks: Vec<(String, String)> = agents
            .iter()
            .map(|a| {
                let role = a["role"].as_str().unwrap_or("worker").to_string();
                let task = a["task"].as_str().unwrap_or_default().to_string();
                (role, task)
            })
            .filter(|(_, t)| !t.is_empty())
            .collect();
        if tasks.is_empty() {
            anyhow::bail!("no valid agents provided");
        }

        let Some(spawner) = &ctx.spawner else {
            anyhow::bail!("sub-agents cannot spawn further agents (depth limit reached)");
        };

        let roles: Vec<String> = tasks.iter().map(|(r, _)| r.clone()).collect();
        let results = spawner.spawn_many(tasks, ctx.observer.clone()).await;

        let mut out = String::new();
        for (i, (role, result)) in roles.iter().zip(results).enumerate() {
            out.push_str(&format!("## worker[{i}] {role}\n"));
            match result {
                Ok(answer) => out.push_str(&answer),
                Err(e) => out.push_str(&format!("(failed: {e})")),
            }
            out.push_str("\n\n");
        }
        Ok(out.trim_end().to_string())
    }
}
