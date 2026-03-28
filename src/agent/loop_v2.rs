//! OSWorld-style agent loop integrating PyAutoGUI execution, observation pipeline,
//! sliding window context management, and multi-model LLM support.
//!
//! Loop flow: observe -> construct messages -> call LLM -> parse response ->
//! check for DONE/FAIL/WAIT -> execute code -> observe -> repeat

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::agent::context::{ContextManager, TrajectoryTurn};
use crate::agent::llm_retry::{extract_reasoning, extract_text_content};
use crate::agent::pyautogui::{self, SpecialCommand};
use crate::docker::DockerSession;
use crate::error::{AgentOutcome, AppError};
use crate::monitor::{MonitorEvent, MonitorHandle};
use crate::observation::{self, Observation, ObservationConfig};
use crate::provider::{ChatMessage, LlmProvider};
use crate::recording::Recording;
use crate::redact::Redactor;
use crate::task::{EarlyExitCondition, EarlyExitConfig};
use crate::trajectory::{StepData, TrajectoryLogger};

/// Default maximum number of agent steps per test.
const DEFAULT_MAX_STEPS: usize = 15;

/// Default per-step wall-clock timeout in seconds.
const DEFAULT_STEP_TIMEOUT_SECS: u64 = 60;

/// Default total wall-clock timeout for the entire test in seconds.
const DEFAULT_TOTAL_TIMEOUT_SECS: u64 = 300;

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
    /// Allow the agent to execute bash commands for debugging.
    pub bash_enabled: bool,
    /// Enable QA bug reporting mode.
    pub qa: bool,
    /// Display width in pixels.
    pub display_width: u32,
    /// Display height in pixels.
    pub display_height: u32,
    /// Test identifier for monitoring events.
    pub test_id: String,
    /// Secret redactor for scrubbing sensitive data from logs.
    pub redactor: Option<Redactor>,
    /// Optional early exit condition — exit the loop early when met.
    pub early_exit: Option<EarlyExitConfig>,
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
            bash_enabled: false,
            qa: false,
            display_width: 1920,
            display_height: 1080,
            test_id: String::new(),
            redactor: None,
            early_exit: None,
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
    // Fields are pub(super) to allow the split impl block in llm_retry.rs
    // to access them directly without additional accessors.
    pub(super) client: Box<dyn LlmProvider>,
    pub(super) session: &'a DockerSession,
    pub(super) artifacts_dir: PathBuf,
    pub(super) context: ContextManager,
    pub(super) config: AgentLoopV2Config,
    pub(super) trajectory: Option<TrajectoryLogger>,
    pub(super) recording: Option<&'a Recording>,
    pub(super) monitor: Option<MonitorHandle>,
    pub(super) test_id: String,
    pub(super) bug_reporter: Option<crate::bug_report::BugReporter>,
    pub(super) notifier: Option<crate::notify::NotifierPipeline>,
    pub(super) redactor: Option<Redactor>,
}

