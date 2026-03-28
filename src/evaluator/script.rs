use std::path::Path;
use std::time::Duration;

use tracing::{info, warn};

use super::MetricResult;
use crate::docker::DockerSession;
use crate::error::AppError;
use crate::trajectory::{TrajectoryEntry, TrajectoryLogger, chrono_iso8601_now};

/// script_replay: Copy a Python script into the container, run it, check for REPLAY_COMPLETE.
/// If `screenshots_dir` is provided, copies that directory into the container so that
/// screenshot comparison assertions can find their expected files.
///
/// Also reconstructs a trajectory from per-step markers emitted by the replay script,
/// copying screenshots from the container and writing `trajectory.jsonl` to `artifacts_dir`.
pub(super) async fn evaluate_script_replay(
    session: &DockerSession,
    script_path: &str,
    screenshots_dir: Option<&str>,
    artifacts_dir: &Path,
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    super::validate_host_path(script_path, "script_path")?;

    let host_path = std::path::Path::new(script_path);
    if !host_path.exists() {
        return Err(AppError::Config(format!(
            "Replay script not found: {script_path}"
        )));
    }

    // Copy expected screenshots into container (for --with-screenshots scripts)
    if let Some(dir) = screenshots_dir {
        super::validate_host_path(dir, "screenshots_dir")?;
        let dir_path = std::path::Path::new(dir);
        if dir_path.exists() {
            tokio::time::timeout(eval_timeout, session.copy_into(dir_path, "/home/tester/"))
                .await
                .map_err(|_| {
                    AppError::Agent(format!(
                        "Evaluation copy_into timed out after {}s: screenshots dir",
                        eval_timeout.as_secs()
                    ))
                })??;
            info!("Copied screenshots from {} into container", dir);
        } else {
            warn!("Screenshots directory not found: {dir}");
        }
    }

    // Copy script into container
    tokio::time::timeout(eval_timeout, session.copy_into(host_path, "/home/tester/"))
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "Evaluation copy_into timed out after {}s: {script_path}",
                eval_timeout.as_secs()
            ))
        })??;

    let script_name = host_path
        .file_name()
        .ok_or_else(|| AppError::Infra("No filename in script_path".into()))?
        .to_string_lossy();

    let container_script = format!("/home/tester/{script_name}");

    // Make executable and run
    tokio::time::timeout(
        eval_timeout,
        session.exec(&["chmod", "+x", &container_script]),
    )
    .await
    .map_err(|_| {
        AppError::Agent(format!(
            "Evaluation command timed out after {}s: chmod script",
            eval_timeout.as_secs()
        ))
    })??;
    let (output, exit_code) = tokio::time::timeout(
        eval_timeout,
        session.exec_with_exit_code(&["python3", &container_script]),
    )
    .await
    .map_err(|_| {
        AppError::Agent(format!(
            "Evaluation script timed out after {}s: {script_path}",
            eval_timeout.as_secs()
        ))
    })??;

    let has_complete = output.contains("REPLAY_COMPLETE");
    let passed = exit_code == 0 && has_complete;

    // Reconstruct trajectory from step markers in script output
    let mut steps = parse_replay_steps(&output);
    // Backfill action codes from the script source for older replay scripts
    // that don't emit REPLAY_ACTION markers.
    if steps.iter().any(|s| s.action_code.is_none()) {
        let script_source = std::fs::read_to_string(host_path).unwrap_or_else(|e| {
            warn!("Failed to read script source for action-code backfill: {e}");
            String::new()
        });
        let fallback_codes = extract_action_codes_from_script(&script_source);
        for step in &mut steps {
            if step.action_code.is_none() {
                if let Some(code) = fallback_codes.get(&step.step) {
                    step.action_code = Some(code.clone());
                }
            }
        }
    }
    if !steps.is_empty() {
        write_replay_trajectory(session, artifacts_dir, &steps, passed).await;
    }

    let detail = if passed {
        "Replay script completed successfully".to_string()
    } else if exit_code != 0 {
        format!("Replay script exited with code {exit_code}")
    } else {
        "Replay script did not output REPLAY_COMPLETE".to_string()
    };

    Ok(MetricResult {
        passed,
        metric: "script_replay".to_string(),
        expected: "exit_code=0, REPLAY_COMPLETE in output".to_string(),
        actual: format!("exit_code={exit_code}, complete={has_complete}"),
        detail,
    })
}

/// A parsed replay step from the script output.
struct ReplayStep {
    step: usize,
    thought: String,
    action_code: Option<String>,
}

