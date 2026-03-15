//! OSWorld-style agent loop integrating PyAutoGUI execution, observation pipeline,
//! sliding window context management, and multi-model LLM support.
//!
//! Loop flow: observe -> construct messages -> call LLM -> parse response ->
//! check for DONE/FAIL/WAIT -> execute code -> observe -> repeat

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::agent::context::{is_context_length_error, ContextManager, TrajectoryTurn};
use crate::agent::pyautogui::{self, SpecialCommand};
use crate::docker::DockerSession;
use crate::error::{AgentOutcome, AppError};
use crate::observation::{self, Observation, ObservationConfig};
use crate::provider::{ChatMessage, LlmProvider};
use crate::recording::Recording;
use crate::trajectory::TrajectoryLogger;

/// Default maximum number of agent steps per test.
const DEFAULT_MAX_STEPS: usize = 15;

/// Default per-step wall-clock timeout in seconds.
const DEFAULT_STEP_TIMEOUT_SECS: u64 = 60;

/// Default total wall-clock timeout for the entire test in seconds.
const DEFAULT_TOTAL_TIMEOUT_SECS: u64 = 300;

/// Retry interval for LLM API transient errors (429, 5xx).
const LLM_RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum number of LLM API retries on transient errors.
const LLM_MAX_RETRIES: usize = 10;

/// Configuration for the v2 agent loop.
#[derive(Debug, Clone)]
pub struct AgentLoopV2Config {
    /// Maximum number of steps before termination.
    pub max_steps: usize,
    /// Per-step timeout (wall-clock time for a single LLM call + execution).
    pub step_timeout: Duration,
    /// Total timeout for the entire test run.
    pub total_timeout: Duration,
    /// Observation pipeline configuration.
    pub observation_config: ObservationConfig,
    /// Maximum trajectory length for sliding window context.
    pub max_trajectory_length: usize,
    /// Enable verbose/debug logging.
    pub debug: bool,
    /// Enable verbose trajectory logging (includes full LLM responses).
    pub verbose: bool,
}

impl Default for AgentLoopV2Config {
    fn default() -> Self {
        Self {
            max_steps: DEFAULT_MAX_STEPS,
            step_timeout: Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECS),
            total_timeout: Duration::from_secs(DEFAULT_TOTAL_TIMEOUT_SECS),
            observation_config: ObservationConfig::default(),
            max_trajectory_length: crate::agent::context::DEFAULT_MAX_TRAJECTORY_LENGTH,
            debug: false,
            verbose: false,
        }
    }
}

/// The OSWorld-style agent loop (v2).
///
/// Integrates:
/// - LlmProvider for multi-model support
/// - PyAutoGUI execution for desktop interaction
/// - Observation pipeline (screenshot + a11y tree)
/// - Sliding window context management
pub struct AgentLoopV2<'a> {
    client: Box<dyn LlmProvider>,
    session: &'a DockerSession,
    artifacts_dir: PathBuf,
    context: ContextManager,
    config: AgentLoopV2Config,
    trajectory: Option<TrajectoryLogger>,
    recording: Option<&'a Recording>,
}

impl<'a> AgentLoopV2<'a> {
    /// Create a new v2 agent loop.
    pub fn new(
        client: Box<dyn LlmProvider>,
        session: &'a DockerSession,
        artifacts_dir: PathBuf,
        instruction: &str,
        display_width: u32,
        display_height: u32,
        config: AgentLoopV2Config,
        recording: Option<&'a Recording>,
    ) -> Self {
        let context = ContextManager::new(
            display_width,
            display_height,
            instruction,
            config.max_trajectory_length,
        );

        let trajectory = match TrajectoryLogger::new(&artifacts_dir, config.verbose) {
            Ok(logger) => Some(logger),
            Err(e) => {
                warn!("Failed to create trajectory logger: {e}");
                None
            }
        };

        Self {
            client,
            session,
            artifacts_dir,
            context,
            config,
            trajectory,
            recording,
        }
    }

