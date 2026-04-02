use std::pin::Pin;

use reqwest::header::RETRY_AFTER;
use serde::{Deserialize, Serialize};
use tracing::info;

use super::{ChatMessage, LlmProvider, sanitize_error_body};
use crate::error::AppError;

/// Anthropic Messages API provider implementing the LlmProvider trait.
///
/// Uses the Anthropic Messages API format:
/// - POST /v1/messages with x-api-key header
/// - Content blocks with text and image (base64) support
pub struct AnthropicProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn with_client(api_key: &str, model: &str, http: reqwest::Client) -> Self {
        Self {
            http,
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://api.anthropic.com".into(),
        }
    }

    /// Override base URL (for testing with wiremock).
    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.into();
        self
    }
}

// ---------- Anthropic API types ----------

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ResponseContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
}

// ---------- Conversion helpers ----------

/// Convert common ChatMessage format to Anthropic API format.
///
/// Returns (system_prompt, messages) tuple. System messages are extracted
/// and concatenated since Anthropic uses a separate `system` field.
pub(crate) fn convert_messages(
    messages: &[ChatMessage],
) -> Result<(Option<String>, Vec<AnthropicMessage>), AppError> {
    let mut system_parts: Vec<String> = Vec::new();
    let mut anthropic_messages: Vec<AnthropicMessage> = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "system" => {
                if let Some(content) = &msg.content {
                    let text = content
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| content.to_string());
                    system_parts.push(text);
                }
            }
            "assistant" => {
                let text = msg
                    .content
                    .as_ref()
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                anthropic_messages.push(AnthropicMessage {
                    role: "assistant".into(),
                    content: AnthropicContent::Text(text),
                });
            }
            "user" => {
                let content = convert_user_content(msg)?;
                anthropic_messages.push(AnthropicMessage {
                    role: "user".into(),
                    content,
                });
            }
            "tool" => {
                // Map tool results as user messages with text content
                let text = msg
                    .content
                    .as_ref()
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let prefix = if let Some(id) = &msg.tool_call_id {
                    format!("[Tool result for {id}]: {text}")
                } else {
                    text
                };
                anthropic_messages.push(AnthropicMessage {
                    role: "user".into(),
                    content: AnthropicContent::Text(prefix),
                });
            }
            _ => {
                // Skip unknown roles
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    Ok((system, anthropic_messages))
}