/// Parse `REPLAY_STEP_DONE:N:thought` and `REPLAY_ACTION:N:base64` markers from script output.
fn parse_replay_steps(output: &str) -> Vec<ReplayStep> {
    use base64::Engine;

    let mut steps = Vec::new();
    // Collect action codes keyed by step number
    let mut action_codes: std::collections::HashMap<usize, String> =
        std::collections::HashMap::new();

    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("REPLAY_STEP_DONE:") {
            if let Some((num_str, thought)) = rest.split_once(':') {
                if let Ok(step) = num_str.parse::<usize>() {
                    steps.push(ReplayStep {
                        step,
                        thought: thought.to_string(),
                        action_code: None,
                    });
                }
            }
        } else if let Some(rest) = line.strip_prefix("REPLAY_ACTION:") {
            if let Some((num_str, b64)) = rest.split_once(':') {
                if let Ok(step) = num_str.parse::<usize>() {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.trim())
                    {
                        if let Ok(code) = String::from_utf8(bytes) {
                            action_codes.insert(step, code);
                        }
                    }
                }
            }
        }
    }

    // Attach action codes to their corresponding steps.
    // Note: `remove` pops the value, so if a step number appears more than once
    // only the first ReplayStep gets the code. This is fine because the generated
    // script emits each step marker exactly once (codify deduplicates in advance).
    for step in &mut steps {
        step.action_code = action_codes.remove(&step.step);
    }

    steps
}

