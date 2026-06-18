use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

use crate::types::{ChatRequest, Message};

/// Sink for streamed text deltas.
pub type DeltaSink = UnboundedSender<String>;

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

    /// Streaming variant: forwards text deltas to `sink` as they arrive and
    /// returns the fully assembled assistant message (including any tool calls).
    ///
    /// The default implementation falls back to a single non-streamed call.
    async fn chat_stream(&self, request: ChatRequest, sink: DeltaSink) -> Result<Message> {
        let message = self.chat(request).await?;
        if let Some(content) = &message.content {
            let _ = sink.send(content.clone());
        }
        Ok(message)
    }

    /// The default model id for this provider.
    fn default_model(&self) -> &str;

    /// Human-readable provider name for logging.
    fn name(&self) -> &str;
}
