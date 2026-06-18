//! Tool abstraction: typed capabilities the model can invoke.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use swarm_llm::ToolSpec;

use crate::coordinator::Coordinator;

/// Context handed to every tool invocation.
#[derive(Clone)]
pub struct ToolContext {
    /// Root directory the agent is allowed to operate within.
    pub workspace: PathBuf,
    /// Identity of the acting agent (used for authoring changes and locks).
    pub agent: String,
    /// Shared coordination services (buffer, locks, events, validator).
    pub coordinator: Arc<Coordinator>,
    /// Hook for spawning sub-agents. `None` inside a sub-agent (depth limit).
    pub spawner: Option<Arc<dyn SubAgentSpawner>>,
    /// Current sub-agent nesting depth (0 = lead agent).
    pub depth: usize,
}

impl ToolContext {
    pub fn for_agent(
        agent: impl Into<String>,
        coordinator: Arc<Coordinator>,
        spawner: Option<Arc<dyn SubAgentSpawner>>,
        depth: usize,
    ) -> Self {
        Self {
            workspace: coordinator.workspace.clone(),
            agent: agent.into(),
            coordinator,
            spawner,
            depth,
        }
    }
}

/// Implemented by the orchestrator so the `spawn_agent` tool can delegate work.
#[async_trait]
pub trait SubAgentSpawner: Send + Sync {
    /// Run a worker agent with `role` to completion on `task`, returning its
    /// final answer.
    async fn spawn(&self, role: &str, task: &str) -> anyhow::Result<String>;
}

/// A capability the model can call.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON schema for the tool's arguments.
    fn parameters(&self) -> serde_json::Value;

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<String>;

    fn spec(&self) -> ToolSpec {
        ToolSpec::function(self.name(), self.description(), self.parameters())
    }
}

/// Collection of tools available to an agent.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<String> {
        match self.tools.iter().find(|t| t.name() == name) {
            Some(tool) => tool.execute(args, ctx).await,
            None => anyhow::bail!("unknown tool: {name}"),
        }
    }
}
