//! PyAutoGUI action execution engine.
//!
//! Parses LLM output for Python code blocks and special commands,
//! then executes the code via the `/usr/local/bin/execute-action` script
//! inside the Docker container.

use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::docker::DockerSession;
use crate::error::AppError;

/// Default per-code-block execution timeout in seconds.
const DEFAULT_STEP_TIMEOUT_SECS: u64 = 60;

/// Special commands the agent can emit instead of (or alongside) code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialCommand {
    /// Agent wants to pause and re-observe.
    Wait,
    /// Agent considers the task complete.
    Done,
    /// Agent considers the task infeasible.
    Fail,
}

/// Result of parsing one LLM response turn.
#[derive(Debug, Clone)]
pub struct ParsedResponse {
    /// Any special command detected in the response.
    pub command: Option<SpecialCommand>,
    /// Python code blocks extracted from the response (may be multiple).
    pub code_blocks: Vec<String>,
    /// Bash code blocks extracted from the response (for debugging).
    pub bash_blocks: Vec<String>,
    /// Bug report descriptions extracted from BUG commands in the response.
    pub bug_reports: Vec<String>,
    /// The full raw text of the LLM response (for logging/context).
    pub raw_text: String,
}

/// Structured JSON response from the execute-action script.
#[derive(Debug, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub error: Option<String>,
    /// Captured stdout/stderr output (used by bash execution to return command output).
    #[serde(default)]
    pub output: Option<String>,
    pub duration_ms: u64,
}

/// Result of executing all code blocks from a single LLM turn.
#[derive(Debug)]
pub struct TurnResult {
    /// Whether a special command was detected.
    pub command: Option<SpecialCommand>,
    /// Results for each code block executed (in order).
    pub executions: Vec<ExecutionResult>,
    /// Whether all executions succeeded (empty = true).
    pub all_succeeded: bool,
    /// Error feedback to send back to the agent (if any execution failed).
    pub error_feedback: Option<String>,
    /// Captured bash command output to feed back to the agent (regardless of success/failure).
    pub bash_output: Option<String>,
    /// Bug reports detected in the LLM response.
    pub bug_reports: Vec<String>,
}

/// Parse an LLM response for special commands and Python code blocks.
///
/// Special commands are detected via simple string matching on the raw text.
/// Python code blocks are extracted from fenced ```python ... ``` blocks.
pub fn parse_response(text: &str) -> ParsedResponse {
    let command = detect_special_command(text);
    let code_blocks = extract_code_blocks(text);
    let bash_blocks = extract_bash_blocks(text);
    let bug_reports = extract_bug_reports(text);

    ParsedResponse {
        command,
        code_blocks,
        bash_blocks,
        bug_reports,
        raw_text: text.to_string(),
    }
}

/// Detect special commands in the LLM response text.
///
/// Commands are detected by matching the entire trimmed text or a line
/// that contains only the command keyword.
fn detect_special_command(text: &str) -> Option<SpecialCommand> {
    // Check each line for a standalone special command
    for line in text.lines() {
        let trimmed = line.trim();
        match trimmed {
            "WAIT" => return Some(SpecialCommand::Wait),
            "DONE" => return Some(SpecialCommand::Done),
            "FAIL" => return Some(SpecialCommand::Fail),
            _ => {}
        }
    }
    None
}

/// Extract Python code blocks from fenced markdown (```python ... ```).
///
/// Multiple code blocks per response are supported.
fn extract_code_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_block = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if !in_block {
            // Start of a code block: ```python or ```py
            if trimmed.starts_with("```python") || trimmed.starts_with("```py") {
                in_block = true;
                current_block.clear();
                continue;
            }
        } else {
            // End of a code block
            if trimmed == "```" {
                in_block = false;
                let block = current_block.trim().to_string();
                if !block.is_empty() {
                    blocks.push(block);
                }
                current_block.clear();
                continue;
            }
            // Inside a code block - preserve original indentation
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    blocks
}

