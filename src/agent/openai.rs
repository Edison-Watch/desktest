#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::AppError;

pub struct OpenAiClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

/// A message in the OpenAI chat completions format.
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
}

impl OpenAiClient {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.openai.com".into(),
        }
    }

    /// Override base URL (for testing with wiremock).
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.into();
        self
    }

    /// Send a chat completion request and return the assistant's response message.
    pub async fn chat_completion(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
    ) -> Result<ChatMessage, AppError> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
            body["tool_choice"] = serde_json::json!("auto");
        }

        debug!("Sending chat completion request ({} messages)", messages.len());

        let response = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Agent(format!("HTTP request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AppError::Agent(format!(
                "OpenAI API error ({}): {}",
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

        debug!(
            "Received response: content={}, tool_calls={}",
            msg.content.is_some(),
            msg.tool_calls.as_ref().map_or(0, |t| t.len())
        );

        Ok(msg)
    }
}

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

/// Helper to create a tool result message.
pub fn tool_result_message(tool_call_id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: Some(serde_json::Value::String(content.into())),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let client = OpenAiClient::new("sk-test", "gpt-4.1").with_base_url(&server.uri());
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

        let client = OpenAiClient::new("sk-test", "gpt-4.1").with_base_url(&server.uri());
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

        let client = OpenAiClient::new("sk-test", "gpt-4.1").with_base_url(&server.uri());
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

        let client = OpenAiClient::new("sk-test", "gpt-4.1").with_base_url(&server.uri());
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

        let client = OpenAiClient::new("sk-test", "gpt-4.1").with_base_url(&server.uri());
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

        let client = OpenAiClient::new("sk-test", "gpt-4.1").with_base_url(&server.uri());
        let result = client
            .chat_completion(&[user_message("test")], &tools)
            .await;
        assert!(result.is_ok());
    }
}
