#![allow(dead_code)]

pub mod anthropic;
pub mod custom;
pub mod http_base;
pub mod openai;

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Trait for LLM providers that can handle chat completions with optional tool use.
///
/// Uses a boxed future return type to support dynamic dispatch (`Box<dyn LlmProvider>`).
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request with messages and optional tool definitions.
    ///
    /// Returns the assistant's response message in the common ChatMessage format.
    fn chat_completion<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        tools: &'a [serde_json::Value],
    ) -> Pin<Box<dyn Future<Output = Result<ChatMessage, AppError>> + Send + 'a>>;
}

// ---------- Common message types ----------

/// A message in the OpenAI chat completions format.
/// Used as the common format across all providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
    /// Gemini thought signatures - must be preserved for multi-turn function calling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_content: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

// ---------- Message helpers ----------

/// Helper to create a system message.
pub fn system_message(content: &str) -> ChatMessage {
    ChatMessage {
        role: "system".into(),
        content: Some(serde_json::Value::String(content.into())),
        tool_calls: None,
        tool_call_id: None,
    }
}

/// Helper to create a user message with text content.
pub fn user_message(content: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: Some(serde_json::Value::String(content.into())),
        tool_calls: None,
        tool_call_id: None,
    }
}

/// Helper to create a user message containing an image (base64 data URL).
pub fn user_image_message(data_url: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: Some(serde_json::json!([
            {
                "type": "image_url",
                "image_url": { "url": data_url }
            }
        ])),
        tool_calls: None,
        tool_call_id: None,
    }
}

// ---------- Provider factory ----------

/// Create an LlmProvider from configuration fields.
///
/// Provider selection:
/// - "anthropic" (default): Anthropic Messages API (Claude models)
/// - "openai": OpenAI API
/// - "openrouter": OpenRouter (OpenAI-compatible, auto-sets base URL)
/// - "cerebras": Cerebras (OpenAI-compatible, auto-sets base URL)
/// - "gemini": Google Gemini (OpenAI-compatible, auto-sets base URL)
/// - "custom": OpenAI-compatible API with configurable base_url
///
/// API key resolution order:
/// 1. Explicit `api_key` parameter
/// 2. Provider-specific env var (OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.)
/// 3. Generic LLM_API_KEY env var
pub fn create_provider(
    provider_name: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
) -> Result<Box<dyn LlmProvider>, AppError> {
    let resolved_key = resolve_api_key(api_key, provider_name)?;

    match provider_name {
        "openai" => {
            let mut client = openai::OpenAiProvider::new(&resolved_key, model);
            if base_url != "https://api.openai.com" && base_url != "https://api.anthropic.com" {
                client = client.with_base_url(base_url);
            }
            Ok(Box::new(client))
        }
        "anthropic" => {
            validate_image_support(provider_name, model)?;
            let mut client = anthropic::AnthropicProvider::new(&resolved_key, model);
            if base_url != "https://api.openai.com" && base_url != "https://api.anthropic.com" {
                client = client.with_base_url(base_url);
            }
            Ok(Box::new(client))
        }
        "openrouter" => {
            let url = if base_url == "https://api.anthropic.com" || base_url == "https://api.openai.com" {
                "https://openrouter.ai/api"
            } else {
                base_url
            };
            let client = custom::CustomProvider::new(&resolved_key, model, url);
            Ok(Box::new(client))
        }
        "cerebras" => {
            let url = if base_url == "https://api.anthropic.com" || base_url == "https://api.openai.com" {
                "https://api.cerebras.ai"
            } else {
                base_url
            };
            let client = custom::CustomProvider::new(&resolved_key, model, url);
            Ok(Box::new(client))
        }
        "gemini" => {
            let url = if base_url == "https://api.anthropic.com" || base_url == "https://api.openai.com" {
                "https://generativelanguage.googleapis.com/v1beta/openai"
            } else {
                base_url
            };
            let client = custom::CustomProvider::new(&resolved_key, model, url);
            Ok(Box::new(client))
        }
        "custom" => {
            let client = custom::CustomProvider::new(&resolved_key, model, base_url);
            Ok(Box::new(client))
        }
        other => Err(AppError::Config(format!(
            "Unknown provider '{other}'. Supported: anthropic, openai, openrouter, cerebras, gemini, custom"
        ))),
    }
}

/// Validate that the selected model supports image inputs.
///
/// Some models (e.g., older text-only models) cannot process screenshots.
/// This check runs at provider creation time to fail fast.
fn validate_image_support(provider_name: &str, model: &str) -> Result<(), AppError> {
    let text_only_models = match provider_name {
        "anthropic" => &["claude-instant-1", "claude-instant-1.2"][..],
        _ => &[],
    };

    if text_only_models.iter().any(|m| model.starts_with(m)) {
        return Err(AppError::Config(format!(
            "Model '{model}' does not support image inputs. \
             Use a vision-capable model (e.g., claude-sonnet-4-20250514)."
        )));
    }

    Ok(())
}

