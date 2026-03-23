use std::time::Duration;

use tracing::{info, warn};

use super::loop_v2::AgentLoopV2;
use crate::agent::context::is_context_length_error;
use crate::error::AppError;
use crate::observation::Observation;
use crate::provider::ChatMessage;

/// Retry interval for LLM API transient errors (429, 5xx).
pub(super) const LLM_RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum number of LLM API retries on transient errors.
pub(super) const LLM_MAX_RETRIES: usize = 10;

impl<'a> AgentLoopV2<'a> {
    /// Call the LLM with retry on transient errors (429, 5xx).
    ///
    /// On context_length_exceeded errors, falls back to a minimal message set
    /// (system prompt + current observation only, dropping trajectory).
    pub(super) async fn call_llm_with_retry(
        &mut self,
        messages: &[ChatMessage],
        current_observation: &Observation,
    ) -> Result<ChatMessage, AppError> {
        let empty_tools: Vec<serde_json::Value> = vec![];
        let mut last_err = None;

        for attempt in 0..=LLM_MAX_RETRIES {
            if attempt > 0 {
                info!(
                    "LLM retry {}/{} after {}s...",
                    attempt,
                    LLM_MAX_RETRIES,
                    LLM_RETRY_INTERVAL.as_secs()
                );
                tokio::time::sleep(LLM_RETRY_INTERVAL).await;
            }

            let step_result = tokio::time::timeout(
                self.config.step_timeout,
                self.client.chat_completion(messages, &empty_tools),
            )
            .await;

            match step_result {
                Ok(Ok(response)) => return Ok(response),
                Ok(Err(e)) => {
                    let err_str = e.to_string();

                    // Check for context length error — try fallback
                    if is_context_length_error(&err_str) {
                        warn!("Context length exceeded, falling back to minimal messages");
                        self.context.clear_trajectory();
                        let fallback_messages =
                            self.context.build_fallback_messages(current_observation);

                        // Fallback call with timeout and retry on transient errors
                        let mut fallback_last_err = None;
                        for fallback_attempt in 0..=LLM_MAX_RETRIES {
                            if fallback_attempt > 0 {
                                warn!(
                                    "Fallback LLM retry {}/{}...",
                                    fallback_attempt, LLM_MAX_RETRIES
                                );
                                tokio::time::sleep(LLM_RETRY_INTERVAL).await;
                            }

                            let fallback_result = tokio::time::timeout(
                                self.config.step_timeout,
                                self.client
                                    .chat_completion(&fallback_messages, &empty_tools),
                            )
                            .await;

                            match fallback_result {
                                Ok(Ok(response)) => return Ok(response),
                                Ok(Err(fb_err)) => {
                                    if is_transient_error(&fb_err.to_string()) {
                                        warn!(
                                            "Transient error on fallback (attempt {fallback_attempt}): {fb_err}"
                                        );
                                        fallback_last_err = Some(fb_err);
                                        continue;
                                    }
                                    warn!("Fallback LLM call failed (non-transient): {fb_err}");
                                    return Err(fb_err);
                                }
                                Err(_timeout) => {
                                    warn!(
                                        "Fallback LLM call timed out (attempt {fallback_attempt})"
                                    );
                                    fallback_last_err = Some(AppError::Agent(format!(
                                        "Fallback LLM call timed out after {:?}",
                                        self.config.step_timeout
                                    )));
                                    continue;
                                }
                            }
                        }

                        return Err(fallback_last_err.unwrap_or_else(|| {
                            AppError::Agent("Fallback LLM call exhausted retries".into())
                        }));
                    }

                    // Check for transient errors (429, 5xx)
                    if is_transient_error(&err_str) {
                        warn!("Transient LLM error (attempt {attempt}): {err_str}");
                        last_err = Some(e);
                        continue;
                    }

                    // Non-transient error — fail immediately
                    return Err(e);
                }
                Err(_timeout) => {
                    warn!(
                        "LLM call timed out after {:?} (attempt {attempt})",
                        self.config.step_timeout
                    );
                    last_err = Some(AppError::Agent(format!(
                        "LLM call timed out after {:?}",
                        self.config.step_timeout
                    )));
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| AppError::Agent("LLM call failed after max retries".into())))
    }
}

/// Check if an error message indicates a transient/retryable error.
pub(super) fn is_transient_error(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("status 500")
        || lower.contains("status 502")
        || lower.contains("status 503")
        || lower.contains("status 504")
        || lower.contains("error 500")
        || lower.contains("error 502")
        || lower.contains("error 503")
        || lower.contains("error 504")
        // Provider format: "API error (502 Bad Gateway): ..."
        || lower.contains("error (500")
        || lower.contains("error (502")
        || lower.contains("error (503")
        || lower.contains("error (504")
        || lower.contains("server error")
        || lower.contains("internal error")
        || lower.contains("overloaded")
        || lower.contains("temporarily unavailable")
}

/// Extract the text content from a ChatMessage response.
pub(super) fn extract_text_content(message: &ChatMessage) -> String {
    match &message.content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            // Combined content array — extract text parts
            arr.iter()
                .filter_map(|item| {
                    if item.get("type")?.as_str()? == "text" {
                        item.get("text")?.as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => String::new(),
    }
}

/// Extract reasoning text from an LLM response (everything before the special command).
pub(super) fn extract_reasoning(response_text: &str) -> String {
    // Remove DONE/FAIL/WAIT lines and return the rest as reasoning
    let lines: Vec<&str> = response_text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != "DONE" && trimmed != "FAIL" && trimmed != "WAIT"
        })
        .collect();

    let reasoning = lines.join("\n").trim().to_string();
    if reasoning.is_empty() {
        "Agent completed without explanation".into()
    } else {
        reasoning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_text_content tests ---

    #[test]
    fn test_extract_text_from_string_content() {
        let msg = ChatMessage {
            role: "assistant".into(),
            content: Some(serde_json::Value::String("Hello world".into())),
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text_content(&msg), "Hello world");
    }

    #[test]
    fn test_extract_text_from_array_content() {
        let msg = ChatMessage {
            role: "assistant".into(),
            content: Some(serde_json::json!([
                {"type": "text", "text": "Part 1"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}},
                {"type": "text", "text": "Part 2"},
            ])),
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text_content(&msg), "Part 1\nPart 2");
    }

    #[test]
    fn test_extract_text_from_null_content() {
        let msg = ChatMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text_content(&msg), "");
    }