/// Convert user message content to Anthropic content blocks.
fn convert_user_content(msg: &ChatMessage) -> Result<AnthropicContent, AppError> {
    let content = match &msg.content {
        Some(c) => c,
        None => return Ok(AnthropicContent::Text(String::new())),
    };

    // Simple string content
    if let Some(text) = content.as_str() {
        return Ok(AnthropicContent::Text(text.to_string()));
    }

    // Array content (may contain text and image_url blocks)
    if let Some(arr) = content.as_array() {
        let mut blocks: Vec<ContentBlock> = Vec::new();

        for item in arr {
            if let Some(item_type) = item.get("type").and_then(|t| t.as_str()) {
                match item_type {
                    "text" => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            blocks.push(ContentBlock::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                    "image_url" => {
                        if let Some(url) = item
                            .get("image_url")
                            .and_then(|iu| iu.get("url"))
                            .and_then(|u| u.as_str())
                        {
                            let block = parse_data_url_to_image_block(url)?;
                            blocks.push(block);
                        }
                    }
                    _ => {}
                }
            }
        }

        if blocks.is_empty() {
            return Ok(AnthropicContent::Text(String::new()));
        }
        return Ok(AnthropicContent::Blocks(blocks));
    }

    // Fallback: serialize as string
    Ok(AnthropicContent::Text(content.to_string()))
}

/// Parse a data URL (data:image/png;base64,ABC...) into an Anthropic image content block.
fn parse_data_url_to_image_block(data_url: &str) -> Result<ContentBlock, AppError> {
    // Expected format: data:<media_type>;base64,<data>
    let rest = data_url
        .strip_prefix("data:")
        .ok_or_else(|| AppError::Agent("Invalid image data URL: missing data: prefix".into()))?;

    let (media_type, base64_data) = rest.split_once(";base64,").ok_or_else(|| {
        AppError::Agent("Invalid image data URL: missing ;base64, separator".into())
    })?;

    Ok(ContentBlock::Image {
        source: ImageSource {
            source_type: "base64".into(),
            media_type: media_type.to_string(),
            data: base64_data.to_string(),
        },
    })
}

impl LlmProvider for AnthropicProvider {
    fn chat_completion<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        _tools: &'a [serde_json::Value],
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ChatMessage, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let (system, anthropic_messages) = convert_messages(messages)?;

            let request = AnthropicRequest {
                model: self.model.clone(),
                max_tokens: 4096,
                system,
                messages: anthropic_messages,
            };

            let body_str = serde_json::to_string(&request).unwrap_or_default();
            let payload_kb = body_str.len() / 1024;
            info!(
                "Anthropic API request: {} messages, ~{} KB payload",
                messages.len(),
                payload_kb
            );

            let start = std::time::Instant::now();

            let response = self
                .http
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await
                .map_err(|e| {
                    AppError::Agent(format!(
                        "HTTP request failed (connect={}, timeout={}, request={}): {}",
                        e.is_connect(),
                        e.is_timeout(),
                        e.is_request(),
                        e
                    ))
                })?;

            let elapsed = start.elapsed();
            let status = response.status();

            if !status.is_success() {
                let retry_after = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| format!("; retry-after: {value}"))
                    .unwrap_or_default();
                let error_body = response.text().await.unwrap_or_default();
                let error_body = sanitize_error_body(&error_body);
                return Err(AppError::Agent(format!(
                    "Anthropic API error ({status}{retry_after}): {error_body}"
                )));
            }

            let api_response: AnthropicResponse = response
                .json()
                .await
                .map_err(|e| AppError::Agent(format!("Failed to parse Anthropic response: {e}")))?;

            // Extract text content from response blocks
            let text_content: String = api_response
                .content
                .iter()
                .filter_map(|block| {
                    if block.block_type == "text" {
                        block.text.clone()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("");

            info!(
                "Anthropic API response in {:.1}s: {} chars, stop_reason={:?}",
                elapsed.as_secs_f64(),
                text_content.len(),
                api_response.stop_reason
            );

            // Convert back to common ChatMessage format
            Ok(ChatMessage {
                role: "assistant".into(),
                content: Some(serde_json::Value::String(text_content)),
                tool_calls: None,
                tool_call_id: None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{system_message, user_image_message, user_message};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_anthropic_response(text: &str) -> serde_json::Value {
        serde_json::json!({
            "content": [
                { "type": "text", "text": text }
            ],
            "stop_reason": "end_turn"
        })
    }

    // ---------- Message conversion tests ----------

    #[test]
    fn test_convert_system_message() {
        let messages = vec![system_message("You are helpful."), user_message("Hi")];
        let (system, msgs) = convert_messages(&messages).unwrap();
        assert_eq!(system.unwrap(), "You are helpful.");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn test_convert_multiple_system_messages() {
        let messages = vec![
            system_message("Part 1."),
            system_message("Part 2."),
            user_message("Hi"),
        ];
        let (system, msgs) = convert_messages(&messages).unwrap();
        assert_eq!(system.unwrap(), "Part 1.\n\nPart 2.");
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_convert_user_text_message() {
        let messages = vec![user_message("Hello")];
        let (system, msgs) = convert_messages(&messages).unwrap();
        assert!(system.is_none());
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        match &msgs[0].content {
            AnthropicContent::Text(t) => assert_eq!(t, "Hello"),
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_convert_image_message() {
        let messages = vec![user_image_message("data:image/png;base64,iVBOR")];
        let (_system, msgs) = convert_messages(&messages).unwrap();
        assert_eq!(msgs.len(), 1);
        match &msgs[0].content {
            AnthropicContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::Image { source } => {
                        assert_eq!(source.source_type, "base64");
                        assert_eq!(source.media_type, "image/png");
                        assert_eq!(source.data, "iVBOR");
                    }
                    _ => panic!("Expected image block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_convert_assistant_message() {
        let assistant_msg = ChatMessage {
            role: "assistant".into(),
            content: Some(serde_json::Value::String("I can help.".into())),
            tool_calls: None,
            tool_call_id: None,
        };
        let messages = vec![user_message("Hi"), assistant_msg];
        let (_system, msgs) = convert_messages(&messages).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].role, "assistant");
        match &msgs[1].content {
            AnthropicContent::Text(t) => assert_eq!(t, "I can help."),
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_convert_tool_result_message() {
        let tool_msg = ChatMessage {
            role: "tool".into(),
            content: Some(serde_json::Value::String("tool output".into())),
            tool_calls: None,
            tool_call_id: Some("call_123".into()),
        };
        let messages = vec![tool_msg];
        let (_system, msgs) = convert_messages(&messages).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        match &msgs[0].content {
            AnthropicContent::Text(t) => {
                assert!(t.contains("call_123"));
                assert!(t.contains("tool output"));
            }
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_parse_data_url_png() {
        let block = parse_data_url_to_image_block("data:image/png;base64,abc123").unwrap();
        match block {
            ContentBlock::Image { source } => {
                assert_eq!(source.source_type, "base64");
                assert_eq!(source.media_type, "image/png");
                assert_eq!(source.data, "abc123");
            }
            _ => panic!("Expected image block"),
        }
    }

    #[test]
    fn test_parse_data_url_jpeg() {
        let block = parse_data_url_to_image_block("data:image/jpeg;base64,xyz789").unwrap();
        match block {
            ContentBlock::Image { source } => {
                assert_eq!(source.media_type, "image/jpeg");
                assert_eq!(source.data, "xyz789");
            }
            _ => panic!("Expected image block"),
        }
    }

    #[test]
    fn test_parse_invalid_data_url() {
        let result = parse_data_url_to_image_block("https://example.com/image.png");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_data_url_missing_base64() {
        let result = parse_data_url_to_image_block("data:image/png,raw-data");
        assert!(result.is_err());
    }

    // ---------- API integration tests ----------

    #[tokio::test]
    async fn test_simple_text_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "sk-ant-test"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mock_anthropic_response("Hello from Claude!")),
            )
            .mount(&server)
            .await;

        let provider = AnthropicProvider::with_client(
            "sk-ant-test",
            "claude-sonnet-4-20250514",
            reqwest::Client::new(),
        )
        .with_base_url(&server.uri());
        let messages = vec![user_message("Hi")];
        let result = provider.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(result.role, "assistant");
        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Hello from Claude!".into())
        );
        assert!(result.tool_calls.is_none());
    }

    #[tokio::test]
    async fn test_system_message_extraction() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_anthropic_response("ok")))
            .mount(&server)
            .await;

        let provider = AnthropicProvider::with_client(
            "sk-test",
            "claude-sonnet-4-20250514",
            reqwest::Client::new(),
        )
        .with_base_url(&server.uri());
        let messages = vec![system_message("Be concise."), user_message("Hi")];
        let result = provider.chat_completion(&messages, &[]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_api_error_handling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let provider = AnthropicProvider::with_client(
            "sk-test",
            "claude-sonnet-4-20250514",
            reqwest::Client::new(),
        )
        .with_base_url(&server.uri());
        let result = provider.chat_completion(&[user_message("test")], &[]).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::Agent(_)));
        assert!(err.to_string().contains("429"));
    }

    #[tokio::test]
    async fn test_server_error_handling() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let provider = AnthropicProvider::with_client(
            "sk-test",
            "claude-sonnet-4-20250514",
            reqwest::Client::new(),
        )
        .with_base_url(&server.uri());
        let result = provider.chat_completion(&[user_message("test")], &[]).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    #[tokio::test]
    async fn test_provider_trait_via_box_dyn() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mock_anthropic_response("Hello via trait!")),
            )
            .mount(&server)
            .await;

        let provider: Box<dyn LlmProvider> = Box::new(
            AnthropicProvider::with_client(
                "sk-test",
                "claude-sonnet-4-20250514",
                reqwest::Client::new(),
            )
            .with_base_url(&server.uri()),
        );
        let messages = vec![user_message("Hi")];
        let result = provider.chat_completion(&messages, &[]).await.unwrap();

        assert_eq!(result.role, "assistant");
        assert_eq!(
            result.content.unwrap(),
            serde_json::Value::String("Hello via trait!".into())
        );
    }

    #[tokio::test]
    async fn test_image_message_sent_correctly() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mock_anthropic_response("I see an image.")),
            )
            .mount(&server)
            .await;

        let provider = AnthropicProvider::with_client(
            "sk-test",
            "claude-sonnet-4-20250514",
            reqwest::Client::new(),
        )
        .with_base_url(&server.uri());
        let messages = vec![user_image_message("data:image/png;base64,iVBORw0KGgo=")];
        let result = provider.chat_completion(&messages, &[]).await;
        assert!(result.is_ok());
    }
}
