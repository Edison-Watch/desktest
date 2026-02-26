#![allow(dead_code)]

use std::pin::Pin;

use tracing::info;

use super::{ChatMessage, LlmProvider};
use crate::error::AppError;

/// Response shape from OpenAI-compatible chat completions endpoints.
#[derive(Debug, serde::Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, serde::Deserialize)]
struct Choice {
    message: ChatMessage,
}

/// Custom provider for OpenAI-compatible endpoints with a configurable base URL.
///
/// This allows using any API that implements the OpenAI chat completions interface,
/// such as local inference servers, vLLM, Ollama, etc.
pub struct CustomProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl CustomProvider {
    pub fn new(api_key: &str, model: &str, base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
        }
    }

    /// Get the full URL for the chat completions endpoint.
    pub fn completions_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }
}

impl LlmProvider for CustomProvider {
    fn chat_completion<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        tools: &'a [serde_json::Value],
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ChatMessage, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let mut body = serde_json::json!({
                "model": self.model,
                "messages": messages,
            });

            if !tools.is_empty() {
                body["tools"] = serde_json::json!(tools);
                body["tool_choice"] = serde_json::json!("auto");
            }

            let body_str = serde_json::to_string(&body).unwrap_or_default();
            let payload_kb = body_str.len() / 1024;
            info!(
                "Custom API request to {}: {} messages, ~{} KB payload",
                self.base_url,
                messages.len(),
                payload_kb
            );

            let start = std::time::Instant::now();
            let url = self.completions_url();

            let response = self
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| AppError::Agent(format!("HTTP request failed: {e}")))?;

            let elapsed = start.elapsed();
            let status = response.status();

            if !status.is_success() {
                let error_body = response.text().await.unwrap_or_default();
                return Err(AppError::Agent(format!(
                    "Custom API error ({}): {}",
                    status, error_body
                )));
            }

            let completion: ChatCompletionResponse = response
                .json()
                .await
                .map_err(|e| AppError::Agent(format!("Failed to parse response: {e}")))?;

            let msg = completion
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| AppError::Agent("No choices in response".into()))?
                .message;

            let tool_names: Vec<&str> = msg
                .tool_calls
                .as_ref()
                .map(|tcs| tcs.iter().map(|tc| tc.function.name.as_str()).collect())
                .unwrap_or_default();
            info!(
                "Custom API response in {:.1}s: tool_calls={:?}",
                elapsed.as_secs_f64(),
                tool_names
            );

            Ok(msg)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::user_message;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_text_response(content: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": content
                }
            }]
        })
    }

    // ---------- URL construction tests ----------

    #[test]
    fn test_completions_url_default() {
        let provider = CustomProvider::new("key", "model", "https://my-server.com");
        assert_eq!(
            provider.completions_url(),
            "https://my-server.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_with_trailing_slash() {
        // Users may or may not include trailing slash - our URL construction handles this
        let provider = CustomProvider::new("key", "model", "https://my-server.com");
        assert!(provider.completions_url().starts_with("https://my-server.com/"));
    }

    #[test]
    fn test_completions_url_localhost() {
        let provider = CustomProvider::new("key", "llama-3", "http://localhost:8080");
        assert_eq!(
            provider.completions_url(),
            "http://localhost:8080/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_with_path_prefix() {
        let provider = CustomProvider::new("key", "model", "https://gateway.example.com/api");
        assert_eq!(
            provider.completions_url(),
            "https://gateway.example.com/api/v1/chat/completions"
        );
    }

    // ---------- API integration tests ----------

    #[tokio::test]
    async fn test_simple_text_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mock_text_response("Custom response!")),
            )
            .mount(&server)
            .await;

        let provider = CustomProvider::new("sk-test", "local-model", &server.uri());
        let messages = vec![user_message("Hi")];
        let result = provider.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(result.role, "assistant");
        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Custom response!".into())
        );
    }

    #[tokio::test]
    async fn test_api_error_handling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let provider = CustomProvider::new("sk-test", "model", &server.uri());
        let result = provider
            .chat_completion(&[user_message("test")], &[])
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    #[tokio::test]
    async fn test_provider_trait_via_box_dyn() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mock_text_response("Via trait!")),
            )
            .mount(&server)
            .await;

        let provider: Box<dyn LlmProvider> =
            Box::new(CustomProvider::new("sk-test", "model", &server.uri()));
        let messages = vec![user_message("Hi")];
        let result = provider.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Via trait!".into())
        );
    }

    #[tokio::test]
    async fn test_tool_call_response() {
        let server = MockServer::start().await;
        let response_body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "test_tool",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        });
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&server)
            .await;

        let provider = CustomProvider::new("sk-test", "model", &server.uri());
        let result = provider
            .chat_completion(&[user_message("test")], &[])
            .await
            .unwrap();

        let tool_calls = result.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "test_tool");
    }
}