    // --- extract_reasoning tests ---

    #[test]
    fn test_extract_reasoning_with_done() {
        let text = "I have completed the task successfully.\n\nDONE";
        let reasoning = extract_reasoning(text);
        assert_eq!(reasoning, "I have completed the task successfully.");
    }

    #[test]
    fn test_extract_reasoning_with_fail() {
        let text = "The button does not exist.\n\nFAIL";
        let reasoning = extract_reasoning(text);
        assert_eq!(reasoning, "The button does not exist.");
    }

    #[test]
    fn test_extract_reasoning_empty() {
        let text = "DONE";
        let reasoning = extract_reasoning(text);
        assert_eq!(reasoning, "Agent completed without explanation");
    }

    #[test]
    fn test_extract_reasoning_multiline() {
        let text = "Step 1: Opened the file.\nStep 2: Edited the content.\nStep 3: Saved.\n\nDONE";
        let reasoning = extract_reasoning(text);
        assert!(reasoning.contains("Step 1"));
        assert!(reasoning.contains("Step 2"));
        assert!(reasoning.contains("Step 3"));
    }

    // --- is_transient_error tests ---

    #[test]
    fn test_transient_429() {
        assert!(is_transient_error("429 Too Many Requests"));
    }

    #[test]
    fn test_transient_rate_limit() {
        assert!(is_transient_error("Rate limit exceeded, please retry"));
    }

    #[test]
    fn test_transient_500() {
        assert!(is_transient_error("HTTP status 500 Internal Server Error"));
    }

    #[test]
    fn test_transient_503() {
        assert!(is_transient_error(
            "error 503 Service Temporarily Unavailable"
        ));
    }

    #[test]
    fn test_transient_provider_format() {
        // Actual provider error format: "OpenAI API error (502 Bad Gateway): ..."
        assert!(is_transient_error(
            "Agent error: OpenAI API error (502 Bad Gateway): upstream error"
        ));
        assert!(is_transient_error(
            "Agent error: Anthropic API error (503 Service Unavailable): overloaded"
        ));
        assert!(is_transient_error(
            "Agent error: Custom API error (504 Gateway Timeout): timeout"
        ));
    }

    #[test]
    fn test_not_transient_token_limit() {
        // "4500" should NOT match as a 500 error
        assert!(!is_transient_error("max_tokens cannot exceed 4500"));
        assert!(!is_transient_error("context window is 32500 tokens"));
    }

    #[test]
    fn test_transient_overloaded() {
        assert!(is_transient_error("The server is overloaded"));
    }

    #[test]
    fn test_not_transient_auth() {
        assert!(!is_transient_error("401 Unauthorized: Invalid API key"));
    }

    #[test]
    fn test_not_transient_bad_request() {
        assert!(!is_transient_error("400 Bad Request: invalid model"));
    }

    // --- extract_text_content edge cases ---

    #[test]
    fn test_extract_text_content_edge_cases() {
        // Number content (should return empty)
        let msg = ChatMessage {
            role: "assistant".into(),
            content: Some(serde_json::json!(42)),
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text_content(&msg), "");

        // Empty string content
        let msg = ChatMessage {
            role: "assistant".into(),
            content: Some(serde_json::Value::String(String::new())),
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text_content(&msg), "");

        // Empty array content
        let msg = ChatMessage {
            role: "assistant".into(),
            content: Some(serde_json::json!([])),
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text_content(&msg), "");
    }

    #[test]
    fn test_reasoning_extraction_preserves_code_blocks() {
        let text =
            "I see the editor. Let me type.\n\n```python\npyautogui.click(100, 200)\n```\n\nDONE";
        let reasoning = extract_reasoning(text);
        assert!(reasoning.contains("I see the editor"));
        assert!(reasoning.contains("```python"));
    }
}
