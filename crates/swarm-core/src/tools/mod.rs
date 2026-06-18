//! Built-in tools available to agents.

pub mod analyze;
pub mod coord;
pub mod exec;
pub mod fs;
pub mod memory;
pub mod search;
pub mod spawn;

use std::sync::Arc;

use crate::tool::ToolRegistry;

/// Registry containing all built-in tools.
pub fn default_registry() -> ToolRegistry {
    ToolRegistry::new()
        // Filesystem (staged through the change buffer).
        .with(Arc::new(fs::ReadFile))
        .with(Arc::new(fs::WriteFile))
        .with(Arc::new(fs::ListDir))
        // Code understanding & search.
        .with(Arc::new(analyze::AnalyzeCode))
        .with(Arc::new(search::Search))
        .with(Arc::new(search::FindFiles))
        // Git-like coordination.
        .with(Arc::new(coord::EditSymbol))
        .with(Arc::new(coord::ViewChanges))
        .with(Arc::new(coord::CommitChanges))
        .with(Arc::new(coord::DiscardChanges))
        .with(Arc::new(coord::ListLocks))
        // Terminal + validation.
        .with(Arc::new(exec::RunCommand))
        .with(Arc::new(exec::CargoCheckTool))
        // Long-term memory.
        .with(Arc::new(memory::Remember))
        .with(Arc::new(memory::Recall))
        .with(Arc::new(memory::Forget))
        // Multi-agent delegation.
        .with(Arc::new(spawn::SpawnAgent))
        .with(Arc::new(spawn::SpawnAgents))
}
