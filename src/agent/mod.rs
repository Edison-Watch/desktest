#![allow(dead_code)]

pub mod openai;
pub mod tools;

use std::path::PathBuf;

use tracing::{debug, info};

use crate::docker::DockerSession;
use crate::error::{AgentOutcome, AppError};
use openai::{
    system_message, tool_result_message, user_image_message, user_message, OpenAiClient,
};
use tools::{dispatch_tool, tool_definitions, ToolResult};

const SYSTEM_PROMPT: &str = "\
You are a professional software tester operating a Linux XFCE desktop via provided tools.

You can control the mouse and keyboard, take screenshots, and report your findings.
Always take a screenshot first to see the current state of the screen before performing actions.
Take screenshots frequently to confirm the result of your actions.

Available interaction tools:
- moveMouse(posX, posY): Move cursor to coordinates
- leftClick(), doubleClick(), rightClick(), middleClick(): Mouse clicks
- scrollUp(ticks), scrollDown(ticks): Scroll the mouse wheel
- dragLeftClickMouse(startX, startY, endX, endY): Drag operation
- pressAndHoldKey(key, milliseconds, modifiers?): Press a key (supports Enter, Tab, Escape, etc.)
- type(str): Type text
- screenshot(): Take a screenshot (returns the image)
- done(isGood, reasoning): Signal test completion with verdict

When you are done testing, call done() with your verdict and reasoning.";

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
