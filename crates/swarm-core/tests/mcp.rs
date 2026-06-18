//! End-to-end MCP client test against a mock stdio server (requires python3).

use std::collections::HashMap;

use swarm_core::mcp::{McpClient, McpServerConfig};

fn python3_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn connects_handshakes_and_calls_tool() {
    if !python3_available() {
        eprintln!("skipping: python3 not available");
        return;
    }
    let script = format!("{}/tests/fixtures/mock_mcp_server.py", env!("CARGO_MANIFEST_DIR"));
    let config = McpServerConfig {
        command: "python3".to_string(),
        args: vec![script],
        env: HashMap::new(),
    };

    let client = McpClient::connect(&config).await.expect("connect");
    assert!(
        client.tools.iter().any(|t| t.name == "echo"),
        "tools: {:?}",
        client.tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    let out = client
        .call_tool("echo", serde_json::json!({ "text": "hello mcp" }))
        .await
        .expect("call");
    assert!(out.contains("echo: hello mcp"), "{out}");
}
