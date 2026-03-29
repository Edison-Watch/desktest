use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::warn;

use super::loop_v2::AgentLoopV2;
use crate::agent::context::is_context_length_error;
use crate::error::AppError;
use crate::observation::Observation;
use crate::provider::ChatMessage;

/// Maximum exponential backoff before jitter is applied.
const LLM_RETRY_BACKOFF_CAP: Duration = Duration::from_secs(30);

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
        match self.retry_llm_request(messages, "LLM").await {
            Ok(response) => Ok(response),
            Err(err) => {
                let err_str = err.to_string();
                if !is_context_length_error(&err_str) {
                    return Err(err);
                }

                warn!("Context length exceeded, falling back to minimal messages");
                self.context.clear_trajectory();
                let fallback_messages = self.context.build_fallback_messages(current_observation);
                self.retry_llm_request(&fallback_messages, "Fallback LLM")
                    .await
            }
        }
    }

    async fn retry_llm_request(
        &mut self,
        messages: &[ChatMessage],
        request_label: &str,
    ) -> Result<ChatMessage, AppError> {
        let empty_tools: Vec<serde_json::Value> = vec![];
        let max_retries = self.config.llm_max_retries;

        for attempt in 0..=max_retries {
            let step_result = tokio::time::timeout(
                self.config.step_timeout,
                self.client.chat_completion(messages, &empty_tools),
            )
            .await;

            match step_result {
                Ok(Ok(response)) => return Ok(response),
                Ok(Err(e)) => {
                    let err_str = e.to_string();

                    if is_context_length_error(&err_str) {
                        return Err(e);
                    }

                    if !is_retryable_error(&err_str) {
                        return Err(e);
                    }

                    if attempt == max_retries {
                        warn!(
                            "{request_label} call exhausted retries after {} retry attempts: {err_str}",
                            max_retries
                        );
                        return Err(e);
                    }

                    let retry_number = attempt + 1;
                    let delay = retry_delay(&err_str, retry_number);
                    warn!(
                        "{request_label} retry {retry_number}/{max_retries} in {:.2}s after retryable error: {err_str}",
                        delay.as_secs_f64()
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(_timeout) => {
                    let err = AppError::Agent(format!(
                        "{request_label} call timed out after {:?}",
                        self.config.step_timeout
                    ));

                    if attempt == max_retries {
                        warn!(
                            "{request_label} call exhausted retries after {} retry attempts: {err}",
                            max_retries
                        );
                        return Err(err);
                    }

                    let retry_number = attempt + 1;
                    let delay = retry_delay(&err.to_string(), retry_number);
                    warn!(
                        "{request_label} retry {retry_number}/{max_retries} in {:.2}s after timeout: {err}",
                        delay.as_secs_f64()
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }

        Err(AppError::Agent(format!(
            "{request_label} call failed after max retries"
        )))
    }
}

/// Check if an error message indicates a retryable error.
pub(super) fn is_retryable_error(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();

    if has_retryable_status(&lower) {
        return true;
    }

    if is_non_retryable_http_error(&lower) {
        return false;
    }

    has_retryable_transport_error(&lower)
}

fn has_retryable_status(lower: &str) -> bool {
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
        || lower.contains("error (500")
        || lower.contains("error (502")
        || lower.contains("error (503")
        || lower.contains("error (504")
        || lower.contains("server error")
        || lower.contains("internal error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("gateway timeout")
        || lower.contains("overloaded")
        || lower.contains("temporarily unavailable")
}

fn has_retryable_transport_error(lower: &str) -> bool {
    lower.contains("error sending request")
        || lower.contains("http request failed")
        || lower.contains("dns")
        || lower.contains("lookup address information")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("name or service not known")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("connection aborted")
        || lower.contains("connection closed")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("tls handshake")
        || lower.contains("network unreachable")
        || lower.contains("connect error")
}

fn is_non_retryable_http_error(lower: &str) -> bool {
    matches_http_status(lower, 400)
        || matches_http_status(lower, 401)
        || matches_http_status(lower, 403)
        || matches_http_status(lower, 404)
}

fn matches_http_status(lower: &str, status: u16) -> bool {
    let status = status.to_string();
    lower.contains(&format!("status {status}"))
        || lower.contains(&format!("error {status}"))
        || lower.contains(&format!("error ({status}"))
        || lower.contains(&format!("http {status}"))
        || lower.contains(&format!("({status} "))
        || lower.contains(&format!(" {status} "))
        || lower.ends_with(&format!(" {status}"))
}

fn retry_delay(err_str: &str, retry_number: usize) -> Duration {
    parse_retry_after(err_str)
        .map(|delay| delay.min(LLM_RETRY_BACKOFF_CAP))
        .unwrap_or_else(|| exponential_backoff_with_jitter(retry_number))
}

fn parse_retry_after(err_str: &str) -> Option<Duration> {
    let lower = err_str.to_lowercase();
    let marker = "retry-after:";
    let start = lower.find(marker)? + marker.len();
    let value = err_str[start..]
        .trim_start()
        .split(')')
        .next()?
        .trim()
        .trim_end_matches(';')
        .trim();
    if let Ok(seconds) = value.trim_end_matches('s').parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    let retry_at = httpdate::parse_http_date(value).ok()?;
    let now = SystemTime::now();
    match retry_at.duration_since(now) {
        Ok(duration) => Some(duration),
        Err(_) => Some(Duration::from_secs(0)),
    }
}

fn exponential_backoff_with_jitter(retry_number: usize) -> Duration {
    let exponent = retry_number.saturating_sub(1).min(63) as u32;
    let base_secs = 1u64
        .checked_shl(exponent)
        .unwrap_or(u64::MAX)
        .min(LLM_RETRY_BACKOFF_CAP.as_secs());
    let base = Duration::from_secs(base_secs);
    apply_jitter(base)
}

fn apply_jitter(base: Duration) -> Duration {
    let mut hasher = DefaultHasher::new();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);

    let ratio = (hasher.finish() % 10_001) as f64 / 10_000.0;
    let factor = 0.75 + (ratio * 0.5);
    Duration::from_secs_f64((base.as_secs_f64() * factor).max(0.0))
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

    // --- retry classification tests ---

    #[test]
    fn test_transient_429() {
        assert!(is_retryable_error("429 Too Many Requests"));
    }

    #[test]
    fn test_transient_rate_limit() {
        assert!(is_retryable_error("Rate limit exceeded, please retry"));
    }

    #[test]
    fn test_transient_500() {
        assert!(is_retryable_error("HTTP status 500 Internal Server Error"));
    }

    #[test]
    fn test_transient_503() {
        assert!(is_retryable_error(
            "error 503 Service Temporarily Unavailable"
        ));
    }

    #[test]
    fn test_transient_provider_format() {
        // Actual provider error format: "OpenAI API error (502 Bad Gateway): ..."
        assert!(is_retryable_error(
            "Agent error: OpenAI API error (502 Bad Gateway): upstream error"
        ));
        assert!(is_retryable_error(
            "Agent error: Anthropic API error (503 Service Unavailable): overloaded"
        ));
        assert!(is_retryable_error(
            "Agent error: Custom API error (504 Gateway Timeout): timeout"
        ));
    }

    #[test]
    fn test_retryable_status_takes_priority_over_body_text() {
        assert!(is_retryable_error(
            "Agent error: OpenAI API error (502 Bad Gateway): upstream host not found"
        ));
        assert!(is_retryable_error(
            "Agent error: Anthropic API error (503 Service Unavailable): resource forbidden during maintenance"
        ));
    }

    #[test]
    fn test_not_transient_token_limit() {
        // "4500" should NOT match as a 500 error
        assert!(!is_retryable_error("max_tokens cannot exceed 4500"));
        assert!(!is_retryable_error("context window is 32500 tokens"));
    }

    #[test]
    fn test_transient_overloaded() {
        assert!(is_retryable_error("The server is overloaded"));
    }

    #[test]
    fn test_not_transient_auth() {
        assert!(!is_retryable_error("401 Unauthorized: Invalid API key"));
    }

    #[test]
    fn test_not_transient_bad_request() {
        assert!(!is_retryable_error("400 Bad Request: invalid model"));
    }

    #[test]
    fn test_bad_request_body_does_not_trigger_transport_retry() {
        assert!(!is_retryable_error(
            r#"Agent error: OpenAI API error (400 Bad Request): {"error":{"message":"timeout must be an integer"}}"#
        ));
    }

    #[test]
    fn test_transient_dns_failure() {
        assert!(is_retryable_error(
            "HTTP request failed: error sending request for url: dns error: failed to lookup address information"
        ));
    }

    #[test]
    fn test_transient_transport_error_not_blocked_by_body_text() {
        assert!(is_retryable_error(
            "HTTP request failed: tls handshake failed: peer certificate unauthorized"
        ));
    }

    #[test]
    fn test_not_transient_404() {
        assert!(!is_retryable_error("404 Not Found"));
    }

    #[test]
    fn test_retry_after_header_is_parsed() {
        assert_eq!(
            parse_retry_after(
                "Anthropic API error (429 Too Many Requests; retry-after: 12): rate limited"
            ),
            Some(Duration::from_secs(12))
        );
        assert_eq!(
            parse_retry_after(
                "OpenAI API error (429 Too Many Requests; retry-after: 3s): rate limited"
            ),
            Some(Duration::from_secs(3))
        );
    }

    #[test]
    fn test_retry_after_http_date_is_parsed() {
        let future = SystemTime::now() + Duration::from_secs(30);
        let http_date = httpdate::fmt_http_date(future);
        let err = format!(
            "Anthropic API error (429 Too Many Requests; retry-after: {http_date}): rate limited"
        );

        let parsed = parse_retry_after(&err).unwrap();
        assert!(parsed <= Duration::from_secs(30));
        assert!(parsed >= Duration::from_secs(20));
    }

    #[test]
    fn test_retry_delay_caps_retry_after_header() {
        assert_eq!(
            retry_delay(
                "Anthropic API error (429 Too Many Requests; retry-after: 120): rate limited",
                1
            ),
            LLM_RETRY_BACKOFF_CAP
        );
    }

    #[test]
    fn test_exponential_backoff_caps_at_30s_before_jitter() {
        let fifth = exponential_backoff_with_jitter(5);
        let sixth = exponential_backoff_with_jitter(6);
        assert!(fifth >= Duration::from_secs_f64(12.0));
        assert!(fifth <= Duration::from_secs_f64(20.0));
        assert!(sixth >= Duration::from_secs_f64(22.5));
        assert!(sixth <= Duration::from_secs_f64(37.5));
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
