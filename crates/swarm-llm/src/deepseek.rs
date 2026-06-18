//! DeepSeek provider (OpenAI-compatible API).
//!
//! See <https://api-docs.deepseek.com/>. The endpoint is
//! `https://api.deepseek.com/chat/completions` and supports function/tool
//! calling. Models: `deepseek-chat` and `deepseek-reasoner`.

use async_trait::async_trait;
use futures::StreamExt;
use serde::Deserialize;

use crate::provider::{DeltaSink, LlmError, LlmProvider, Result};
use crate::types::{ChatRequest, ChatResponse, FunctionCall, Message, Role, ToolCall};

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

    async fn chat_stream(&self, mut request: ChatRequest, sink: DeltaSink) -> Result<Message> {
        request.stream = Some(true);
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

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut content = String::new();
        let mut tool_calls: Vec<PartialToolCall> = Vec::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buf.push_str(&String::from_utf8_lossy(&bytes));

            // Process complete SSE lines.
            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].trim().to_string();
                buf.drain(..=nl);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let parsed: StreamChunk = match serde_json::from_str(data) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if let Some(choice) = parsed.choices.into_iter().next() {
                    if let Some(text) = choice.delta.content {
                        if !text.is_empty() {
                            content.push_str(&text);
                            let _ = sink.send(text);
                        }
                    }
                    for delta in choice.delta.tool_calls.unwrap_or_default() {
                        accumulate_tool_call(&mut tool_calls, delta);
                    }
                }
            }
        }

        let calls: Vec<ToolCall> = tool_calls.into_iter().map(PartialToolCall::finish).collect();
        Ok(Message {
            role: Role::Assistant,
            content: if content.is_empty() && !calls.is_empty() {
                None
            } else {
                Some(content)
            },
            tool_calls: if calls.is_empty() { None } else { Some(calls) },
            tool_call_id: None,
            name: None,
        })
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    fn name(&self) -> &str {
        "deepseek"
    }
}

// ---- Streaming wire types -------------------------------------------------

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Deserialize)]
struct DeltaToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Deserialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl PartialToolCall {
    fn finish(self) -> ToolCall {
        ToolCall {
            id: self.id,
            kind: "function".to_string(),
            function: FunctionCall {
                name: self.name,
                arguments: self.arguments,
            },
        }
    }
}

/// Merge an incremental tool-call delta into the accumulator (keyed by index).
fn accumulate_tool_call(acc: &mut Vec<PartialToolCall>, delta: DeltaToolCall) {
    if delta.index >= acc.len() {
        acc.resize_with(delta.index + 1, PartialToolCall::default);
    }
    let slot = &mut acc[delta.index];
    if let Some(id) = delta.id {
        slot.id = id;
    }
    if let Some(func) = delta.function {
        if let Some(name) = func.name {
            slot.name = name;
        }
        if let Some(args) = func.arguments {
            slot.arguments.push_str(&args);
        }
    }
}
