//! Backward-compatibility re-exports.
//!
//! The LLM provider implementation lives in `crate::provider::http_base`.
//! Types and helpers have moved to `crate::provider`.
//! This module re-exports them so existing code continues to work.

#[allow(unused_imports)]
pub use crate::provider::{
    system_message, tool_result_message, user_image_message, user_message,
    ChatMessage, FunctionCall, ToolCall,
};
#[allow(unused_imports)]
pub use crate::provider::http_base::HttpProvider as OpenAiClient;
