//! Runtime configuration.

use std::path::PathBuf;

use swarm_llm::deepseek::{DeepSeekProvider, MODEL_CHAT};

/// Configuration assembled from the environment and CLI flags.
#[derive(Debug, Clone)]
pub struct Config {
    /// Workspace root the agents operate within.
    pub workspace: PathBuf,
    /// Model id (e.g. `deepseek-chat`).
    pub model: String,
}

impl Config {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            model: std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| MODEL_CHAT.to_string()),
        }
    }

    /// Build a DeepSeek provider from the environment (`DEEPSEEK_API_KEY`, etc.).
    pub fn provider(&self) -> swarm_llm::Result<DeepSeekProvider> {
        Ok(DeepSeekProvider::from_env()?.with_model(self.model.clone()))
    }
}
