use std::pin::Pin;

use tracing::info;

use super::{ChatMessage, LlmProvider};
use crate::error::AppError;

/// Response shape from the OpenAI chat completions endpoint.
#[derive(Debug, serde::Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, serde::Deserialize)]
struct Choice {
    message: ChatMessage,
}

/// Generic OpenAI-compatible HTTP provider.
///
/// Both the first-party OpenAI provider and custom OpenAI-compatible endpoints
/// share the exact same request/response flow. This struct captures that shared
/// logic, parameterized only by base URL and a display label for log messages.
pub struct HttpProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    /// Label used in log/error messages (e.g., "OpenAI" or "Custom").
    label: String,
}

impl HttpProvider {
    pub fn new(api_key: &str, model: &str, base_url: &str, label: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
            label: label.into(),
        }
    }

    /// Override base URL (for testing with wiremock or custom endpoints).
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.into();
        self
    }

    /// Get the full URL for the chat completions endpoint.
    pub fn completions_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }
}

impl LlmProvider for HttpProvider {
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
            let image_count = messages
                .iter()
                .filter(|m| {
                    m.content
                        .as_ref()
                        .and_then(|c| c.as_array())
                        .map_or(false, |arr| {
                            arr.iter().any(|item| item.get("image_url").is_some())
                        })
                })
                .count();

            info!(
                "{} API request: {} messages, {} images, ~{} KB payload",
                self.label,
                messages.len(),
                image_count,
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
                    "{} API error ({}): {}",
                    self.label, status, error_body
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
                "{} API response in {:.1}s: tool_calls={:?}",
                self.label,
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

    fn mock_tool_call_response(tool_calls: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": tool_calls
                }
            }]
        })
    }

    #[tokio::test]
    async fn test_simple_text_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mock_text_response("Hello!")),
            )
            .mount(&server)
            .await;

        let client = HttpProvider::new("sk-test", "gpt-4.1", &server.uri(), "Test");
        let messages = vec![user_message("Hi")];
        let result = client.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(result.role, "assistant");
        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Hello!".into())
        );
        assert!(result.tool_calls.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_response() {
        let server = MockServer::start().await;
        let tool_call = serde_json::json!({
            "id": "call_123",
            "type": "function",
            "function": {
                "name": "moveMouse",
                "arguments": "{\"posX\": 100, \"posY\": 200}"
            }
        });
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mock_tool_call_response(vec![tool_call])),
            )
            .mount(&server)
            .await;

        let client = HttpProvider::new("sk-test", "gpt-4.1", &server.uri(), "Test");
        let messages = vec![user_message("test")];
        let result = client.chat_completion(&messages, &[]).await.unwrap();

        let tool_calls = result.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "moveMouse");
        assert_eq!(tool_calls[0].id, "call_123");
    }

    #[tokio::test]
    async fn test_multiple_tool_calls() {
        let server = MockServer::start().await;
        let calls = vec![
            serde_json::json!({
                "id": "call_1", "type": "function",
                "function": { "name": "moveMouse", "arguments": "{\"posX\":0,\"posY\":0}" }
            }),
            serde_json::json!({
                "id": "call_2", "type": "function",
                "function": { "name": "leftClick", "arguments": "{}" }
            }),
        ];
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mock_tool_call_response(calls)),
            )
            .mount(&server)
            .await;

        let client = HttpProvider::new("sk-test", "gpt-4.1", &server.uri(), "Test");
        let result = client
            .chat_completion(&[user_message("test")], &[])
            .await
            .unwrap();

        let tool_calls = result.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].function.name, "moveMouse");
        assert_eq!(tool_calls[1].function.name, "leftClick");
    }

    #[tokio::test]
    async fn test_api_error_handling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = HttpProvider::new("sk-test", "gpt-4.1", &server.uri(), "Test");
        let result = client
            .chat_completion(&[user_message("test")], &[])
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::Agent(_)));
        assert!(err.to_string().contains("429"));
    }

    #[tokio::test]
    async fn test_server_error_handling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let client = HttpProvider::new("sk-test", "gpt-4.1", &server.uri(), "Test");
        let result = client
            .chat_completion(&[user_message("test")], &[])
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    #[tokio::test]
    async fn test_request_includes_tools() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mock_text_response("ok")),
            )
            .expect(1)
            .mount(&server)
            .await;

        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "test_tool",
                "parameters": {}
            }
        })];

        let client = HttpProvider::new("sk-test", "gpt-4.1", &server.uri(), "Test");
        let result = client
            .chat_completion(&[user_message("test")], &tools)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_provider_trait_via_box_dyn() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mock_text_response("Hello via trait!")),
            )
            .mount(&server)
            .await;

        let provider: Box<dyn LlmProvider> =
            Box::new(HttpProvider::new("sk-test", "gpt-4.1", &server.uri(), "Test"));
        let messages = vec![user_message("Hi")];
        let result = provider.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(result.role, "assistant");
        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Hello via trait!".into())
        );
    }

    #[test]
    fn test_completions_url() {
        let provider = HttpProvider::new("key", "model", "https://my-server.com", "Test");
        assert_eq!(
            provider.completions_url(),
            "https://my-server.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_localhost() {
        let provider = HttpProvider::new("key", "llama-3", "http://localhost:8080", "Test");
        assert_eq!(
            provider.completions_url(),
            "http://localhost:8080/v1/chat/completions"
        );
    }

    #[test]
    fn test_completions_url_with_path_prefix() {
        let provider =
            HttpProvider::new("key", "model", "https://gateway.example.com/api", "Test");
        assert_eq!(
            provider.completions_url(),
            "https://gateway.example.com/api/v1/chat/completions"
        );
    }

    #[test]
    fn test_with_base_url() {
        let provider = HttpProvider::new("key", "model", "https://api.openai.com", "OpenAI")
            .with_base_url("https://custom.api.com");
        assert_eq!(
            provider.completions_url(),
            "https://custom.api.com/v1/chat/completions"
        );
    }
}
