//! Slack notification integration via Incoming Webhooks.
//!
//! Posts bug reports as Block Kit messages to a configured Slack channel.

use std::future::Future;
use std::pin::Pin;

use reqwest::Client;
use serde_json::{Value, json};

use super::{BugEvent, Notifier};

/// Sends bug notifications to Slack via an Incoming Webhook URL.
pub struct SlackNotifier {
    client: Client,
    webhook_url: String,
    channel: Option<String>,
}

impl SlackNotifier {
    pub fn new(webhook_url: String, channel: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            client,
            webhook_url,
            channel,
        }
    }

    /// Build a Slack Block Kit payload for a bug event.
    fn build_payload(&self, event: &BugEvent) -> Value {
        // Slack header blocks have a 150-character limit on plain_text.
        // Truncate the summary to fit within that budget.
        let header_text = {
            let prefix = format!("\u{1f41b} {} \u{2014} ", event.bug_id);
            let max_summary = 150usize.saturating_sub(prefix.chars().count());
            let truncated: String = event.summary.chars().take(max_summary).collect();
            if truncated.len() < event.summary.len() {
                format!("{prefix}{truncated}\u{2026}")
            } else {
                format!("{prefix}{truncated}")
            }
        };

        let mut blocks = vec![
            json!({
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": header_text,
                    "emoji": true
                }
            }),
            json!({
                "type": "section",
                "fields": [
                    {
                        "type": "mrkdwn",
                        "text": format!("*Test:*\n{}", event.test_id)
                    },
                    {
                        "type": "mrkdwn",
                        "text": format!("*Step:*\n{}", event.step)
                    }
                ]
            }),
        ];

        // Truncate description for Slack's 3000-char block text limit.
        // Use chars().take() for safe Unicode truncation (consistent with bug_report.rs).
        let desc = {
            let truncated: String = event.description.chars().take(2900).collect();
            if truncated.len() < event.description.len() {
                format!("{truncated}… _(truncated)_")
            } else {
                event.description.clone()
            }
        };

        blocks.push(json!({
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": desc
            }
        }));

        blocks.push(json!({ "type": "divider" }));

        let mut payload = json!({ "blocks": blocks });

        if let Some(ref channel) = self.channel {
            payload["channel"] = json!(channel);
        }

        payload
    }
}

impl Notifier for SlackNotifier {
    fn name(&self) -> &str {
        "Slack"
    }

    fn notify<'a>(
        &'a self,
        event: &'a BugEvent,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let payload = self.build_payload(event);

            let resp = self
                .client
                .post(&self.webhook_url)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("request failed: {e}"))?;

            if resp.status().is_success() {
                Ok(())
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Err(format!("HTTP {status}: {body}"))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_payload_basic() {
        let notifier = SlackNotifier::new(
            "https://hooks.slack.com/test".to_string(),
            Some("#qa-bugs".to_string()),
        );
        let event = BugEvent {
            bug_id: "BUG-001".to_string(),
            step: 5,
            summary: "Save dialog loses extension".to_string(),
            description: "Expected .txt but got nothing".to_string(),
            screenshot_path: None,
            test_id: "gedit-save".to_string(),
        };

        let payload = notifier.build_payload(&event);
        assert_eq!(payload["channel"], "#qa-bugs");

        let blocks = payload["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 4); // header, fields, description, divider

        // Header contains bug ID
        let header_text = blocks[0]["text"]["text"].as_str().unwrap();
        assert!(header_text.contains("BUG-001"));
        assert!(header_text.contains("Save dialog loses extension"));
    }

    #[test]
    fn test_build_payload_no_channel() {
        let notifier = SlackNotifier::new("https://hooks.slack.com/test".to_string(), None);
        let event = BugEvent {
            bug_id: "BUG-002".to_string(),
            step: 3,
            summary: "Button unresponsive".to_string(),
            description: "Click has no effect".to_string(),
            screenshot_path: None,
            test_id: "app-test".to_string(),
        };

        let payload = notifier.build_payload(&event);
        assert!(payload.get("channel").is_none());
    }

    #[test]
    fn test_build_payload_truncates_long_description() {
        let notifier = SlackNotifier::new("https://hooks.slack.com/test".to_string(), None);
        let long_desc = "x".repeat(3500);
        let event = BugEvent {
            bug_id: "BUG-003".to_string(),
            step: 1,
            summary: "Long bug".to_string(),
            description: long_desc,
            screenshot_path: None,
            test_id: "test".to_string(),
        };

        let payload = notifier.build_payload(&event);
        let desc_text = payload["blocks"][2]["text"]["text"].as_str().unwrap();
        assert!(desc_text.len() < 3100);
        assert!(desc_text.ends_with("_(truncated)_"));
    }
}
