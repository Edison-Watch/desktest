use std::time::Duration;

use crate::docker::DockerSession;
use crate::error::AppError;
use crate::task::MatchMode;
use super::MetricResult;

/// command_output: Run command in container, check stdout.
pub(super) async fn evaluate_command_output(
    session: &DockerSession,
    command: &str,
    expected: &str,
    match_mode: &MatchMode,
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    let output = tokio::time::timeout(eval_timeout, session.exec(&["bash", "-c", command]))
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "Evaluation command timed out after {}s: {command}",
                eval_timeout.as_secs()
            ))
        })?
        .map_err(|e| AppError::Infra(format!("Failed to run command '{command}': {e}")))?;

    let stdout = output.trim_end();

    let (passed, detail) = match match_mode {
        MatchMode::Contains => {
            if stdout.contains(expected) {
                (true, "Output contains expected string".to_string())
            } else {
                (
                    false,
                    format!("Output does not contain '{expected}'. Got: '{stdout}'"),
                )
            }
        }
        MatchMode::Equals => {
            let trimmed_expected = expected.trim_end();
            if stdout == trimmed_expected {
                (true, "Output matches expected string".to_string())
            } else {
                (
                    false,
                    format!("Output does not equal expected. Got: '{stdout}'"),
                )
            }
        }
        MatchMode::Regex => match regex::Regex::new(expected) {
            Ok(re) => {
                if re.is_match(stdout) {
                    (true, "Output matches regex pattern".to_string())
                } else {
                    (
                        false,
                        format!("Output does not match regex '{expected}'. Got: '{stdout}'"),
                    )
                }
            }
            Err(e) => (false, format!("Invalid regex '{expected}': {e}")),
        },
    };

    Ok(MetricResult {
        passed,
        metric: "command_output".to_string(),
        expected: format!("{expected} ({match_mode:?})"),
        actual: stdout.to_string(),
        detail,
    })
}

/// file_exists: Check if a file exists in the container.
pub(super) async fn evaluate_file_exists(
    session: &DockerSession,
    path: &str,
    should_not_exist: bool,
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    let check_cmd = format!("test -e {} && echo EXISTS || echo MISSING", shell_escape::escape(path.into()));
    let output = tokio::time::timeout(eval_timeout, session.exec(&["bash", "-c", &check_cmd]))
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "Evaluation command timed out after {}s: file_exists check for {path}",
                eval_timeout.as_secs()
            ))
        })?
        .map_err(|e| AppError::Infra(format!("Failed to check file '{path}': {e}")))?;

    let exists = output.trim().contains("EXISTS");

    let (passed, detail) = if should_not_exist {
        if exists {
            (false, format!("File '{path}' exists but should not"))
        } else {
            (true, format!("File '{path}' does not exist (as expected)"))
        }
    } else if exists {
        (true, format!("File '{path}' exists"))
    } else {
        (false, format!("File '{path}' does not exist"))
    };

    Ok(MetricResult {
        passed,
        metric: "file_exists".to_string(),
        expected: if should_not_exist {
            format!("{path} should NOT exist")
        } else {
            format!("{path} should exist")
        },
        actual: if exists {
            "exists".to_string()
        } else {
            "missing".to_string()
        },
        detail,
    })
}

/// exit_code: Run command and check exit code.
pub(super) async fn evaluate_exit_code(
    session: &DockerSession,
    command: &str,
    expected: i32,
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    // Run the command and capture the exit code via $?
    let exit_cmd = format!("{command}; echo \"EXIT_CODE:$?\"");
    let output = tokio::time::timeout(eval_timeout, session.exec(&["bash", "-c", &exit_cmd]))
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "Evaluation command timed out after {}s: {command}",
                eval_timeout.as_secs()
            ))
        })?
        .map_err(|e| AppError::Infra(format!("Failed to run command '{command}': {e}")))?;

    // Parse exit code from the output
    let actual_code = output
        .lines()
        .rev()
        .find_map(|line| {
            line.trim()
                .strip_prefix("EXIT_CODE:")
                .and_then(|code| code.parse::<i32>().ok())
        })
        .unwrap_or(-1);

    let passed = actual_code == expected;
    let detail = if passed {
        format!("Exit code {actual_code} matches expected")
    } else {
        format!("Exit code {actual_code} does not match expected {expected}")
    };

    Ok(MetricResult {
        passed,
        metric: "exit_code".to_string(),
        expected: expected.to_string(),
        actual: actual_code.to_string(),
        detail,
    })
}
