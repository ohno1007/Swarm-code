//! Code-understanding tool backed by `swarm-analyzer` (tree-sitter).

use async_trait::async_trait;
use serde_json::{json, Value};
use swarm_analyzer::Analyzer;

use crate::tool::{Tool, ToolContext};

/// Produce a structural outline (symbols + scopes) of a source file so the
/// agent can understand it without reading every line.
pub struct AnalyzeCode;

#[async_trait]
impl Tool for AnalyzeCode {
    fn name(&self) -> &str {
        "analyze_code"
    }

    fn description(&self) -> &str {
        "Parse a source file with tree-sitter and return an outline of its \
         symbols (functions, types, modules) and lexical scopes. Supports \
         Rust, Python, JavaScript and Go. Use this to quickly understand a \
         file's structure before reading it in full."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Source file path relative to the workspace root."
                },
                "format": {
                    "type": "string",
                    "enum": ["outline", "json"],
                    "description": "outline = indented text (default); json = full symbol tree."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let rel = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: path"))?;
        let format = args["format"].as_str().unwrap_or("outline");

        let path = ctx.workspace.join(rel);
        let analyzer = Analyzer::new();
        let root = analyzer
            .analyze_path(&path)
            .map_err(|e| anyhow::anyhow!("analyze failed: {e}"))?;

        match format {
            "json" => Ok(serde_json::to_string_pretty(&root)?),
            _ => Ok(swarm_analyzer::render_outline(&root)),
        }
    }
}
