//! LLM provider abstraction for Swarm-code.
//!
//! Currently ships a DeepSeek backend. Add new providers by implementing
//! [`provider::LlmProvider`].

pub mod deepseek;
pub mod provider;
pub mod types;

pub use deepseek::DeepSeekProvider;
pub use provider::{DeltaSink, LlmError, LlmProvider, Result};
pub use types::{
    ChatRequest, ChatResponse, Choice, FunctionCall, FunctionSpec, Message, Role, ToolCall,
    ToolSpec, Usage,
};
