//! DeepSeek provider (OpenAI-compatible API).
//!
//! See <https://api-docs.deepseek.com/>. The endpoint is
//! `https://api.deepseek.com/chat/completions` and supports function/tool
//! calling. Models: `deepseek-chat` and `deepseek-reasoner`.

use async_trait::async_trait;

use crate::provider::{LlmError, LlmProvider, Result};
use crate::types::{ChatRequest, ChatResponse, Message};

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
pub const MODEL_CHAT: &str = "deepseek-chat";
pub const MODEL_REASONER: &str = "deepseek-reasoner";

#[derive(Clone)]
pub struct DeepSeekProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl DeepSeekProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: MODEL_CHAT.to_string(),
        }
    }

    /// Build from environment: `DEEPSEEK_API_KEY`, optional `DEEPSEEK_BASE_URL`
    /// and `DEEPSEEK_MODEL`.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("DEEPSEEK_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        let mut p = Self::new(api_key);
        if let Ok(base) = std::env::var("DEEPSEEK_BASE_URL") {
            p.base_url = base.trim_end_matches('/').to_string();
        }
        if let Ok(model) = std::env::var("DEEPSEEK_MODEL") {
            p.model = model;
        }
        Ok(p)
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }
}

#[async_trait]
impl LlmProvider for DeepSeekProvider {
    async fn chat(&self, request: ChatRequest) -> Result<Message> {
        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Status {
                status: status.as_u16(),
                body,
            });
        }

        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| LlmError::Decode(e.to_string()))?;

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| LlmError::Decode("no choices returned".to_string()))
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    fn name(&self) -> &str {
        "deepseek"
    }
}