impl<'a> AgentLoopV2<'a> {
    /// Create a new v2 agent loop.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: Box<dyn LlmProvider>,
        session: &'a DockerSession,
        artifacts_dir: PathBuf,
        instruction: &str,
        config: AgentLoopV2Config,
        recording: Option<&'a Recording>,
        monitor: Option<MonitorHandle>,
        notifier: Option<crate::notify::NotifierPipeline>,
    ) -> Self {
        let test_id = config.test_id.clone();
        let redactor = config.redactor.clone();
        let context = ContextManager::new(
            config.display_width,
            config.display_height,
            instruction,
            config.max_trajectory_length,
            config.bash_enabled,
            config.qa,
        );

        let trajectory =
            match TrajectoryLogger::new(&artifacts_dir, config.verbose, redactor.clone()) {
                Ok(logger) => Some(logger),
                Err(e) => {
                    warn!("Failed to create trajectory logger: {e}");
                    None
                }
            };

        let bug_reporter = if config.qa {
            match crate::bug_report::BugReporter::new(&artifacts_dir) {
                Ok(reporter) => Some(reporter),
                Err(e) => {
                    warn!("Failed to create bug reporter: {e}");
                    None
                }
            }
        } else {
            None
        };

        Self {
            client,
            session,
            artifacts_dir,
            context,
            config,
            trajectory,
            recording,
            monitor,
            test_id,
            bug_reporter,
            notifier,
            redactor,
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

        // TestStart is published by run_task_inner in main.rs with the full task context.

        loop {
            // Check total timeout
            if start_time.elapsed() >= self.config.total_timeout {
                warn!(
                    "Total timeout ({:?}) exceeded after {} steps",
                    self.config.total_timeout, step_index
                );
                let data = StepData {
                    step_index,
                    response_text: "",
                    code_blocks: &[],
                    result: "timeout",
                    raw_response: None,
                    bash_output: None,
                    error_feedback: None,
                    action_type: None,
                };
                self.log_trajectory_entry(&data, &current_observation);
                self.save_conversation_log();
                let reasoning = format!(
                    "Total timeout ({}s) exceeded after {} steps",
                    self.config.total_timeout.as_secs(),
                    step_index
                );
                self.publish_test_complete(false, &reasoning, start_time);
                return Ok(AgentOutcome {
                    passed: false,
                    reasoning,
                    screenshot_count: step_index,
                    bugs_found: self.bugs_found(),
                });
            }

            // Check max steps
            if step_index >= self.config.max_steps {
                warn!("Max steps ({}) reached", self.config.max_steps);
                let data = StepData {
                    step_index,
                    response_text: "",
                    code_blocks: &[],
                    result: "max_steps",
                    raw_response: None,
                    bash_output: None,
                    error_feedback: None,
                    action_type: None,
                };
                self.log_trajectory_entry(&data, &current_observation);
                self.save_conversation_log();
                let reasoning = format!(
                    "Max steps ({}) reached without task completion",
                    self.config.max_steps
                );
                self.publish_test_complete(false, &reasoning, start_time);
                return Ok(AgentOutcome {
                    passed: false,
                    reasoning,
                    screenshot_count: step_index,
                    bugs_found: self.bugs_found(),
                });
            }

            step_index += 1;
            info!("--- Step {}/{} ---", step_index, self.config.max_steps);

            // Build messages with sliding window context
            let messages = self.context.build_messages(&current_observation);

            // Call LLM with retry on transient errors and step timeout
            let llm_result = self
                .call_llm_with_retry(&messages, &current_observation)
                .await;

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
                let display_text = match &self.redactor {
                    Some(r) => r.redact(&response_text),
                    None => response_text.clone(),
                };
                debug!("LLM response: {display_text}");
            }
            info!("LLM response length: {} chars", response_text.len());

            // Parse response for special commands and code blocks
            let parsed = pyautogui::parse_response(&response_text);
            let code_blocks = parsed.code_blocks.clone();
            let bash_blocks = parsed.bash_blocks.clone();
            // Combine all code blocks for display in monitor/trajectory
            let all_blocks: Vec<String> = bash_blocks
                .iter()
                .map(|b| format!("# [bash]\n{b}"))
                .chain(code_blocks.iter().cloned())
                .collect();
            let action_type = crate::trajectory::compute_action_type(&code_blocks, &bash_blocks);

            // Update video caption with agent's thought before executing
            self.update_caption(step_index, &response_text, &all_blocks)
                .await;

            let turn_result = pyautogui::process_turn(
                self.session,
                &response_text,
                Some(self.config.step_timeout),
                self.config.bash_enabled,
            )
            .await?;

            // Handle bug reports (non-terminal, always process before commands)
            self.handle_bug_reports(
                step_index,
                &turn_result.bug_reports,
                &current_observation,
                turn_result.bash_output.as_deref(),
            );

            // Check for special commands
            if let Some(ref command) = turn_result.command {
                match command {
                    SpecialCommand::Done => {
                        info!("Agent signalled DONE at step {step_index}");
                        let data = StepData {
                            step_index,
                            response_text: &response_text,
                            code_blocks: &all_blocks,
                            result: "done",
                            raw_response: Some(&response_text),
                            bash_output: turn_result.bash_output.as_deref(),
                            error_feedback: turn_result.error_feedback.as_deref(),
                            action_type: action_type.as_deref(),
                        };
                        self.log_trajectory_entry(&data, &current_observation);
                        self.publish_step_event(&data, &current_observation);
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: turn_result.error_feedback.clone(),
                            bash_output: turn_result.bash_output.clone(),
                        });
                        self.save_conversation_log();
                        let reasoning = extract_reasoning(&response_text);
                        self.publish_test_complete(true, &reasoning, start_time);
                        return Ok(AgentOutcome {
                            passed: true,
                            reasoning,
                            screenshot_count: step_index,
                            bugs_found: self.bugs_found(),
                        });
                    }
                    SpecialCommand::Fail => {
                        info!("Agent signalled FAIL at step {step_index}");
                        let data = StepData {
                            step_index,
                            response_text: &response_text,
                            code_blocks: &all_blocks,
                            result: "fail",
                            raw_response: Some(&response_text),
                            bash_output: turn_result.bash_output.as_deref(),
                            error_feedback: turn_result.error_feedback.as_deref(),
                            action_type: action_type.as_deref(),
                        };
                        self.log_trajectory_entry(&data, &current_observation);
                        self.publish_step_event(&data, &current_observation);
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: turn_result.error_feedback.clone(),
                            bash_output: turn_result.bash_output.clone(),
                        });
                        self.save_conversation_log();
                        let reasoning = extract_reasoning(&response_text);
                        self.publish_test_complete(false, &reasoning, start_time);
                        return Ok(AgentOutcome {
                            passed: false,
                            reasoning,
                            screenshot_count: step_index,
                            bugs_found: self.bugs_found(),
                        });
                    }
                    SpecialCommand::Wait => {
                        info!("Agent signalled WAIT at step {step_index}, re-observing...");
                        let data = StepData {
                            step_index,
                            response_text: &response_text,
                            code_blocks: &all_blocks,
                            result: "wait",
                            raw_response: Some(&response_text),
                            bash_output: turn_result.bash_output.as_deref(),
                            error_feedback: turn_result.error_feedback.as_deref(),
                            action_type: action_type.as_deref(),
                        };
                        self.log_trajectory_entry(&data, &current_observation);
                        self.publish_step_event(&data, &current_observation);
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: turn_result.error_feedback.clone(),
                            bash_output: turn_result.bash_output.clone(),
                        });
                        // Re-observe without executing any code
                        current_observation = self.capture_observation_for_step(step_index).await?;
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
            let data = StepData {
                step_index,
                response_text: &response_text,
                code_blocks: &all_blocks,
                result: &result_str,
                raw_response: Some(&response_text),
                bash_output: turn_result.bash_output.as_deref(),
                error_feedback: turn_result.error_feedback.as_deref(),
                action_type: action_type.as_deref(),
            };
            self.log_trajectory_entry(&data, &current_observation);
            self.publish_step_event(&data, &current_observation);

            // Record the turn in trajectory
            self.context.push_turn(TrajectoryTurn {
                observation: current_observation,
                response_text: response_text.clone(),
                error_feedback: turn_result.error_feedback.clone(),
                bash_output: turn_result.bash_output.clone(),
            });

            // If no code blocks were extracted (text-only response without special commands)
            if turn_result.executions.is_empty() && turn_result.command.is_none() {
                warn!("No code blocks or special commands in LLM response, re-observing...");
            }

            // Capture new observation after action(s)
            current_observation = self.capture_observation_for_step(step_index).await?;

            // Check early exit condition against fresh post-action observation
            if let Some(ref early_exit) = self.config.early_exit {
                if step_index % early_exit.check_every as usize == 0
                    && self.check_early_exit(&current_observation).await?
                {
                    let message = early_exit
                        .message
                        .as_deref()
                        .unwrap_or("Early exit condition met");
                    let reasoning = format!("{message} (at step {step_index})");
                    info!("Early exit triggered at step {step_index}: {message}");
                    self.save_conversation_log();
                    self.publish_test_complete(false, &reasoning, start_time);
                    return Ok(AgentOutcome {
                        passed: false,
                        reasoning,
                        screenshot_count: step_index,
                        bugs_found: self.bugs_found(),
                    });
                }
            }
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
                        self.config.total_timeout.as_secs(),
                        step_index
                    ),
                    screenshot_count: step_index,
                    bugs_found: self.bugs_found(),
                });
            }

            if step_index >= self.config.max_steps {
                warn!("Max steps ({}) reached", self.config.max_steps);
                self.save_conversation_log();
                return Ok(AgentOutcome {
                    passed: false,
                    reasoning: format!("Max steps ({}) reached", self.config.max_steps),
                    screenshot_count: step_index,
                    bugs_found: self.bugs_found(),
                });
            }

            step_index += 1;

            // Pause and wait for user input (timeout does NOT tick during this wait)
            println!(
                "\n--- Step {}/{} --- Press Enter to execute, 'q' to quit ---",
                step_index, self.config.max_steps
            );
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
                        bugs_found: self.bugs_found(),
                    });
                }
            }

            let step_start = Instant::now();
            info!(
                "--- Executing step {}/{} ---",
                step_index, self.config.max_steps
            );

            // Build messages and call LLM
            let messages = self.context.build_messages(&current_observation);
            let llm_result = self
                .call_llm_with_retry(&messages, &current_observation)
                .await;

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
            let display_text = match &self.redactor {
                Some(r) => r.redact(&response_text),
                None => response_text.clone(),
            };
            let preview: String = display_text.chars().take(500).collect();
            println!("  {preview}");
            if display_text.len() > 500 {
                println!("  ... (truncated)");
            }

            let parsed = pyautogui::parse_response(&response_text);
            let code_blocks = parsed.code_blocks.clone();
            let bash_blocks = parsed.bash_blocks.clone();
            let all_blocks: Vec<String> = bash_blocks
                .iter()
                .map(|b| format!("# [bash]\n{b}"))
                .chain(code_blocks.iter().cloned())
                .collect();
            let action_type = crate::trajectory::compute_action_type(&code_blocks, &bash_blocks);

            // Update video caption with agent's thought before executing
            self.update_caption(step_index, &response_text, &all_blocks)
                .await;

            let turn_result = pyautogui::process_turn(
                self.session,
                &response_text,
                Some(self.config.step_timeout),
                self.config.bash_enabled,
            )
            .await?;

            // Handle bug reports (non-terminal, always process before commands)
            self.handle_bug_reports(
                step_index,
                &turn_result.bug_reports,
                &current_observation,
                turn_result.bash_output.as_deref(),
            );

            // Check for special commands
            if let Some(ref command) = turn_result.command {
                match command {
                    SpecialCommand::Done => {
                        println!("  => Agent signalled DONE");
                        let data = StepData {
                            step_index,
                            response_text: &response_text,
                            code_blocks: &all_blocks,
                            result: "done",
                            raw_response: Some(&response_text),
                            bash_output: turn_result.bash_output.as_deref(),
                            error_feedback: turn_result.error_feedback.as_deref(),
                            action_type: action_type.as_deref(),
                        };
                        self.log_trajectory_entry(&data, &current_observation);
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: turn_result.error_feedback.clone(),
                            bash_output: turn_result.bash_output.clone(),
                        });
                        self.save_conversation_log();
                        return Ok(AgentOutcome {
                            passed: true,
                            reasoning: extract_reasoning(&response_text),
                            screenshot_count: step_index,
                            bugs_found: self.bugs_found(),
                        });
                    }
                    SpecialCommand::Fail => {
                        println!("  => Agent signalled FAIL");
                        let data = StepData {
                            step_index,
                            response_text: &response_text,
                            code_blocks: &all_blocks,
                            result: "fail",
                            raw_response: Some(&response_text),
                            bash_output: turn_result.bash_output.as_deref(),
                            error_feedback: turn_result.error_feedback.as_deref(),
                            action_type: action_type.as_deref(),
                        };
                        self.log_trajectory_entry(&data, &current_observation);
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: turn_result.error_feedback.clone(),
                            bash_output: turn_result.bash_output.clone(),
                        });
                        self.save_conversation_log();
                        return Ok(AgentOutcome {
                            passed: false,
                            reasoning: extract_reasoning(&response_text),
                            screenshot_count: step_index,
                            bugs_found: self.bugs_found(),
                        });
                    }
                    SpecialCommand::Wait => {
                        println!("  => Agent signalled WAIT, re-observing...");
                        let data = StepData {
                            step_index,
                            response_text: &response_text,
                            code_blocks: &all_blocks,
                            result: "wait",
                            raw_response: Some(&response_text),
                            bash_output: turn_result.bash_output.as_deref(),
                            error_feedback: turn_result.error_feedback.as_deref(),
                            action_type: action_type.as_deref(),
                        };
                        self.log_trajectory_entry(&data, &current_observation);
                        self.context.push_turn(TrajectoryTurn {
                            observation: current_observation,
                            response_text: response_text.clone(),
                            error_feedback: turn_result.error_feedback.clone(),
                            bash_output: turn_result.bash_output.clone(),
                        });
                        current_observation = self.capture_observation_for_step(step_index).await?;
                        execution_elapsed += step_start.elapsed();
                        continue;
                    }
                }
            }

            // Show execution result
            let result_str = if turn_result.all_succeeded {
                println!(
                    "  => Executed {} code block(s) successfully",
                    turn_result.executions.len()
                );
                "success".to_string()
            } else {
                let err = turn_result
                    .error_feedback
                    .as_deref()
                    .unwrap_or("unknown error");
                println!("  => Execution error: {err}");
                format!("error:{err}")
            };

            let data = StepData {
                step_index,
                response_text: &response_text,
                code_blocks: &all_blocks,
                result: &result_str,
                raw_response: Some(&response_text),
                bash_output: turn_result.bash_output.as_deref(),
                error_feedback: turn_result.error_feedback.as_deref(),
                action_type: action_type.as_deref(),
            };
            self.log_trajectory_entry(&data, &current_observation);
            self.context.push_turn(TrajectoryTurn {
                observation: current_observation,
                response_text: response_text.clone(),
                error_feedback: turn_result.error_feedback.clone(),
                bash_output: turn_result.bash_output.clone(),
            });

            // Capture new observation
            current_observation = self.capture_observation_for_step(step_index).await?;

            // Check early exit condition against fresh post-action observation
            if let Some(ref early_exit) = self.config.early_exit {
                if step_index % early_exit.check_every as usize == 0
                    && self.check_early_exit(&current_observation).await?
                {
                    let message = early_exit
                        .message
                        .as_deref()
                        .unwrap_or("Early exit condition met");
                    let reasoning = format!("{message} (at step {step_index})");
                    println!("  => Early exit: {message}");
                    self.save_conversation_log();
                    return Ok(AgentOutcome {
                        passed: false,
                        reasoning,
                        screenshot_count: step_index,
                        bugs_found: self.bugs_found(),
                    });
                }
            }

            // Track elapsed time after early exit check so LlmJudge time is counted
            execution_elapsed += step_start.elapsed();
        }
    }

    /// Check the early exit condition against current state.
    ///
    /// For programmatic conditions (file_exists, command_output, etc.), delegates to
    /// `evaluator::evaluate_metric`. For `LlmJudge`, sends the current observation
    /// to the agent's LLM and checks for a YES/NO answer.
    ///
    /// Returns `true` if the condition is met (i.e., should exit early).
    async fn check_early_exit(&self, observation: &Observation) -> Result<bool, AppError> {
        let early_exit = match &self.config.early_exit {
            Some(e) => e,
            None => return Ok(false),
        };

        match &early_exit.condition {
            EarlyExitCondition::LlmJudge { prompt } => {
                let messages = build_judge_messages(prompt, observation);
                match self.client.chat_completion(&messages, &[]).await {
                    Ok(response) => {
                        let text = extract_text_content(&response).to_uppercase();
                        Ok(text.trim() == "YES")
                    }
                    Err(e) => {
                        warn!("Early exit LLM judge call failed: {e}");
                        Ok(false)
                    }
                }
            }
            other => {
                if let Some(metric) = other.to_metric_config() {
                    let result = crate::evaluator::evaluate_metric(
                        self.session,
                        &metric,
                        &self.artifacts_dir,
                        Duration::from_secs(30),
                    )
                    .await;
                    match result {
                        Ok(r) => Ok(r.passed),
                        Err(e) => {
                            warn!("Early exit condition check failed: {e}");
                            Ok(false)
                        }
                    }
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// Helper to get bugs_found count from the reporter.
    fn bugs_found(&self) -> usize {
        self.bug_reporter.as_ref().map_or(0, |r| r.bug_count())
    }

    /// Process any bug reports from the current turn.
    fn handle_bug_reports(
        &mut self,
        step_index: usize,
        bug_reports: &[String],
        observation: &Observation,
        bash_output: Option<&str>,
    ) {
        if bug_reports.is_empty() {
            return;
        }
        if let Some(ref mut reporter) = self.bug_reporter {
            for description in bug_reports {
                let screenshot_path = observation
                    .screenshot_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string());
                match reporter.report_bug(
                    step_index,
                    description,
                    screenshot_path.as_deref(),
                    observation.a11y_tree_text.as_deref(),
                    bash_output,
                ) {
                    Ok(bug_id) => {
                        info!("Bug reported: {bug_id} at step {step_index}");

                        // Send notification to configured integrations
                        if let Some(ref notifier) = self.notifier {
                            let summary = description
                                .lines()
                                .next()
                                .unwrap_or("No summary")
                                .to_string();
                            notifier.notify_all(crate::notify::BugEvent {
                                bug_id,
                                step: step_index,
                                summary,
                                description: description.clone(),
                                screenshot_path: observation.screenshot_path.clone(),
                                test_id: self.test_id.clone(),
                            });
                        }
                    }
                    Err(e) => {
                        warn!("Failed to write bug report: {e}");
                    }
                }
            }
        }
    }

    /// Publish a StepComplete event to the live monitor.
    fn publish_step_event(
        &self,
        data: &crate::trajectory::StepData<'_>,
        observation: &Observation,
    ) {
        if let Some(ref m) = self.monitor {
            let thought = crate::trajectory::extract_thought(data.response_text, data.code_blocks);
            let action_code = data.code_blocks.join("\n\n");
            let screenshot_base64 = observation.screenshot_data_url.as_ref().and_then(|url| {
                url.strip_prefix("data:image/png;base64,")
                    .map(|s| s.to_string())
            });
            let timestamp = crate::trajectory::chrono_iso8601_now();
            m.send(MonitorEvent::StepComplete {
                step: data.step_index,
                thought,
                action_code,
                result: data.result.to_string(),
                screenshot_base64,
                timestamp,
                bash_output: data.bash_output.map(|s| s.to_string()),
                error_feedback: data.error_feedback.map(|s| s.to_string()),
                action_type: data.action_type.map(|s| s.to_string()),
            });
        }
    }

    /// Publish a TestComplete event to the live monitor.
    fn publish_test_complete(&self, passed: bool, reasoning: &str, start_time: Instant) {
        if let Some(ref m) = self.monitor {
            m.send(MonitorEvent::TestComplete {
                test_id: self.test_id.clone(),
                passed,
                reasoning: reasoning.to_string(),
                duration_ms: start_time.elapsed().as_millis() as u64,
            });
        }
    }

    /// Log a trajectory entry for the current step.
    fn log_trajectory_entry(
        &mut self,
        data: &crate::trajectory::StepData<'_>,
        observation: &Observation,
    ) {
        if let Some(ref mut trajectory) = self.trajectory {
            let entry = trajectory.build_entry(
                data,
                observation.screenshot_path.as_deref(),
                observation.a11y_tree_text.as_deref(),
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

        match serialize_conversation_log(&messages, self.redactor.as_ref()) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&log_path, json) {
                    warn!("Failed to write conversation log: {e}");
                }
            }
            Err(e) => warn!("Failed to serialize conversation log: {e}"),
        }
    }
}

/// Build messages for the LLM judge early exit check.
fn build_judge_messages(prompt: &str, observation: &Observation) -> Vec<ChatMessage> {
    use crate::provider::{system_message, user_message};

    let system = system_message(
        "You are evaluating whether a specific condition is currently met on a desktop screen. \
         You will be shown a screenshot and/or accessibility tree of the current screen state, \
         along with a condition to evaluate. \
         Answer with exactly YES if the condition is met, or NO if it is not. \
         Do not explain your reasoning — respond with only YES or NO.",
    );

    let mut messages = vec![system];

    // Build observation content (screenshot + a11y tree)
    match (
        &observation.screenshot_data_url,
        &observation.a11y_tree_text,
    ) {
        (Some(data_url), Some(a11y_text)) => {
            messages.push(ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!([
                    {
                        "type": "image_url",
                        "image_url": { "url": data_url }
                    },
                    {
                        "type": "text",
                        "text": format!(
                            "Accessibility tree:\n```\n{}\n```\n\nCondition to evaluate: {}",
                            a11y_text, prompt
                        )
                    }
                ])),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        (Some(data_url), None) => {
            messages.push(ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!([
                    {
                        "type": "image_url",
                        "image_url": { "url": data_url }
                    },
                    {
                        "type": "text",
                        "text": format!("Condition to evaluate: {}", prompt)
                    }
                ])),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        (None, Some(a11y_text)) => {
            messages.push(user_message(&format!(
                "Accessibility tree:\n```\n{}\n```\n\nCondition to evaluate: {}",
                a11y_text, prompt
            )));
        }
        (None, None) => {
            messages.push(user_message(&format!("Condition to evaluate: {}", prompt)));
        }
    }

    messages
}

fn serialize_conversation_log(
    messages: &[ChatMessage],
    redactor: Option<&Redactor>,
) -> Result<String, serde_json::Error> {
    // Sanitize base64 image data for readability
    let sanitized: Vec<serde_json::Value> = messages
        .iter()
        .map(|msg| {
            let mut val = serde_json::to_value(msg).unwrap_or_default();
            if let Some(content) = val.get_mut("content") {
                if let Some(arr) = content.as_array_mut() {
                    for item in arr.iter_mut() {
                        if let Some(url) = item.get_mut("image_url").and_then(|u| u.get_mut("url"))
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

    let mut sanitized = serde_json::Value::Array(sanitized);
    if let Some(redactor) = redactor {
        crate::redact::redact_json_value(&mut sanitized, redactor);
    }
    serde_json::to_string_pretty(&sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::user_message;
    use crate::redact::Redactor;

    // --- AgentLoopV2Config tests ---

    #[test]
    fn test_default_config() {
        let config = AgentLoopV2Config::default();
        assert_eq!(config.max_steps, DEFAULT_MAX_STEPS);
        assert_eq!(
            config.step_timeout,
            Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECS)
        );
        assert_eq!(
            config.total_timeout,
            Duration::from_secs(DEFAULT_TOTAL_TIMEOUT_SECS)
        );
        assert_eq!(config.max_trajectory_length, 3);
        assert!(!config.debug);
        assert!(!config.verbose);
        assert!(!config.bash_enabled);
        assert!(!config.qa);
    }

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
            bash_enabled: true,
            qa: false,
            display_width: 1920,
            display_height: 1080,
            test_id: String::new(),
            redactor: None,
            early_exit: None,
        };
        assert_eq!(config.max_steps, 25);
        assert_eq!(config.step_timeout.as_secs(), 120);
        assert_eq!(config.total_timeout.as_secs(), 600);
        assert_eq!(config.max_trajectory_length, 5);
        assert!(config.debug);
        assert!(config.verbose);
    }

    #[test]
    fn test_serialize_conversation_log_redacts_secrets() {
        let messages = vec![user_message("password is s3cret")];
        let redactor = Redactor::new(["s3cret".to_string()]);

        let json = serialize_conversation_log(&messages, Some(&redactor)).unwrap();

        assert!(!json.contains("s3cret"));
        assert!(json.contains("[REDACTED]"));
    }

    #[test]
    fn test_step_preview_redacts_secrets() {
        let redactor = Redactor::new(["s3cret".to_string()]);
        let response_text = "Use password s3cret to log in".to_string();
        let display_text = match Some(&redactor) {
            Some(r) => r.redact(&response_text),
            None => response_text.clone(),
        };
        let preview: String = display_text.chars().take(500).collect();

        assert!(!preview.contains("s3cret"));
        assert!(preview.contains("[REDACTED]"));
    }
}