/// Extract bash code blocks from fenced markdown (```bash ... ``` or ```sh ... ```).
fn extract_bash_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current_block = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if !in_block {
            if trimmed.starts_with("```bash") || trimmed.starts_with("```sh") {
                in_block = true;
                current_block.clear();
                continue;
            }
        } else {
            if trimmed == "```" {
                in_block = false;
                let block = current_block.trim().to_string();
                if !block.is_empty() {
                    blocks.push(block);
                }
                current_block.clear();
                continue;
            }
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    blocks
}

/// Extract bug report descriptions from the LLM response.
///
/// A bug report starts with `BUG` on its own line, followed by description
/// lines until a blank line, code fence, or another special command.
fn extract_bug_reports(text: &str) -> Vec<String> {
    let mut reports = Vec::new();
    let mut current_report: Option<Vec<String>> = None;
    let mut in_code_block = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Track code fence boundaries so BUG inside code blocks is ignored
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            // A closing fence also terminates any in-progress report
            if !in_code_block {
                if let Some(lines) = current_report.take() {
                    let desc = lines.join("\n").trim().to_string();
                    if !desc.is_empty() {
                        reports.push(desc);
                    }
                }
            }
            continue;
        }

        if in_code_block {
            continue;
        }

        if trimmed == "BUG" {
            // Flush any in-progress report
            if let Some(lines) = current_report.take() {
                let desc = lines.join("\n").trim().to_string();
                if !desc.is_empty() {
                    reports.push(desc);
                }
            }
            // Start a new report
            current_report = Some(Vec::new());
            continue;
        }

        if let Some(ref mut lines) = current_report {
            // End the report on blank line or special command
            if trimmed.is_empty()
                || matches!(trimmed, "DONE" | "FAIL" | "WAIT")
            {
                let desc = lines.join("\n").trim().to_string();
                if !desc.is_empty() {
                    reports.push(desc);
                }
                current_report = None;
            } else {
                lines.push(line.to_string());
            }
        }
    }

    // Flush any remaining report at end of text
    if let Some(lines) = current_report {
        let desc = lines.join("\n").trim().to_string();
        if !desc.is_empty() {
            reports.push(desc);
        }
    }

    reports
}

/// Execute a bash command inside the container and return the result.
pub async fn execute_bash_code(
    session: &DockerSession,
    code: &str,
    step_timeout: Option<Duration>,
) -> Result<ExecutionResult, AppError> {
    let timeout = step_timeout.unwrap_or(Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECS));

    debug!("Executing bash command ({} bytes, timeout {:?})", code.len(), timeout);

    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        timeout,
        session.exec_with_stdin(&["bash"], code.as_bytes()),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            debug!("Bash output ({} bytes): {}", output.len(), &output[..output.len().min(500)]);
            Ok(ExecutionResult {
                success: true,
                error: None,
                output: if output.trim().is_empty() { None } else { Some(output) },
                duration_ms,
            })
        }
        Ok(Err(e)) => {
            warn!("Bash exec failed: {e}");
            Ok(ExecutionResult {
                success: false,
                error: Some(format!("Bash exec error: {e}")),
                output: None,
                duration_ms: 0,
            })
        }
        Err(_) => {
            warn!("Bash execution timed out after {:?}", timeout);
            Ok(ExecutionResult {
                success: false,
                error: Some(format!(
                    "Bash execution timed out after {} seconds",
                    timeout.as_secs()
                )),
                output: None,
                duration_ms: timeout.as_millis() as u64,
            })
        }
    }
}

