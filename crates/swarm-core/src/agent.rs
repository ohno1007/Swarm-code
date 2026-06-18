//! An agent: a conversation loop over an LLM with tool-calling.

use std::sync::Arc;

use swarm_llm::{ChatRequest, LlmProvider, Message};
use tracing::{debug, info, warn};

use crate::tool::{ToolContext, ToolRegistry};

/// A single conversational agent that can call tools in a loop until it
/// produces a final text answer.
pub struct Agent {
    pub name: String,
    provider: Arc<dyn LlmProvider>,
    model: String,
    tools: Arc<ToolRegistry>,
    messages: Vec<Message>,
    /// Max LLM round-trips before giving up.
    max_steps: usize,
    temperature: f32,
}

impl Agent {
    pub fn new(
        name: impl Into<String>,
        provider: Arc<dyn LlmProvider>,
        model: impl Into<String>,
        tools: Arc<ToolRegistry>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            provider,
            model: model.into(),
            tools,
            messages: vec![Message::system(system_prompt)],
            max_steps: 16,
            temperature: 0.2,
        }
    }

    pub fn with_max_steps(mut self, steps: usize) -> Self {
        self.max_steps = steps;
        self
    }

    /// Append a user message to the conversation.
    pub fn push_user(&mut self, content: impl Into<String>) {
        self.messages.push(Message::user(content));
    }

    pub fn history(&self) -> &[Message] {
        &self.messages
    }

    /// Drive the agent until it returns a final answer or hits `max_steps`.
    pub async fn run(&mut self, ctx: &ToolContext) -> anyhow::Result<String> {
        for step in 0..self.max_steps {
            let request = ChatRequest::new(self.model.clone(), self.messages.clone())
                .with_tools(self.tools.specs())
                .with_temperature(self.temperature);

            let reply = self.provider.chat(request).await?;
            self.messages.push(reply.clone());

            let tool_calls = reply.tool_calls.unwrap_or_default();
            if tool_calls.is_empty() {
                let answer = reply.content.unwrap_or_default();
                debug!(agent = %self.name, step, "final answer");
                return Ok(answer);
            }

            info!(agent = %self.name, step, calls = tool_calls.len(), "executing tools");
            for call in tool_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&call.function.arguments).unwrap_or(serde_json::Value::Null);
                let result = match self
                    .tools
                    .execute(&call.function.name, args, ctx)
                    .await
                {
                    Ok(out) => out,
                    Err(e) => {
                        warn!(agent = %self.name, tool = %call.function.name, "tool error: {e}");
                        format!("Error: {e}")
                    }
                };
                self.messages.push(Message::tool_result(
                    call.id,
                    call.function.name,
                    truncate(&result, 24_000),
                ));
            }
        }

        anyhow::bail!(
            "agent '{}' exceeded max steps ({})",
            self.name,
            self.max_steps
        )
    }
}

/// Keep tool output from blowing up the context window.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}\n…[truncated {} bytes]", &s[..max], s.len() - max)
    }
}

/// Convenience: the role label used in worker system prompts.
pub fn worker_system_prompt(role: &str) -> String {
    format!(
        "You are a '{role}' worker agent in the Swarm-code multi-agent system. \
         You have tools to read, write, list and analyze code within the \
         workspace. Complete the assigned task autonomously and return a \
         concise, self-contained result. Do not ask follow-up questions."
    )
}

/// The lead/orchestrator system prompt.
pub const ORCHESTRATOR_PROMPT: &str = "\
You are Swarm-code, an AI coding orchestrator. You coordinate a swarm of agents \
to understand and modify a codebase efficiently.\n\n\
Capabilities:\n\
- read_file / write_file / list_dir: inspect and edit the workspace.\n\
- analyze_code: get a tree-sitter outline of a file's symbols and scopes. \
Prefer this over reading whole files when you only need structure.\n\
- spawn_agent: delegate focused, independent subtasks to worker agents that run \
in their own context. Use it to parallelize work or keep your own context lean.\n\n\
Work step by step. Use analyze_code to build a mental model of the project \
quickly. When a task has separable parts, delegate them. Finish with a clear \
summary of what you did.";
