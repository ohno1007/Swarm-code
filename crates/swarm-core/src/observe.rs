//! Live agent observation events.
//!
//! Agents emit [`AgentMsg`] (an [`AgentEvent`] tagged with the agent's name and
//! nesting depth) so the UI can stream output — including from concurrently
//! running sub-agents — and label/group it per agent.

use tokio::sync::mpsc::UnboundedSender;

/// A single piece of live agent activity.
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

/// An [`AgentEvent`] tagged with its source agent.
#[derive(Debug, Clone)]
pub struct AgentMsg {
    /// Agent name, e.g. `orchestrator` or `worker:reviewer`.
    pub agent: String,
    /// Sub-agent nesting depth (0 = lead).
    pub depth: usize,
    pub event: AgentEvent,
}

/// Channel agents emit [`AgentMsg`]s on.
pub type AgentObserver = UnboundedSender<AgentMsg>;
