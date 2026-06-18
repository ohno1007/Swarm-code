use async_trait::async_trait;

use crate::types::{ChatRequest, Message};

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider returned status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("invalid response: {0}")]
    Decode(String),
    #[error("missing api key (set DEEPSEEK_API_KEY)")]
    MissingApiKey,
}

pub type Result<T> = std::result::Result<T, LlmError>;

/// Abstraction over a chat-completion LLM backend.
///
/// Implementations are cheap to clone (wrap a shared `reqwest::Client`) and are
/// shared across agents via `Arc<dyn LlmProvider>`.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Run a single chat completion and return the assistant message.
    async fn chat(&self, request: ChatRequest) -> Result<Message>;

    /// The default model id for this provider.
    fn default_model(&self) -> &str;

    /// Human-readable provider name for logging.
    fn name(&self) -> &str;
}