/// Execute a Python code block inside the container via the execute-action script.
///
/// Sends the code via stdin to `/usr/local/bin/execute-action` and parses
/// the structured JSON response.
pub async fn execute_code(
    session: &DockerSession,
    code: &str,
    step_timeout: Option<Duration>,
) -> Result<ExecutionResult, AppError> {
    let timeout = step_timeout.unwrap_or(Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECS));

    debug!("Executing PyAutoGUI code ({} bytes, timeout {:?})", code.len(), timeout);

    // Execute with timeout
    let result = tokio::time::timeout(
        timeout,
        session.exec_with_stdin(
            &["python3", "/usr/local/bin/execute-action"],
            code.as_bytes(),
        ),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            // Parse the JSON response from the executor
            parse_executor_response(&output)
        }
        Ok(Err(e)) => {
            // Docker exec itself failed
            warn!("execute-action docker exec failed: {e}");
            Ok(ExecutionResult {
                success: false,
                error: Some(format!("Docker exec error: {e}")),
                output: None,
                duration_ms: 0,
            })
        }
        Err(_) => {
            // Timeout exceeded - kill the executor process
            warn!("Code execution timed out after {:?}", timeout);
            // Try to kill any lingering execute-action processes
            let _ = session
                .exec(&["bash", "-c", "pkill -f execute-action || true"])
                .await;
            Ok(ExecutionResult {
                success: false,
                error: Some(format!(
                    "Execution timed out after {} seconds",
                    timeout.as_secs()
                )),
                output: None,
                duration_ms: timeout.as_millis() as u64,
            })
        }
    }
}

/// Parse the JSON output from the execute-action script.
fn parse_executor_response(output: &str) -> Result<ExecutionResult, AppError> {
    let trimmed = output.trim();

    // The executor might print warnings or other output before the JSON.
    // Find the last JSON object in the output.
    if let Some(json_start) = trimmed.rfind('{') {
        let json_str = &trimmed[json_start..];
        match serde_json::from_str::<ExecutionResult>(json_str) {
            Ok(result) => {
                debug!(
                    "Execution result: success={}, error={:?}, duration={}ms",
                    result.success, result.error, result.duration_ms
                );
                Ok(result)
            }
            Err(e) => {
                warn!("Failed to parse executor JSON: {e}, output: {trimmed}");
                Ok(ExecutionResult {
                    success: false,
                    error: Some(format!("Failed to parse executor response: {e}")),
                    output: None,
                    duration_ms: 0,
                })
            }
        }
    } else {
        warn!("No JSON found in executor output: {trimmed}");
        Ok(ExecutionResult {
            success: false,
            error: Some(format!("No JSON in executor output: {trimmed}")),
            output: None,
            duration_ms: 0,
        })
    }
}