/// Resolve the API key using the fallback chain:
/// 1. Explicit key (if non-empty)
/// 2. Provider-specific env var
/// 3. LLM_API_KEY env var
pub fn resolve_api_key(explicit_key: &str, provider_name: &str) -> Result<String, AppError> {
    resolve_api_key_with_source(explicit_key, provider_name).map(|(key, _source)| key)
}

/// Like `resolve_api_key`, but also returns a label indicating where the key
/// came from (e.g. "config file", "ANTHROPIC_API_KEY", "LLM_API_KEY").
/// Used by the `doctor` command to show the key source without revealing the key.
pub fn resolve_api_key_with_source(
    explicit_key: &str,
    provider_name: &str,
) -> Result<(String, &'static str), AppError> {
    if !explicit_key.is_empty() {
        return Ok((explicit_key.to_string(), "config file"));
    }

    let provider_env = match provider_name {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "cerebras" => "CEREBRAS_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        _ => "",
    };

    if !provider_env.is_empty() {
        if let Ok(key) = std::env::var(provider_env) {
            if !key.is_empty() {
                return Ok((key, provider_env));
            }
        }
    }

    if let Ok(key) = std::env::var("LLM_API_KEY") {
        if !key.is_empty() {
            return Ok((key, "LLM_API_KEY"));
        }
    }

    Err(AppError::Config(format!(
        "No API key found. Set it in config, {provider_env}, or LLM_API_KEY."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_message() {
        let msg = system_message("Hello");
        assert_eq!(msg.role, "system");
        assert_eq!(
            msg.content.unwrap(),
            serde_json::Value::String("Hello".into())
        );
    }

    #[test]
    fn test_user_message() {
        let msg = user_message("Hi");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content.unwrap(), serde_json::Value::String("Hi".into()));
    }

    #[test]
    fn test_user_image_message() {
        let msg = user_image_message("data:image/png;base64,abc");
        assert_eq!(msg.role, "user");
        let content = msg.content.unwrap();
        let arr = content.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "image_url");
        assert_eq!(arr[0]["image_url"]["url"], "data:image/png;base64,abc");
    }

    #[test]
    fn test_resolve_api_key_explicit() {
        let key = resolve_api_key("sk-test", "openai").unwrap();
        assert_eq!(key, "sk-test");
    }

    #[test]
    fn test_resolve_api_key_empty_explicit_falls_through() {
        let key = resolve_api_key("my-key", "openai").unwrap();
        assert_eq!(key, "my-key");
    }

    #[test]
    fn test_create_provider_openai() {
        let provider = create_provider("openai", "sk-test", "gpt-4.1", "https://api.openai.com");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_create_provider_anthropic() {
        let provider = create_provider(
            "anthropic",
            "sk-ant-test",
            "claude-sonnet-4-20250514",
            "https://api.anthropic.com",
        );
        assert!(provider.is_ok());
    }

    #[test]
    fn test_create_provider_custom() {
        let provider = create_provider("custom", "sk-test", "local-model", "http://localhost:8080");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_create_provider_openrouter() {
        let provider = create_provider("openrouter", "sk-or-test", "anthropic/claude-sonnet-4", "https://api.anthropic.com");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_create_provider_cerebras() {
        let provider = create_provider("cerebras", "csk-test", "llama-4-scout-17b-16e-instruct", "https://api.anthropic.com");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_create_provider_gemini() {
        let provider = create_provider("gemini", "AIza-test", "gemini-2.5-flash", "https://api.anthropic.com");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_create_provider_unknown() {
        let result = create_provider("unknown", "sk-test", "model", "https://example.com");
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Unknown provider"));
    }

    #[test]
    fn test_create_provider_with_custom_base_url() {
        let provider = create_provider("openai", "sk-test", "gpt-4.1", "https://custom.api.com");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_anthropic_text_only_model_rejected() {
        let result = create_provider(
            "anthropic",
            "sk-ant-test",
            "claude-instant-1.2",
            "https://api.anthropic.com",
        );
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("does not support image inputs"));
    }

    #[test]
    fn test_anthropic_vision_model_accepted() {
        let result = create_provider(
            "anthropic",
            "sk-ant-test",
            "claude-sonnet-4-20250514",
            "https://api.anthropic.com",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_api_key_anthropic_provider() {
        // With explicit key, should resolve immediately
        let key = resolve_api_key("sk-ant-test", "anthropic").unwrap();
        assert_eq!(key, "sk-ant-test");
    }

    #[test]
    fn test_resolve_api_key_custom_provider() {
        // Custom provider falls through to LLM_API_KEY (no provider-specific env var)
        let key = resolve_api_key("custom-key", "custom").unwrap();
        assert_eq!(key, "custom-key");
    }
}