/// Extract action codes from `def step_NNN():` functions in a replay script.
///
/// This is a fallback for older replay scripts that don't emit `REPLAY_ACTION` markers.
/// Parses the Python source, finds each `def step_NNN():` block, skips the docstring,
/// and collects the remaining indented body as action code.
fn extract_action_codes_from_script(source: &str) -> std::collections::HashMap<usize, String> {
    use std::collections::HashMap;

    let mut codes: HashMap<usize, String> = HashMap::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Match `def step_NNN():` pattern
        if let Some(rest) = trimmed.strip_prefix("def step_") {
            if let Some(num_str) = rest.strip_suffix("():") {
                if let Ok(step_num) = num_str.parse::<usize>() {
                    i += 1;
                    // Skip docstring (triple-quoted, may be single or multi-line)
                    if i < lines.len() && lines[i].trim().starts_with("\"\"\"") {
                        if lines[i].trim().ends_with("\"\"\"") && lines[i].trim().len() > 3 {
                            // Single-line docstring
                            i += 1;
                        } else {
                            // Multi-line docstring: skip until closing """
                            i += 1;
                            while i < lines.len() && !lines[i].trim().ends_with("\"\"\"") {
                                i += 1;
                            }
                            i += 1; // skip the closing """ line
                        }
                    }
                    // Skip screenshot comparison lines (from --with-screenshots codify)
                    while i < lines.len() {
                        let body = lines[i].trim();
                        if body.is_empty()
                            || body.starts_with("# Verify pre-action")
                            || body.starts_with("time.sleep(0.5)  # Wait for UI")
                            || body.starts_with("subprocess.run(['scrot'")
                            || body.starts_with(
                                "subprocess.run(['python3', '/usr/local/bin/screenshot-compare'",
                            )
                            || body.starts_with("'--expected'")
                        {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    // Collect indented body lines as action code
                    let mut code_lines = Vec::new();
                    while i < lines.len() {
                        let body_line = lines[i];
                        // Stop at next function def or non-indented line
                        if !body_line.is_empty()
                            && !body_line.starts_with(' ')
                            && !body_line.starts_with('\t')
                        {
                            break;
                        }
                        // Also stop at blank line followed by non-indented (function boundary)
                        if body_line.trim().is_empty()
                            && i + 1 < lines.len()
                            && !lines[i + 1].starts_with(' ')
                            && !lines[i + 1].starts_with('\t')
                            && !lines[i + 1].trim().is_empty()
                        {
                            break;
                        }
                        code_lines.push(body_line);
                        i += 1;
                    }
                    // Dedent: remove exactly 4 spaces of indentation (codify standard)
                    let code: String = code_lines
                        .iter()
                        .map(|l| l.strip_prefix("    ").unwrap_or(l))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let code = code.trim_end().to_string();
                    if !code.is_empty() {
                        codes.insert(step_num, code);
                    }
                    continue;
                }
            }
        }
        i += 1;
    }

    codes
}

/// Copy per-step screenshots from the container and append to trajectory.jsonl.
///
/// Uses `TrajectoryLogger::new_append` so that multiple `ScriptReplay` metrics
/// in the same evaluator accumulate entries rather than truncating previous ones.
async fn write_replay_trajectory(
    session: &DockerSession,
    artifacts_dir: &Path,
    steps: &[ReplayStep],
    replay_passed: bool,
) {
    let mut trajectory_logger = match TrajectoryLogger::new_append(artifacts_dir, false, None) {
        Ok(tl) => tl,
        Err(e) => {
            warn!("Failed to create trajectory logger for replay: {e}");
            return;
        }
    };

    for (i, step) in steps.iter().enumerate() {
        let container_screenshot = format!("/tmp/replay_step_{:03}.png", step.step);
        let local_screenshot = artifacts_dir.join(format!("step_{:03}.png", step.step));

        // Copy screenshot from container
        let screenshot_path = match session
            .copy_from(&container_screenshot, &local_screenshot)
            .await
        {
            Ok(()) => Some(format!("step_{:03}.png", step.step)),
            Err(e) => {
                warn!(
                    "Failed to copy replay screenshot for step {}: {e}",
                    step.step
                );
                None
            }
        };

        // All completed steps succeeded. If the replay failed overall, the crash
        // happened *after* the last REPLAY_STEP_DONE marker, so mark it "interrupted"
        // rather than "fail" (which would wrongly blame a step that actually passed).
        let is_last = i == steps.len() - 1;
        let result = if is_last && replay_passed {
            "done"
        } else if is_last && !replay_passed {
            "interrupted"
        } else {
            "success"
        };

        let entry = TrajectoryEntry {
            step: step.step,
            timestamp: chrono_iso8601_now(),
            action_code: step.action_code.clone().unwrap_or_default(),
            thought: Some(step.thought.clone()),
            screenshot_path,
            a11y_tree_path: None,
            result: result.to_string(),
            llm_raw_response: None,
            bash_output: None,
            error_feedback: None,
            action_type: None,
        };
        trajectory_logger.log_entry(&entry);
    }

    info!("Wrote replay trajectory with {} steps", steps.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_replay_steps_with_action_code() {
        use base64::Engine;
        let code = "pyautogui.click(100, 200)";
        let b64 = base64::engine::general_purpose::STANDARD.encode(code);
        let output = format!("REPLAY_STEP_DONE:1:some thought\nREPLAY_ACTION:1:{b64}\n");
        let steps = parse_replay_steps(&output);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].step, 1);
        assert_eq!(steps[0].thought, "some thought");
        assert_eq!(steps[0].action_code.as_deref(), Some(code));
    }

    #[test]
    fn test_parse_replay_steps_without_action_marker() {
        let output = "REPLAY_STEP_DONE:1:click button\nREPLAY_COMPLETE\n";
        let steps = parse_replay_steps(output);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].step, 1);
        assert!(steps[0].action_code.is_none());
    }

    #[test]
    fn test_parse_replay_steps_multiline_action_code() {
        use base64::Engine;
        let code = "import pyautogui\npyautogui.click(100, 200)\npyautogui.press('enter')";
        let b64 = base64::engine::general_purpose::STANDARD.encode(code);
        let output = format!(
            "REPLAY_STEP_DONE:1:step one\nREPLAY_ACTION:1:{b64}\nREPLAY_STEP_DONE:2:step two\nREPLAY_COMPLETE\n"
        );
        let steps = parse_replay_steps(&output);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].action_code.as_deref(), Some(code));
        assert!(steps[1].action_code.is_none());
    }

    #[test]
    fn test_extract_action_codes_basic() {
        let script = r#"def step_001():
    """Click the button"""
    pyautogui.click(100, 200)

def step_002():
    """Type hello"""
    pyautogui.typewrite('hello')
    pyautogui.press('enter')

def main():
    pass
"#;
        let codes = extract_action_codes_from_script(script);
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[&1], "pyautogui.click(100, 200)");
        assert_eq!(
            codes[&2],
            "pyautogui.typewrite('hello')\npyautogui.press('enter')"
        );
    }

    #[test]
    fn test_extract_action_codes_multiline_docstring() {
        let script = r#"def step_001():
    """This is a longer thought
    that spans multiple lines"""
    pyautogui.click(50, 60)

def main():
    pass
"#;
        let codes = extract_action_codes_from_script(script);
        assert_eq!(codes[&1], "pyautogui.click(50, 60)");
    }

    #[test]
    fn test_extract_action_codes_with_screenshot_assertions() {
        let script = r#"def step_001():
    """Click button"""
    # Verify pre-action screen state (threshold: 0.95)
    time.sleep(0.5)  # Wait for UI to settle
    subprocess.run(['scrot', '/tmp/_replay_actual.png'], check=True)
    subprocess.run(['python3', '/usr/local/bin/screenshot-compare',
        '--expected', '/home/tester/artifacts/step_001.png', '--actual', '/tmp/_replay_actual.png', '--threshold', '0.95'], check=True)
    pyautogui.click(100, 200)

def main():
    pass
"#;
        let codes = extract_action_codes_from_script(script);
        assert_eq!(codes[&1], "pyautogui.click(100, 200)");
    }

    #[test]
    fn test_extract_action_codes_empty_script() {
        let codes = extract_action_codes_from_script("");
        assert!(codes.is_empty());
    }

    #[test]
    fn test_extract_action_codes_no_step_functions() {
        let script = "def main():\n    print('hello')\n";
        let codes = extract_action_codes_from_script(script);
        assert!(codes.is_empty());
    }
}
