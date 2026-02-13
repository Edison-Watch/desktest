#![allow(dead_code)]

pub mod openai;
pub mod tools;

use std::path::PathBuf;

use tracing::{debug, info, warn};

use crate::docker::DockerSession;
use crate::error::{AgentOutcome, AppError};
use openai::{
    system_message, tool_result_message, user_image_message, user_message, ChatMessage,
    OpenAiClient,
};
use tools::{dispatch_tool, tool_definitions, ToolResult};

const SYSTEM_PROMPT: &str = "\
You are a professional software tester operating a Linux XFCE desktop via provided tools.

## Core workflow

1. ALWAYS call think() first to describe what you see and plan your next action.
2. Take a screenshot to see the current state of the screen.
3. Call think() again to analyze the screenshot before acting.
4. Perform your action (click, type, etc.).
5. Take another screenshot to verify the result.
6. Repeat until the test is complete, then call done().

## How to interact with GUI applications

- You are controlling a graphical desktop. Applications have buttons, menus, and input fields that you interact with by clicking on them with the mouse.
- To click a button or UI element: first moveMouse() to its coordinates, then leftClick().
- To type into a text field: first click on the field to focus it, then use type().
- Do NOT type mathematical expressions or commands as text unless you see a text input field that accepts them. Most GUI apps (calculators, etc.) have clickable buttons.
- The mouse cursor is visible in screenshots as a small arrow. Use it to verify your cursor position before clicking.

## Important guidelines

- Think step-by-step. After each screenshot, use think() to describe what you see and plan your next move.
- If an action doesn't produce the expected result, stop and re-evaluate. Don't repeat the same failing action.
- Use the screen coordinates carefully. Examine the screenshot to identify exact positions of buttons and UI elements before clicking.
- When you are done testing, call done() with your verdict and reasoning.";

pub struct AgentLoop<'a> {
    client: OpenAiClient,
    session: &'a DockerSession,
    artifacts_dir: PathBuf,
    instructions: String,
    debug: bool,
}

impl<'a> AgentLoop<'a> {
    pub fn new(
        client: OpenAiClient,
        session: &'a DockerSession,
        artifacts_dir: PathBuf,
        instructions: String,
        debug: bool,
    ) -> Self {
        Self {
            client,
            session,
            artifacts_dir,
            instructions,
            debug,
        }
    }

    /// Save the conversation log to artifacts, replacing base64 image data
    /// with a short placeholder to keep the file human-readable.
    fn save_conversation_log(&self, messages: &[ChatMessage]) {
        let log_path = self.artifacts_dir.join("agent_conversation.json");

        // Build a sanitized copy: replace base64 data URLs with placeholders
        let sanitized: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| {
                let mut val = serde_json::to_value(msg).unwrap_or_default();
                // Check for image_url content (user messages with screenshots)
                if let Some(content) = val.get_mut("content") {
                    if let Some(arr) = content.as_array_mut() {
                        for item in arr.iter_mut() {
                            if let Some(url) = item
                                .get_mut("image_url")
                                .and_then(|u| u.get_mut("url"))
                            {
                                if let Some(s) = url.as_str() {
                                    if s.starts_with("data:image/") {
                                        *url = serde_json::Value::String(
                                            "[base64 image data omitted]".into(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                val
            })
            .collect();

        match serde_json::to_string_pretty(&sanitized) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&log_path, json) {
                    warn!("Failed to write conversation log: {e}");
                }
            }
            Err(e) => warn!("Failed to serialize conversation log: {e}"),
        }
    }

    pub async fn run(&mut self) -> Result<AgentOutcome, AppError> {
        let tools = tool_definitions();
        let mut messages = vec![
            system_message(SYSTEM_PROMPT),
            user_message(&format!(
                "Here are your testing instructions:\n\n{}",
                self.instructions
            )),
        ];
        let mut screenshot_counter: usize = 0;

        info!("Starting agent loop");

        loop {
            self.save_conversation_log(&messages);

            let response = self.client.chat_completion(&messages, &tools).await?;
            messages.push(response.clone());

            if let Some(tool_calls) = &response.tool_calls {
                for tc in tool_calls {
                    info!("Tool call: {}({})", tc.function.name, tc.function.arguments);

                    let result = dispatch_tool(
                        &tc.function.name,
                        &tc.function.arguments,
                        &self.session,
                        &self.artifacts_dir,
                        &mut screenshot_counter,
                    )
                    .await?;

                    match result {
                        ToolResult::Done { passed, reasoning } => {
                            info!("Agent done: passed={passed}, reasoning={reasoning}");
                            messages.push(tool_result_message(&tc.id, &reasoning));
                            self.save_conversation_log(&messages);
                            return Ok(AgentOutcome {
                                passed,
                                reasoning,
                                screenshot_count: screenshot_counter,
                            });
                        }
                        ToolResult::ScreenshotTaken(data_url) => {
                            messages.push(tool_result_message(&tc.id, "Screenshot taken."));
                            messages.push(user_image_message(&data_url));
                        }
                        ToolResult::Success(text) => {
                            messages.push(tool_result_message(&tc.id, &text));
                        }
                    }
                }
            } else {
                // Model sent text-only response (unusual in tool-use mode)
                if self.debug {
                    debug!(
                        "Model text response: {:?}",
                        response.content.as_ref().map(|c| c.to_string())
                    );
                }
                // Continue the loop - model may follow up with tool calls
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_done_response(is_good: bool, reasoning: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_done",
                        "type": "function",
                        "function": {
                            "name": "done",
                            "arguments": serde_json::json!({
                                "isGood": is_good,
                                "reasoning": reasoning
                            }).to_string()
                        }
                    }]
                }
            }]
        })
    }

    fn test_session_not_needed() -> (crate::config::Config, PathBuf) {
        // For tests that call done() immediately, the session/artifacts aren't used
        // by the done tool. We still need a valid config for DockerSession though,
        // so we test at a higher level using wiremock.
        let config = crate::config::Config {
            openai_api_key: "sk-test".into(),
            openai_model: "gpt-4.1".into(),
            display_width: 1280,
            display_height: 800,
            vnc_bind_addr: "127.0.0.1".into(),
            vnc_port: None,
            app_type: crate::config::AppType::Appimage,
            app_path: None,
            app_dir: None,
            entrypoint: None,
            startup_timeout_seconds: 30,
        };
        let artifacts = std::env::temp_dir().join("test-artifacts");
        (config, artifacts)
    }

    #[tokio::test]
    async fn test_system_prompt_includes_instructions() {
        // Verify the system prompt and instructions are set up correctly
        let messages = vec![
            system_message(SYSTEM_PROMPT),
            user_message(&format!(
                "Here are your testing instructions:\n\n{}",
                "Click the button and verify it works."
            )),
        ];

        assert_eq!(messages[0].role, "system");
        assert!(messages[0]
            .content
            .as_ref()
            .unwrap()
            .as_str()
            .unwrap()
            .contains("professional software tester"));

        assert_eq!(messages[1].role, "user");
        assert!(messages[1]
            .content
            .as_ref()
            .unwrap()
            .as_str()
            .unwrap()
            .contains("Click the button"));
    }

    #[tokio::test]
    async fn test_api_error_propagates() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
            .mount(&server)
            .await;

        // We can't easily create a DockerSession in unit tests, so we test
        // the OpenAI client error propagation directly.
        let client = OpenAiClient::new("sk-test", "gpt-4.1").with_base_url(&server.uri());
        let result = client
            .chat_completion(&[user_message("test")], &tool_definitions())
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Agent(_)));
    }
}
