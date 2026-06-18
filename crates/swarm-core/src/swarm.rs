//! The `Swarm`: shared configuration and the factory/coordinator for agents.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use swarm_llm::LlmProvider;
use tracing::info;

use crate::agent::{worker_system_prompt, Agent, ORCHESTRATOR_PROMPT};
use crate::coordinator::Coordinator;
use crate::tool::{SubAgentSpawner, ToolContext, ToolRegistry};

/// Holds everything agents need (provider, model, tools, coordinator) and acts
/// as the factory that spawns workers.
///
/// A single level of delegation is supported: the lead orchestrator may
/// `spawn_agent`, but workers run with `spawner = None`. All agents share one
/// [`Coordinator`], so their staged changes, locks and events are coordinated.
pub struct Swarm {
    provider: Arc<dyn LlmProvider>,
    model: String,
    tools: Arc<ToolRegistry>,
    workspace: PathBuf,
    coordinator: Arc<Coordinator>,
}

impl Swarm {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        model: impl Into<String>,
        tools: ToolRegistry,
        workspace: PathBuf,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            tools: Arc::new(tools),
            coordinator: Coordinator::new(workspace.clone()),
            workspace,
        }
    }

    pub fn workspace(&self) -> &PathBuf {
        &self.workspace
    }

    pub fn coordinator(&self) -> &Arc<Coordinator> {
        &self.coordinator
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Build the lead orchestrator agent.
    pub fn lead_agent(&self) -> Agent {
        Agent::new(
            "orchestrator",
            self.provider.clone(),
            self.model.clone(),
            self.tools.clone(),
            ORCHESTRATOR_PROMPT,
        )
    }

    /// Tool context for the lead agent, wired so `spawn_agent` can delegate.
    pub fn lead_context(self: &Arc<Self>) -> ToolContext {
        ToolContext::for_agent(
            "orchestrator",
            self.coordinator.clone(),
            Some(self.clone()),
            0,
        )
    }

    /// Run a worker agent to completion on `task`.
    ///
    /// Workers get `spawner = None` (depth 1 limit), keeping delegation a
    /// single level deep and avoiding runaway recursion.
    pub async fn run_subagent(&self, role: &str, task: &str) -> anyhow::Result<String> {
        info!(role, "spawning worker agent");
        let mut agent = Agent::new(
            format!("worker:{role}"),
            self.provider.clone(),
            self.model.clone(),
            self.tools.clone(),
            worker_system_prompt(role),
        );
        agent.push_user(task);

        let ctx = ToolContext::for_agent(
            format!("worker:{role}"),
            self.coordinator.clone(),
            None,
            1,
        );
        agent.run(&ctx).await
    }
}

#[async_trait]
impl SubAgentSpawner for Swarm {
    async fn spawn(&self, role: &str, task: &str) -> anyhow::Result<String> {
        self.run_subagent(role, task).await
    }
}
