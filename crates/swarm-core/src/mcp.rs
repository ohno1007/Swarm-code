//! Minimal Model Context Protocol (MCP) client over stdio.
//!
//! Lets the swarm connect to external MCP servers and call their tools. The
//! transport is newline-delimited JSON-RPC 2.0 over the server's stdin/stdout
//! (the MCP stdio transport). The agent drives this entirely through tools
//! (`mcp_add_server`, `mcp_list_tools`, `mcp_call`), so servers can be
//! configured and used at runtime without a restart.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex as AsyncMutex};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How to launch an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// A tool advertised by an MCP server.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
}

/// On-disk config file shape: `{ "mcpServers": { name: {...} } }`.
#[derive(Default, Serialize, Deserialize)]
struct McpConfigFile {
    #[serde(default, rename = "mcpServers")]
    servers: HashMap<String, McpServerConfig>,
}

/// Manages MCP server configs and live connections (shared across agents).
pub struct McpManager {
    config_path: std::path::PathBuf,
    configs: Mutex<HashMap<String, McpServerConfig>>,
    clients: AsyncMutex<HashMap<String, Arc<McpClient>>>,
}

impl McpManager {
    /// Load configs from `<workspace>/.swarm/mcp.json` (does not connect yet).
    pub fn new(workspace: &std::path::Path) -> Self {
        let config_path = workspace.join(".swarm").join("mcp.json");
        let configs = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|t| serde_json::from_str::<McpConfigFile>(&t).ok())
            .map(|c| c.servers)
            .unwrap_or_default();
        Self {
            config_path,
            configs: Mutex::new(configs),
            clients: AsyncMutex::new(HashMap::new()),
        }
    }

    fn persist(&self) {
        if let Some(parent) = self.config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = McpConfigFile {
            servers: self.configs.lock().unwrap().clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&file) {
            let _ = std::fs::write(&self.config_path, json);
        }
    }

    /// Add (or replace) a server, persist it, and connect immediately.
    pub async fn add_server(
        &self,
        name: &str,
        config: McpServerConfig,
    ) -> anyhow::Result<Vec<McpToolDef>> {
        self.configs
            .lock()
            .unwrap()
            .insert(name.to_string(), config.clone());
        self.persist();
        let client = self.connect(name, &config).await?;
        Ok(client.tools.clone())
    }

    /// Get a connected client, connecting on demand from stored config.
    pub async fn ensure(&self, name: &str) -> anyhow::Result<Arc<McpClient>> {
        if let Some(c) = self.clients.lock().await.get(name) {
            return Ok(c.clone());
        }
        let config = self
            .configs
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no MCP server named '{name}' (add it first)"))?;
        self.connect(name, &config).await
    }

    async fn connect(&self, name: &str, config: &McpServerConfig) -> anyhow::Result<Arc<McpClient>> {
        let client = Arc::new(McpClient::connect(config).await?);
        self.clients
            .lock()
            .await
            .insert(name.to_string(), client.clone());
        Ok(client)
    }

    /// `(name, connected?, configured)` for all known servers.
    pub async fn list_servers(&self) -> Vec<(String, bool)> {
        let configured: Vec<String> = self.configs.lock().unwrap().keys().cloned().collect();
        let clients = self.clients.lock().await;
        let mut out: Vec<(String, bool)> = configured
            .into_iter()
            .map(|n| {
                let connected = clients.contains_key(&n);
                (n, connected)
            })
            .collect();
        out.sort();
        out
    }

    pub async fn list_tools(&self, name: &str) -> anyhow::Result<Vec<McpToolDef>> {
        Ok(self.ensure(name).await?.tools.clone())
    }

    pub async fn call(&self, server: &str, tool: &str, args: Value) -> anyhow::Result<String> {
        self.ensure(server).await?.call_tool(tool, args).await
    }
}

/// A single connected MCP server.
pub struct McpClient {
    inner: Arc<Inner>,
    pub tools: Vec<McpToolDef>,
    _child: Child,
}

struct Inner {
    stdin: AsyncMutex<ChildStdin>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    next_id: AtomicU64,
}

impl McpClient {
    /// Spawn the server, perform the MCP handshake and fetch its tool list.
    pub async fn connect(config: &McpServerConfig) -> anyhow::Result<Self> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to launch '{}': {e}", config.command))?;

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");

        let inner = Arc::new(Inner {
            stdin: AsyncMutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });

        // Reader task: match responses to pending requests by id.
        {
            let inner = inner.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(value) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                        if let Some(tx) = inner.pending.lock().unwrap().remove(&id) {
                            let _ = tx.send(value);
                        }
                    }
                    // Notifications (no id) are ignored.
                }
            });
        }

        let client_inner = inner.clone();

        // initialize handshake
        client_inner
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "swarm-code", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        client_inner.notify("notifications/initialized", json!({})).await?;

        // tools/list
        let listed = client_inner.request("tools/list", json!({})).await?;
        let tools = listed
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|t| McpToolDef {
                        name: t.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        description: t
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            inner,
            tools,
            _child: child,
        })
    }

    /// Call a tool and return its concatenated text content.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> anyhow::Result<String> {
        let resp = self
            .inner
            .request("tools/call", json!({ "name": name, "arguments": arguments }))
            .await?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!("mcp error: {err}");
        }
        let content = resp
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = String::new();
        for part in content {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                out.push_str(text);
                out.push('\n');
            }
        }
        if out.is_empty() {
            out = resp.get("result").map(|r| r.to_string()).unwrap_or_default();
        }
        Ok(out.trim_end().to_string())
    }
}

impl Inner {
    async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_line(&msg).await?;

        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => anyhow::bail!("mcp connection closed during '{method}'"),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                anyhow::bail!("mcp request '{method}' timed out")
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_line(&msg).await
    }

    async fn write_line(&self, msg: &Value) -> anyhow::Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }
}
