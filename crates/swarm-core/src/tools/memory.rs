//! Long-term memory tools backed by the workspace [`MemoryStore`].

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext};

/// Store a durable fact in workspace memory (survives across sessions).
pub struct Remember;

#[async_trait]
impl Tool for Remember {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "Save a durable fact to long-term workspace memory (persisted to \
         .swarm/memory.json). Use for project conventions, architecture notes, \
         where things live, or decisions worth recalling in future sessions."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The fact/note to remember." },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional keywords to aid later recall."
                }
            },
            "required": ["text"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let text = args["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: text"))?;
        let tags = args["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let id = ctx.coordinator.memory.remember(text, tags, &ctx.agent);
        Ok(format!("remembered ({id})"))
    }
}

/// Recall facts from workspace memory.
pub struct Recall;

#[async_trait]
impl Tool for Recall {
    fn name(&self) -> &str {
        "recall"
    }
    fn description(&self) -> &str {
        "Search long-term workspace memory. With a query, returns the most \
         relevant notes; without one, returns the most recent."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Optional keywords to search for." },
                "limit": { "type": "integer", "description": "Max results (default 8)." }
            }
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let query = args["query"].as_str();
        let limit = args["limit"].as_u64().unwrap_or(8) as usize;
        let entries = ctx.coordinator.memory.recall(query, limit);
        if entries.is_empty() {
            return Ok("no matching memories".to_string());
        }
        Ok(entries
            .into_iter()
            .map(|e| {
                let tags = if e.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", e.tags.join(","))
                };
                format!("({}){tags} {}", e.id, e.text)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Forget a memory by id.
pub struct Forget;

#[async_trait]
impl Tool for Forget {
    fn name(&self) -> &str {
        "forget"
    }
    fn description(&self) -> &str {
        "Delete a long-term memory entry by its id."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "The memory id to delete." } },
            "required": ["id"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let id = args["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: id"))?;
        if ctx.coordinator.memory.forget(id) {
            Ok(format!("forgot {id}"))
        } else {
            Ok(format!("no memory with id {id}"))
        }
    }
}