/// Process a full LLM response turn: parse, detect commands, execute code blocks.
///
/// Returns a `TurnResult` with the combined outcome of all executions.
pub async fn process_turn(
    session: &DockerSession,
    llm_response: &str,
    step_timeout: Option<Duration>,
    bash_enabled: bool,
) -> Result<TurnResult, AppError> {
    let parsed = parse_response(llm_response);

    info!(
        "Parsed LLM response: command={:?}, code_blocks={}, bash_blocks={}",
        parsed.command,
        parsed.code_blocks.len(),
        parsed.bash_blocks.len(),
    );

    // If there's a special command and no code, return immediately
    if parsed.command.is_some() && parsed.code_blocks.is_empty() && parsed.bash_blocks.is_empty() {
        return Ok(TurnResult {
            command: parsed.command,
            executions: vec![],
            all_succeeded: true,
            error_feedback: None,
            bash_output: None,
            bug_reports: parsed.bug_reports,
        });
    }

    let mut executions = Vec::new();
    let mut all_succeeded = true;
    let mut error_feedback = None;
    let mut bash_outputs: Vec<String> = Vec::new();

    // Execute bash blocks first (debugging/investigation before action)
    if bash_enabled {
        for (i, code) in parsed.bash_blocks.iter().enumerate() {
            debug!("Executing bash block {} of {}", i + 1, parsed.bash_blocks.len());
            let result = execute_bash_code(session, code, step_timeout).await?;

            if !result.success {
                all_succeeded = false;
                let err_msg = result.error.as_deref().unwrap_or("Unknown error");
                let feedback = format!("Bash block {} failed: {}", i + 1, err_msg);
                warn!("{}", feedback);
                if error_feedback.is_none() {
                    error_feedback = Some(feedback);
                }
            }

            // Always capture bash output (success or failure) so the agent can see results
            if let Some(ref output) = result.output {
                bash_outputs.push(format!("$ bash block {}:\n{}", i + 1, output));
            }

            executions.push(result);
        }
    } else if !parsed.bash_blocks.is_empty() {
        warn!("Agent emitted bash blocks but bash is not enabled — ignoring");
    }

    // Execute Python/PyAutoGUI code blocks
    for (i, code) in parsed.code_blocks.iter().enumerate() {
        debug!("Executing code block {} of {}", i + 1, parsed.code_blocks.len());
        let result = execute_code(session, code, step_timeout).await?;

        if !result.success {
            all_succeeded = false;
            let err_msg = result
                .error
                .as_deref()
                .unwrap_or("Unknown error");
            let feedback = format!(
                "Code block {} failed: {}",
                i + 1,
                err_msg,
            );
            warn!("{}", feedback);
            if error_feedback.is_none() {
                error_feedback = Some(feedback);
            }
        }

        executions.push(result);
    }

    let bash_output = if bash_outputs.is_empty() {
        None
    } else {
        Some(bash_outputs.join("\n"))
    };

    Ok(TurnResult {
        command: parsed.command,
        executions,
        all_succeeded,
        error_feedback,
        bash_output,
        bug_reports: parsed.bug_reports,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_response tests ---

    #[test]
    fn test_parse_empty_response() {
        let parsed = parse_response("");
        assert_eq!(parsed.command, None);
        assert!(parsed.code_blocks.is_empty());
    }

    #[test]
    fn test_parse_text_only_response() {
        let parsed = parse_response("I need to click the button at (100, 200).");
        assert_eq!(parsed.command, None);
        assert!(parsed.code_blocks.is_empty());
    }

    #[test]
    fn test_parse_single_code_block() {
        let text = r#"I'll click the button.

```python
pyautogui.click(100, 200)
```
"#;
        let parsed = parse_response(text);
        assert_eq!(parsed.command, None);
        assert_eq!(parsed.code_blocks.len(), 1);
        assert_eq!(parsed.code_blocks[0], "pyautogui.click(100, 200)");
    }

    #[test]
    fn test_parse_multiple_code_blocks() {
        let text = r#"First I'll move, then click.

```python
pyautogui.moveTo(100, 200)
```

Now the click:

```python
pyautogui.click()
time.sleep(0.5)
```
"#;
        let parsed = parse_response(text);
        assert_eq!(parsed.command, None);
        assert_eq!(parsed.code_blocks.len(), 2);
        assert_eq!(parsed.code_blocks[0], "pyautogui.moveTo(100, 200)");
        assert!(parsed.code_blocks[1].contains("pyautogui.click()"));
        assert!(parsed.code_blocks[1].contains("time.sleep(0.5)"));
    }

    #[test]
    fn test_parse_code_block_with_py_fence() {
        let text = "```py\npyautogui.press('enter')\n```";
        let parsed = parse_response(text);
        assert_eq!(parsed.code_blocks.len(), 1);
        assert_eq!(parsed.code_blocks[0], "pyautogui.press('enter')");
    }

    #[test]
    fn test_parse_code_block_preserves_multiline() {
        let text = r#"```python
import time
pyautogui.click(100, 200)
time.sleep(1)
pyautogui.typewrite('hello')
```"#;
        let parsed = parse_response(text);
        assert_eq!(parsed.code_blocks.len(), 1);
        let code = &parsed.code_blocks[0];
        assert!(code.contains("import time"));
        assert!(code.contains("pyautogui.click(100, 200)"));
        assert!(code.contains("time.sleep(1)"));
        assert!(code.contains("pyautogui.typewrite('hello')"));
    }

    #[test]
    fn test_ignore_non_python_code_blocks() {
        let text = "```bash\necho hello\n```\n```python\npyautogui.click()\n```";
        let parsed = parse_response(text);
        assert_eq!(parsed.code_blocks.len(), 1);
        assert_eq!(parsed.code_blocks[0], "pyautogui.click()");
    }

    #[test]
    fn test_empty_code_block_ignored() {
        let text = "```python\n\n```";
        let parsed = parse_response(text);
        assert!(parsed.code_blocks.is_empty());
    }

    // --- detect_special_command tests ---

    #[test]
    fn test_detect_wait_command() {
        assert_eq!(
            detect_special_command("WAIT"),
            Some(SpecialCommand::Wait)
        );
    }

    #[test]
    fn test_detect_done_command() {
        assert_eq!(
            detect_special_command("DONE"),
            Some(SpecialCommand::Done)
        );
    }

    #[test]
    fn test_detect_fail_command() {
        assert_eq!(
            detect_special_command("FAIL"),
            Some(SpecialCommand::Fail)
        );
    }

    #[test]
    fn test_detect_command_with_surrounding_text() {
        let text = "I've completed the task.\nDONE\nAll looks good.";
        assert_eq!(
            detect_special_command(text),
            Some(SpecialCommand::Done)
        );
    }

    #[test]
    fn test_no_command_in_regular_text() {
        assert_eq!(detect_special_command("I'm not done yet"), None);
    }

    #[test]
    fn test_command_not_detected_as_substring() {
        // "WAITING" should not match WAIT
        assert_eq!(detect_special_command("WAITING for input"), None);
    }

    #[test]
    fn test_command_with_whitespace() {
        assert_eq!(
            detect_special_command("  DONE  "),
            Some(SpecialCommand::Done)
        );
    }

    #[test]
    fn test_command_with_code_block() {
        // Command should still be detected even with code blocks
        let text = "```python\npyautogui.click()\n```\nDONE";
        let parsed = parse_response(text);
        assert_eq!(parsed.command, Some(SpecialCommand::Done));
        assert_eq!(parsed.code_blocks.len(), 1);
    }

    // --- parse_executor_response tests ---

    #[test]
    fn test_parse_success_response() {
        let json = r#"{"success": true, "error": null, "duration_ms": 150}"#;
        let result = parse_executor_response(json).unwrap();
        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.duration_ms, 150);
    }

    #[test]
    fn test_parse_error_response() {
        let json = r#"{"success": false, "error": "NameError: name 'foo' is not defined", "duration_ms": 5}"#;
        let result = parse_executor_response(json).unwrap();
        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("NameError: name 'foo' is not defined")
        );
    }

    #[test]
    fn test_parse_response_with_prefix_output() {
        // Sometimes the script might print warnings before the JSON
        let output = "Warning: something\n{\"success\": true, \"error\": null, \"duration_ms\": 100}";
        let result = parse_executor_response(output).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_parse_no_json_output() {
        let result = parse_executor_response("no json here").unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_executor_response("{not valid json}").unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    // --- Integration-style tests for ParsedResponse ---

    #[test]
    fn test_full_llm_response_with_reflection_and_code() {
        let text = r#"Looking at the screenshot, I can see the text editor is open with a blank document.
I need to type "Hello World" into the editor.

```python
pyautogui.click(640, 400)
time.sleep(0.5)
pyautogui.typewrite('Hello World', interval=0.05)
```

This should type the text into the editor."#;

        let parsed = parse_response(text);
        assert_eq!(parsed.command, None);
        assert_eq!(parsed.code_blocks.len(), 1);
        assert!(parsed.code_blocks[0].contains("pyautogui.click(640, 400)"));
        assert!(parsed.code_blocks[0].contains("pyautogui.typewrite"));
    }

    #[test]
    fn test_wait_command_response() {
        let text = "The application is still loading. I need to wait.\n\nWAIT";
        let parsed = parse_response(text);
        assert_eq!(parsed.command, Some(SpecialCommand::Wait));
        assert!(parsed.code_blocks.is_empty());
    }

    #[test]
    fn test_done_with_explanation() {
        let text = "I have successfully completed the task. The file has been saved.\n\nDONE";
        let parsed = parse_response(text);
        assert_eq!(parsed.command, Some(SpecialCommand::Done));
        assert!(parsed.code_blocks.is_empty());
    }

    #[test]
    fn test_fail_with_explanation() {
        let text = "The required button does not exist in this application version.\n\nFAIL";
        let parsed = parse_response(text);
        assert_eq!(parsed.command, Some(SpecialCommand::Fail));
        assert!(parsed.code_blocks.is_empty());
    }

    // --- extract_bash_blocks tests ---

    #[test]
    fn test_extract_single_bash_block() {
        let text = "Let me check the process.\n\n```bash\nps aux | grep myapp\n```";
        let parsed = parse_response(text);
        assert_eq!(parsed.bash_blocks.len(), 1);
        assert_eq!(parsed.bash_blocks[0], "ps aux | grep myapp");
    }

    #[test]
    fn test_extract_sh_block() {
        let text = "```sh\nls -la /tmp\n```";
        let parsed = parse_response(text);
        assert_eq!(parsed.bash_blocks.len(), 1);
        assert_eq!(parsed.bash_blocks[0], "ls -la /tmp");
    }

    #[test]
    fn test_extract_multiple_bash_blocks() {
        let text = "```bash\nps aux\n```\n\nNow checking logs:\n\n```bash\ncat /tmp/app.log\n```";
        let parsed = parse_response(text);
        assert_eq!(parsed.bash_blocks.len(), 2);
        assert_eq!(parsed.bash_blocks[0], "ps aux");
        assert_eq!(parsed.bash_blocks[1], "cat /tmp/app.log");
    }

    #[test]
    fn test_empty_bash_block_ignored() {
        let text = "```bash\n\n```";
        let parsed = parse_response(text);
        assert!(parsed.bash_blocks.is_empty());
    }

    #[test]
    fn test_bash_and_python_blocks_both_extracted() {
        let text = "```bash\nps aux\n```\n\n```python\npyautogui.click(100, 200)\n```";
        let parsed = parse_response(text);
        assert_eq!(parsed.bash_blocks.len(), 1);
        assert_eq!(parsed.bash_blocks[0], "ps aux");
        assert_eq!(parsed.code_blocks.len(), 1);
        assert_eq!(parsed.code_blocks[0], "pyautogui.click(100, 200)");
    }

    // --- extract_bug_reports tests ---

    #[test]
    fn test_extract_single_bug_report() {
        let text = "I noticed something wrong.\n\nBUG\nThe save dialog does not preserve the file extension.\nExpected .txt but got no extension.";
        let reports = extract_bug_reports(text);
        assert_eq!(reports.len(), 1);
        assert!(reports[0].contains("save dialog"));
        assert!(reports[0].contains("Expected .txt"));
    }

    #[test]
    fn test_extract_bug_report_with_code() {
        let text = "I found a bug.\n\nBUG\nButton is misaligned.\n\n```python\npyautogui.click(100, 200)\n```";
        let parsed = parse_response(text);
        assert_eq!(parsed.bug_reports.len(), 1);
        assert!(parsed.bug_reports[0].contains("misaligned"));
        assert_eq!(parsed.code_blocks.len(), 1);
    }

    #[test]
    fn test_extract_bug_report_with_done() {
        let text = "Found a bug but task is complete.\n\nBUG\nCrash on save.\n\nDONE";
        let parsed = parse_response(text);
        assert_eq!(parsed.bug_reports.len(), 1);
        assert!(parsed.bug_reports[0].contains("Crash on save"));
        assert_eq!(parsed.command, Some(SpecialCommand::Done));
    }

    #[test]
    fn test_extract_multiple_bug_reports() {
        let text = "BUG\nFirst bug description.\n\nSome text.\n\nBUG\nSecond bug description.";
        let reports = extract_bug_reports(text);
        assert_eq!(reports.len(), 2);
        assert!(reports[0].contains("First bug"));
        assert!(reports[1].contains("Second bug"));
    }

    #[test]
    fn test_no_bug_reports_in_normal_response() {
        let text = "I'll click the button.\n\n```python\npyautogui.click(100, 200)\n```";
        let parsed = parse_response(text);
        assert!(parsed.bug_reports.is_empty());
    }

    #[test]
    fn test_bug_not_detected_as_substring() {
        let text = "BUGGING out is not a report.\nDEBUG mode is on.";
        let reports = extract_bug_reports(text);
        assert!(reports.is_empty());
    }

    #[test]
    fn test_bug_inside_code_block_ignored() {
        let text = "```python\nBUG\nprint('not a bug report')\n```\n\nBUG\nActual bug outside code block.";
        let reports = extract_bug_reports(text);
        assert_eq!(reports.len(), 1);
        assert!(reports[0].contains("Actual bug"));
    }
}
