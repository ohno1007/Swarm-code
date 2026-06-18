//! Tools for self-configuring and using MCP servers.

use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::mcp::McpServerConfig;
use crate::tool::{Tool, ToolContext};

/// Add/connect an MCP server — the agent extending its own toolset.
pub struct McpAddServer;

#[async_trait]
impl Tool for McpAddServer {
    fn name(&self) -> &str {
        "mcp_add_server"
    }
    fn description(&self) -> &str {
        "Connect a Model Context Protocol (MCP) server to extend your tools. \
         Provide a launch command (and optional args/env). The server is started, \
         its tools are discovered, and the config is saved to .swarm/mcp.json so \
         it reconnects in future sessions. Then use mcp_list_tools and mcp_call."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Local name for the server." },
                "command": { "type": "string", "description": "Executable to launch (stdio MCP server)." },
                "args": { "type": "array", "items": { "type": "string" }, "description": "Command arguments." },
                "env": { "type": "object", "description": "Extra environment variables (string->string)." }
            },
            "required": ["name", "command"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let name = args["name"].as_str().ok_or_else(|| anyhow::anyhow!("missing name"))?;
        let command = args["command"].as_str().ok_or_else(|| anyhow::anyhow!("missing command"))?;
        let cmd_args: Vec<String> = args["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let env: HashMap<String, String> = args["env"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let config = McpServerConfig {
            command: command.to_string(),
            args: cmd_args,
            env,
        };
        let tools = ctx.coordinator.mcp.add_server(name, config).await?;
        let list = tools
            .iter()
            .map(|t| format!("  - {}: {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!(
            "connected MCP server '{name}' with {} tool(s):\n{list}",
            tools.len()
        ))
    }
}

/// List configured/connected MCP servers.
pub struct McpListServers;

#[async_trait]
impl Tool for McpListServers {
    fn name(&self) -> &str {
        "mcp_list_servers"
    }
    fn description(&self) -> &str {
        "List MCP servers known to this workspace and whether they're connected."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let servers = ctx.coordinator.mcp.list_servers().await;
        if servers.is_empty() {
            return Ok("no MCP servers configured (add one with mcp_add_server)".to_string());
        }
        Ok(servers
            .into_iter()
            .map(|(n, connected)| format!("- {n} [{}]", if connected { "connected" } else { "configured" }))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// List the tools a server exposes.
pub struct McpListTools;

#[async_trait]
impl Tool for McpListTools {
    fn name(&self) -> &str {
        "mcp_list_tools"
    }
    fn description(&self) -> &str {
        "List the tools exposed by a connected MCP server (connects on demand)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "server": { "type": "string", "description": "MCP server name." } },
            "required": ["server"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let server = args["server"].as_str().ok_or_else(|| anyhow::anyhow!("missing server"))?;
        let tools = ctx.coordinator.mcp.list_tools(server).await?;
        if tools.is_empty() {
            return Ok(format!("server '{server}' exposes no tools"));
        }
        Ok(tools
            .into_iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Call a tool on an MCP server.
pub struct McpCall;

#[async_trait]
impl Tool for McpCall {
    fn name(&self) -> &str {
        "mcp_call"
    }
    fn description(&self) -> &str {
        "Invoke a tool on a connected MCP server. Use mcp_list_tools to discover \
         tool names and their expected arguments."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": { "type": "string", "description": "MCP server name." },
                "tool": { "type": "string", "description": "Tool name on that server." },
                "arguments": { "type": "object", "description": "Arguments object for the tool." }
            },
            "required": ["server", "tool"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let server = args["server"].as_str().ok_or_else(|| anyhow::anyhow!("missing server"))?;
        let tool = args["tool"].as_str().ok_or_else(|| anyhow::anyhow!("missing tool"))?;
        let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
        ctx.coordinator.mcp.call(server, tool, arguments).await
    }
}
