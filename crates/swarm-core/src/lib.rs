//! Core of Swarm-code: agents, tools, multi-agent coordination and concurrent
//! sessions.
//!
//! Layering:
//! - [`tool`] / [`tools`]: capabilities the model can call (fs, code analysis,
//!   sub-agent spawning).
//! - [`agent::Agent`]: a single tool-calling conversation loop.
//! - [`swarm::Swarm`]: shared config + coordinator that spawns worker agents.
//! - [`session::SessionManager`]: many concurrent sessions.

pub mod agent;
pub mod config;
pub mod session;
pub mod swarm;
pub mod tool;
pub mod tools;

use std::path::PathBuf;
use std::sync::Arc;

pub use agent::Agent;
pub use config::Config;
pub use session::{Session, SessionManager};
pub use swarm::Swarm;
pub use tool::{SubAgentSpawner, Tool, ToolContext, ToolRegistry};

/// Build a [`SessionManager`] wired up with a DeepSeek provider and the default
/// toolset, rooted at `workspace`.
pub fn build_manager(workspace: PathBuf) -> anyhow::Result<SessionManager> {
    let config = Config::new(workspace.clone());
    let provider = config.provider()?;
    let swarm = Swarm::new(
        Arc::new(provider),
        config.model.clone(),
        tools::default_registry(),
        workspace,
    );
    Ok(SessionManager::new(Arc::new(swarm)))
}
