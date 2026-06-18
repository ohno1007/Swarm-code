//! An agent: a conversation loop over an LLM with tool-calling.

use std::sync::Arc;

use swarm_llm::{ChatRequest, LlmProvider, Message};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, info, warn};

use crate::memory::WorkingMemory;
use crate::tool::{ToolContext, ToolRegistry};

/// Live feedback emitted while an agent runs, for the REPL/UI to render.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A chunk of assistant text (streamed).
    Text(String),
    /// A tool call is about to run.
    ToolStart { name: String, args: String },
    /// A tool call finished.
    ToolEnd { name: String, ok: bool, preview: String },
    /// Working memory was compacted (N messages summarized).
    Compacted { summarized: usize },
}

/// Channel an agent emits [`AgentEvent`]s on.
pub type AgentObserver = UnboundedSender<AgentEvent>;

/// A single conversational agent that can call tools in a loop until it
/// produces a final text answer.
pub struct Agent {
    pub name: String,
    provider: Arc<dyn LlmProvider>,
    model: String,
    tools: Arc<ToolRegistry>,
    memory: WorkingMemory,
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
            memory: WorkingMemory::new(system_prompt),
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
        self.memory.push(Message::user(content));
    }

    pub fn history(&self) -> &[Message] {
        self.memory.messages()
    }

    /// Drive the agent until it returns a final answer or hits `max_steps`.
    pub async fn run(&mut self, ctx: &ToolContext) -> anyhow::Result<String> {
        self.run_observed(ctx, None).await
    }

    /// Like [`Agent::run`], but emits [`AgentEvent`]s to `observer` for live
    /// streaming and tool/command feedback (used by the interactive REPL).
    pub async fn run_observed(
        &mut self,
        ctx: &ToolContext,
        observer: Option<AgentObserver>,
    ) -> anyhow::Result<String> {
        for step in 0..self.max_steps {
            // Compact working memory before the call if it's grown too large.
            let summarized = self
                .memory
                .compact_if_needed(&*self.provider, &self.model)
                .await;
            if summarized > 0 {
                if let Some(obs) = &observer {
                    let _ = obs.send(AgentEvent::Compacted { summarized });
                }
            }

            let request = ChatRequest::new(self.model.clone(), self.memory.messages().to_vec())
                .with_tools(self.tools.specs())
                .with_temperature(self.temperature);

            let reply = self.call_model(request, observer.as_ref()).await?;
            self.memory.push(reply.clone());

            let tool_calls = reply.tool_calls.unwrap_or_default();
            if tool_calls.is_empty() {
                let answer = reply.content.unwrap_or_default();
                debug!(agent = %self.name, step, "final answer");
                return Ok(answer);
            }

            info!(agent = %self.name, step, calls = tool_calls.len(), "executing tools");
            for call in tool_calls {
                let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or(serde_json::Value::Null);

                if let Some(obs) = &observer {
                    let _ = obs.send(AgentEvent::ToolStart {
                        name: call.function.name.clone(),
                        args: truncate(&call.function.arguments, 160),
                    });
                }

                let outcome = self.tools.execute(&call.function.name, args, ctx).await;
                let (ok, result) = match outcome {
                    Ok(out) => (true, out),
                    Err(e) => {
                        warn!(agent = %self.name, tool = %call.function.name, "tool error: {e}");
                        (false, format!("Error: {e}"))
                    }
                };

                if let Some(obs) = &observer {
                    let _ = obs.send(AgentEvent::ToolEnd {
                        name: call.function.name.clone(),
                        ok,
                        preview: preview(&result),
                    });
                }

                self.memory.push(Message::tool_result(
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

    /// Call the model, streaming text to `observer` when present.
    async fn call_model(
        &self,
        request: ChatRequest,
        observer: Option<&AgentObserver>,
    ) -> anyhow::Result<Message> {
        match observer {
            None => Ok(self.provider.chat(request).await?),
            Some(obs) => {
                // Bridge the provider's text-delta sink to AgentEvent::Text.
                let (dtx, mut drx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let obs = obs.clone();
                let forward = tokio::spawn(async move {
                    while let Some(text) = drx.recv().await {
                        let _ = obs.send(AgentEvent::Text(text));
                    }
                });
                let reply = self.provider.chat_stream(request, dtx).await;
                let _ = forward.await;
                Ok(reply?)
            }
        }
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

/// One-line preview of a tool result for feedback display.
fn preview(s: &str) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    let line = if line.is_empty() { s.trim() } else { line };
    truncate(line, 120)
}

/// Convenience: the role label used in worker system prompts.
pub fn worker_system_prompt(role: &str) -> String {
    format!(
        "You are a '{role}' worker agent in the Swarm-code multi-agent system. \
         You share a change buffer and symbol locks with the other agents. \
         {WORKFLOW}\n\n\
         Complete the assigned task autonomously and return a concise, \
         self-contained result. Do not ask follow-up questions."
    )
}

const WORKFLOW: &str = "\
Editing workflow (git-like):\n\
- Writes do NOT touch disk directly. write_file and edit_symbol stage changes \
into a shared change buffer.\n\
- edit_symbol replaces one named symbol and takes a symbol-level lock, so other \
agents can edit other symbols in the same file concurrently. Prefer it for \
targeted edits; it fails if another agent holds the lock.\n\
- view_changes shows the staged diff. commit_changes applies the buffer to \
disk, releases your locks, and runs a compile check (cargo check). If it fails, \
fix and commit again. discard_changes throws staged work away.\n\
- run_command gives you a terminal (builds, tests, git, grep).";

/// The lead/orchestrator system prompt.
pub const ORCHESTRATOR_PROMPT: &str = "\
You are Swarm-code, an AI coding orchestrator. You coordinate a swarm of agents \
to understand and modify a codebase efficiently.\n\n\
Capabilities:\n\
- read_file / list_dir: inspect the workspace.\n\
- search / find_files: grep file contents by regex, or locate files by name.\n\
- analyze_code: tree-sitter outline of a file's symbols and scopes. Prefer it \
over reading whole files when you only need structure.\n\
- write_file / edit_symbol: stage edits into the shared change buffer.\n\
- view_changes / commit_changes / discard_changes / list_locks: manage the \
git-like change workflow.\n\
- run_command / cargo_check: terminal access and compile validation.\n\
- remember / recall / forget: long-term memory that persists across sessions. \
Save durable project facts (conventions, where things live, decisions) and \
recall them later.\n\
- list_skills / use_skill / create_skill: reusable instruction packs. Load one \
with use_skill, or teach yourself a repeatable procedure with create_skill.\n\
- mcp_add_server / mcp_list_servers / mcp_list_tools / mcp_call: connect external \
Model Context Protocol servers to extend your own toolset at runtime, then call \
their tools.\n\
- spawn_agent / spawn_agents: delegate subtasks to worker agents in their own \
context. spawn_agents runs MULTIPLE workers in parallel — prefer it to fan out \
independent work. Workers share your change buffer, symbol locks and memory.\n\n\
Use analyze_code to build a mental model quickly. Make targeted edits with \
edit_symbol, review with view_changes, then commit_changes to validate. When a \
task has separable parts, delegate them. Record durable insights with remember. \
Finish with a clear summary.";
