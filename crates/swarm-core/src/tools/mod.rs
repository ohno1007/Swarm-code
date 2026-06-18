//! Built-in tools available to agents.

pub mod analyze;
pub mod fs;
pub mod spawn;

use std::sync::Arc;

use crate::tool::ToolRegistry;

/// Registry containing all built-in tools.
pub fn default_registry() -> ToolRegistry {
    ToolRegistry::new()
        .with(Arc::new(fs::ReadFile))
        .with(Arc::new(fs::WriteFile))
        .with(Arc::new(fs::ListDir))
        .with(Arc::new(analyze::AnalyzeCode))
        .with(Arc::new(spawn::SpawnAgent))
}