    /// Run the agent loop to completion.
    ///
    /// Returns an `AgentOutcome` with the test verdict, or an error on infra/config failure.
    pub async fn run(&mut self) -> Result<AgentOutcome, AppError> {
        let start_time = Instant::now();
        let mut step_index: usize = 0;

        info!(
            "Starting AgentLoopV2: max_steps={}, step_timeout={:?}, total_timeout={:?}",
            self.config.max_steps, self.config.step_timeout, self.config.total_timeout
        );

        // Capture initial observation (before any action)
        info!("Capturing initial observation...");
        let mut current_observation = self.capture_observation_for_step(0).await?;

        loop {
            // Check total timeout
            if start_time.elapsed() >= self.config.total_timeout {
                warn!(
                    "Total timeout ({:?}) exceeded after {} steps",
                    self.config.total_timeout, step_index
                );
                self.log_trajectory_entry(
                    step_index,
                    "",
                    &[],
                    &current_observation,
                    "timeout",
                    None,
                );
                self.save_conversation_log();
                return Ok(AgentOutcome {
                    passed: false,
                    reasoning: format!(
                        "Total timeout ({}s) exceeded after {} steps",
                        self.config.total_timeout.as_secs(),
                        step_index
                    ),
                    screenshot_count: step_index,
                });
            }

            // Check max steps
            if step_index >= self.config.max_steps {
                warn!("Max steps ({}) reached", self.config.max_steps);
                self.log_trajectory_entry(
                    step_index,
                    "",
                    &[],
                    &current_observation,
                    "max_steps",
                    None,
                );
                self.save_conversation_log();
                return Ok(AgentOutcome {
                    passed: false,
                    reasoning: format!(
                        "Max steps ({}) reached without task completion",
                        self.config.max_steps
                    ),
                    screenshot_count: step_index,
                });
            }

            step_index += 1;
            info!("--- Step {}/{} ---", step_index, self.config.max_steps);

            // Build messages with sliding window context
            let messages = self.context.build_messages(&current_observation);

            // Call LLM with retry on transient errors and step timeout
            let llm_result = self.call_llm_with_retry(&messages, &current_observation).await;

            let response = match llm_result {
                Ok(msg) => msg,
                Err(e) => {
                    warn!("LLM call failed after retries: {e}");
                    self.save_conversation_log();
                    return Err(e);
                }
            };

            // Extract text content from the response
            let response_text = extract_text_content(&response);
            if self.config.debug {
                debug!("LLM response: {response_text}");
            }
            info!("LLM response length: {} chars", response_text.len());

            // Parse response for special commands and code blocks
            let parsed = pyautogui::parse_response(&response_text);
            let code_blocks = parsed.code_blocks.clone();

            // Update video caption with agent's thought before executing
            self.update_caption(step_index, &response_text, &code_blocks).await;

            let turn_result = pyautogui::process_turn(
                self.session,
                &response_text,
                Some(self.config.step_timeout),
            )
            .await?;

            // Check for special commands
            if let Some(ref command) = turn_result.command {
                match command {
                    SpecialCommand::Done => {
                        info!("Agent signalled DONE at step {step_index}");
                        self.log_trajectory_entry(
                            step_index,
                            &response_text,
                            &code_blocks,
                            &current_observation,
                            "done",
                            Some(&response_text),
                        );
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: None,
                        });
                        self.save_conversation_log();
                        return Ok(AgentOutcome {
                            passed: true,
                            reasoning: extract_reasoning(&response_text),
                            screenshot_count: step_index,
                        });
                    }
                    SpecialCommand::Fail => {
                        info!("Agent signalled FAIL at step {step_index}");
                        self.log_trajectory_entry(
                            step_index,
                            &response_text,
                            &code_blocks,
                            &current_observation,
                            "fail",
                            Some(&response_text),
                        );
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: None,
                        });
                        self.save_conversation_log();
                        return Ok(AgentOutcome {
                            passed: false,
                            reasoning: extract_reasoning(&response_text),
                            screenshot_count: step_index,
                        });
                    }
                    SpecialCommand::Wait => {
                        info!("Agent signalled WAIT at step {step_index}, re-observing...");
                        self.log_trajectory_entry(
                            step_index,
                            &response_text,
                            &code_blocks,
                            &current_observation,
                            "wait",
                            Some(&response_text),
                        );
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: None,
                        });
                        // Re-observe without executing any code
                        current_observation =
                            self.capture_observation_for_step(step_index).await?;
                        continue;
                    }
                }
            }

            // Determine result string for trajectory
            let result_str = if turn_result.all_succeeded {
                "success".to_string()
            } else {
                format!(
                    "error:{}",
                    turn_result
                        .error_feedback
                        .as_deref()
                        .unwrap_or("unknown error")
                )
            };

            // Log trajectory entry
            self.log_trajectory_entry(
                step_index,
                &response_text,
                &code_blocks,
                &current_observation,
                &result_str,
                Some(&response_text),
            );

            // Record the turn in trajectory
            self.context.push_turn(TrajectoryTurn {
                observation: current_observation,
                response_text: response_text.clone(),
                error_feedback: turn_result.error_feedback.clone(),
            });

            // If no code blocks were extracted (text-only response without special commands)
            if turn_result.executions.is_empty() && turn_result.command.is_none() {
                warn!("No code blocks or special commands in LLM response, re-observing...");
            }

            // Capture new observation after action(s)
            current_observation = self.capture_observation_for_step(step_index).await?;
        }
    }

    /// Run the agent loop step by step, pausing after each step for user confirmation.
    ///
    /// Reads a line from stdin after each step. Press Enter to continue, 'q' to quit.
    pub async fn run_step_by_step(&mut self) -> Result<AgentOutcome, AppError> {
        let mut execution_elapsed = Duration::ZERO;
        let mut step_index: usize = 0;

        info!(
            "Starting AgentLoopV2 (step-by-step): max_steps={}, step_timeout={:?}, total_timeout={:?}",
            self.config.max_steps, self.config.step_timeout, self.config.total_timeout
        );

        // Capture initial observation
        info!("Capturing initial observation...");
        let mut current_observation = self.capture_observation_for_step(0).await?;

        loop {
            if execution_elapsed >= self.config.total_timeout {
                warn!("Total timeout exceeded after {} steps", step_index);
                self.save_conversation_log();
                return Ok(AgentOutcome {
                    passed: false,
                    reasoning: format!(
                        "Total timeout ({}s) exceeded after {} steps",
                        self.config.total_timeout.as_secs(), step_index
                    ),
                    screenshot_count: step_index,
                });
            }

            if step_index >= self.config.max_steps {
                warn!("Max steps ({}) reached", self.config.max_steps);
                self.save_conversation_log();
                return Ok(AgentOutcome {
                    passed: false,
                    reasoning: format!("Max steps ({}) reached", self.config.max_steps),
                    screenshot_count: step_index,
                });
            }

            step_index += 1;

            // Pause and wait for user input (timeout does NOT tick during this wait)
            println!("\n--- Step {}/{} --- Press Enter to execute, 'q' to quit ---",
                step_index, self.config.max_steps);
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() {
                let trimmed = input.trim().to_lowercase();
                if trimmed == "q" || trimmed == "quit" {
                    info!("User requested quit at step {step_index}");
                    self.save_conversation_log();
                    return Ok(AgentOutcome {
                        passed: false,
                        reasoning: format!("User quit at step {step_index}"),
                        screenshot_count: step_index - 1,
                    });
                }
            }

            let step_start = Instant::now();
            info!("--- Executing step {}/{} ---", step_index, self.config.max_steps);

            // Build messages and call LLM
            let messages = self.context.build_messages(&current_observation);
            let llm_result = self.call_llm_with_retry(&messages, &current_observation).await;

            let response = match llm_result {
                Ok(msg) => msg,
                Err(e) => {
                    warn!("LLM call failed: {e}");
                    self.save_conversation_log();
                    return Err(e);
                }
            };

            let response_text = extract_text_content(&response);
            println!("  LLM response ({} chars):", response_text.len());

            // Show a preview of the response
            let preview: String = response_text.chars().take(500).collect();
            println!("  {preview}");
            if response_text.len() > 500 {
                println!("  ... (truncated)");
            }

            let parsed = pyautogui::parse_response(&response_text);
            let code_blocks = parsed.code_blocks.clone();

            let turn_result = pyautogui::process_turn(
                self.session,
                &response_text,
                Some(self.config.step_timeout),
            )
            .await?;

            // Check for special commands
            if let Some(ref command) = turn_result.command {
                match command {
                    SpecialCommand::Done => {
                        println!("  => Agent signalled DONE");
                        self.log_trajectory_entry(step_index, &response_text, &code_blocks, &current_observation, "done", Some(&response_text));
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: None,
                        });
                        self.save_conversation_log();
                        return Ok(AgentOutcome {
                            passed: true,
                            reasoning: extract_reasoning(&response_text),
                            screenshot_count: step_index,
                        });
                    }
                    SpecialCommand::Fail => {
                        println!("  => Agent signalled FAIL");
                        self.log_trajectory_entry(step_index, &response_text, &code_blocks, &current_observation, "fail", Some(&response_text));
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: None,
                        });
                        self.save_conversation_log();
                        return Ok(AgentOutcome {
                            passed: false,
                            reasoning: extract_reasoning(&response_text),
                            screenshot_count: step_index,
                        });
                    }
                    SpecialCommand::Wait => {
                        println!("  => Agent signalled WAIT, re-observing...");
                        self.log_trajectory_entry(step_index, &response_text, &code_blocks, &current_observation, "wait", Some(&response_text));
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: None,
                        });
                        current_observation = self.capture_observation_for_step(step_index).await?;
                        continue;
                    }
                }
            }

            // Show execution result
            let result_str = if turn_result.all_succeeded {
                println!("  => Executed {} code block(s) successfully", turn_result.executions.len());
                "success".to_string()
            } else {
                let err = turn_result.error_feedback.as_deref().unwrap_or("unknown error");
                println!("  => Execution error: {err}");
                format!("error:{err}")
            };

            self.log_trajectory_entry(step_index, &response_text, &code_blocks, &current_observation, &result_str, Some(&response_text));
            self.context.push_turn(TrajectoryTurn {
                observation: current_observation,
                response_text: response_text.clone(),
                error_feedback: turn_result.error_feedback.clone(),
            });

            // Capture new observation
            current_observation = self.capture_observation_for_step(step_index).await?;
            execution_elapsed += step_start.elapsed();
        }
    }

    /// Log a trajectory entry for the current step.
    fn log_trajectory_entry(
        &mut self,
        step_index: usize,
        response_text: &str,
        code_blocks: &[String],
        observation: &Observation,
        result: &str,
        raw_response: Option<&str>,
    ) {
        if let Some(ref mut trajectory) = self.trajectory {
            let entry = trajectory.build_entry(
                step_index,
                response_text,
                code_blocks,
                observation.screenshot_path.as_deref(),
                observation.a11y_tree_text.as_deref(),
                result,
                raw_response,
            );
            trajectory.log_entry(&entry);
        }
    }

    /// Update the video recording caption with the agent's current thought and actions.
    async fn update_caption(&self, step: usize, response_text: &str, code_blocks: &[String]) {
        if let Some(recording) = self.recording {
            let thought = crate::trajectory::extract_thought(response_text, code_blocks);
            recording
                .update_caption(self.session, step, thought.as_deref(), code_blocks)
                .await;
        }
    }

    /// Capture an observation for the given step, handling errors gracefully.
    async fn capture_observation_for_step(
        &self,
        step_index: usize,
    ) -> Result<Observation, AppError> {
        observation::capture_observation(
            self.session,
            &self.artifacts_dir,
            step_index,
            &self.config.observation_config,
        )
        .await
    }

    /// Call the LLM with retry on transient errors (429, 5xx).
    ///
    /// On context_length_exceeded errors, falls back to a minimal message set
    /// (system prompt + current observation only, dropping trajectory).
    async fn call_llm_with_retry(
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
                                        warn!("Transient error on fallback (attempt {fallback_attempt}): {fb_err}");
                                        fallback_last_err = Some(fb_err);
                                        continue;
                                    }
                                    warn!("Fallback LLM call failed (non-transient): {fb_err}");
                                    return Err(fb_err);
                                }
                                Err(_timeout) => {
                                    warn!("Fallback LLM call timed out (attempt {fallback_attempt})");
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

        Err(last_err.unwrap_or_else(|| {
            AppError::Agent("LLM call failed after max retries".into())
        }))
    }

    /// Save the conversation log to artifacts.
    fn save_conversation_log(&self) {
        // Build the current message state for logging
        let dummy_obs = Observation {
            screenshot_path: None,
            screenshot_data_url: None,
            a11y_tree_text: None,
        };
        let messages = self.context.build_messages(&dummy_obs);
        let log_path = self.artifacts_dir.join("agent_conversation.json");

        // Sanitize base64 image data for readability
        let sanitized: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| {
                let mut val = serde_json::to_value(msg).unwrap_or_default();
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
}

/// Check if an error message indicates a transient/retryable error.
fn is_transient_error(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("server error")
        || lower.contains("internal error")
        || lower.contains("overloaded")
        || lower.contains("temporarily unavailable")
}

/// Extract the text content from a ChatMessage response.
fn extract_text_content(message: &ChatMessage) -> String {
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
fn extract_reasoning(response_text: &str) -> String {
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
        assert!(is_transient_error("500 Internal Server Error"));
    }

    #[test]
    fn test_transient_503() {
        assert!(is_transient_error("503 Service Temporarily Unavailable"));
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

    // --- AgentLoopV2Config tests ---

    #[test]
    fn test_default_config() {
        let config = AgentLoopV2Config::default();
        assert_eq!(config.max_steps, DEFAULT_MAX_STEPS);
        assert_eq!(config.step_timeout, Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECS));
        assert_eq!(config.total_timeout, Duration::from_secs(DEFAULT_TOTAL_TIMEOUT_SECS));
        assert_eq!(config.max_trajectory_length, 3);
        assert!(!config.debug);
        assert!(!config.verbose);
    }

    // --- Integration-style tests ---

    #[test]
    fn test_agent_loop_v2_config_custom() {
        let config = AgentLoopV2Config {
            max_steps: 25,
            step_timeout: Duration::from_secs(120),
            total_timeout: Duration::from_secs(600),
            observation_config: ObservationConfig::default(),
            max_trajectory_length: 5,
            debug: true,
            verbose: true,
        };
        assert_eq!(config.max_steps, 25);
        assert_eq!(config.step_timeout.as_secs(), 120);
        assert_eq!(config.total_timeout.as_secs(), 600);
        assert_eq!(config.max_trajectory_length, 5);
        assert!(config.debug);
        assert!(config.verbose);
    }

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
        let text = "I see the editor. Let me type.\n\n```python\npyautogui.click(100, 200)\n```\n\nDONE";
        let reasoning = extract_reasoning(text);
        assert!(reasoning.contains("I see the editor"));
        assert!(reasoning.contains("```python"));
    }
}
