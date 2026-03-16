use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::docker::DockerSession;
use crate::error::AppError;
use crate::task::{
    CompareMode, Conjunction, EvaluatorConfig, EvaluatorMode, MatchMode, MetricConfig,
    SemanticFormat,
};

/// The result of evaluating a single metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricResult {
    pub passed: bool,
    pub metric: String,
    pub expected: String,
    pub actual: String,
    pub detail: String,
}

/// The combined result of all programmatic evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub passed: bool,
    pub mode: String,
    pub metric_results: Vec<MetricResult>,
}

/// Run programmatic evaluation against the container state.
///
/// Evaluates all metrics defined in the evaluator config and combines results
/// using the specified conjunction (and/or).
pub async fn run_evaluation(
    session: &DockerSession,
    evaluator: &EvaluatorConfig,
    artifacts_dir: &Path,
) -> Result<EvaluationResult, AppError> {
    info!(
        "Running programmatic evaluation ({} metrics, conjunction: {:?})",
        evaluator.metrics.len(),
        evaluator.conjunction
    );

    let mut metric_results = Vec::new();

    for (i, metric) in evaluator.metrics.iter().enumerate() {
        debug!("Evaluating metric {i}: {}", metric_type_name(metric));
        let result = evaluate_metric(session, metric, artifacts_dir).await;
        match result {
            Ok(mr) => {
                if mr.passed {
                    info!("  Metric {i} ({}) PASSED", mr.metric);
                } else {
                    warn!("  Metric {i} ({}) FAILED: {}", mr.metric, mr.detail);
                }
                metric_results.push(mr);
            }
            Err(e) => {
                warn!("  Metric {i} ({}) ERROR: {e}", metric_type_name(metric));
                metric_results.push(MetricResult {
                    passed: false,
                    metric: metric_type_name(metric).to_string(),
                    expected: String::new(),
                    actual: String::new(),
                    detail: format!("Evaluation error: {e}"),
                });
            }
        }
    }

    let passed = combine_results(&metric_results, &evaluator.conjunction);

    Ok(EvaluationResult {
        passed,
        mode: match evaluator.mode {
            EvaluatorMode::Llm => "llm".to_string(),
            EvaluatorMode::Programmatic => "programmatic".to_string(),
            EvaluatorMode::Hybrid => "hybrid".to_string(),
        },
        metric_results,
    })
}

/// Combine metric results using the specified conjunction.
pub fn combine_results(results: &[MetricResult], conjunction: &Conjunction) -> bool {
    if results.is_empty() {
        return true;
    }

    match conjunction {
        Conjunction::And => results.iter().all(|r| r.passed),
        Conjunction::Or => results.iter().any(|r| r.passed),
    }
}

/// Evaluate a single metric against the container state.
async fn evaluate_metric(
    session: &DockerSession,
    metric: &MetricConfig,
    artifacts_dir: &Path,
) -> Result<MetricResult, AppError> {
    match metric {
        MetricConfig::FileCompare {
            actual_path,
            expected_path,
            compare_mode,
        } => evaluate_file_compare(session, actual_path, expected_path, compare_mode, artifacts_dir).await,
        MetricConfig::FileCompareSemantic {
            actual_path,
            expected_path,
            format,
        } => evaluate_file_compare_semantic(session, actual_path, expected_path, format, artifacts_dir).await,
        MetricConfig::CommandOutput {
            command,
            expected,
            match_mode,
        } => evaluate_command_output(session, command, expected, match_mode).await,
        MetricConfig::FileExists {
            path,
            should_not_exist,
        } => evaluate_file_exists(session, path, *should_not_exist).await,
        MetricConfig::ExitCode { command, expected } => {
            evaluate_exit_code(session, command, *expected).await
        }
        MetricConfig::ScriptReplay { script_path, screenshots_dir } => {
            evaluate_script_replay(session, script_path, screenshots_dir.as_deref()).await
        }
    }
}

/// Get the type name for a metric (for logging/reporting).
fn metric_type_name(metric: &MetricConfig) -> &'static str {
    match metric {
        MetricConfig::FileCompare { .. } => "file_compare",
        MetricConfig::FileCompareSemantic { .. } => "file_compare_semantic",
        MetricConfig::CommandOutput { .. } => "command_output",
        MetricConfig::FileExists { .. } => "file_exists",
        MetricConfig::ExitCode { .. } => "exit_code",
        MetricConfig::ScriptReplay { .. } => "script_replay",
    }
}

/// file_compare: Copy file from container, compare against expected file.
async fn evaluate_file_compare(
    session: &DockerSession,
    actual_path: &str,
    expected_path: &str,
    compare_mode: &CompareMode,
    artifacts_dir: &Path,
) -> Result<MetricResult, AppError> {
    // Copy the actual file from the container to a temp location
    let temp_actual = artifacts_dir.join("eval_actual_file");
    session.copy_from(actual_path, &temp_actual).await.map_err(|e| {
        AppError::Infra(format!(
            "Failed to copy file '{actual_path}' from container: {e}"
        ))
    })?;

    let actual_bytes = std::fs::read(&temp_actual)
        .map_err(|e| AppError::Infra(format!("Failed to read copied file: {e}")))?;
    let expected_bytes = std::fs::read(expected_path)
        .map_err(|e| AppError::Infra(format!("Failed to read expected file '{expected_path}': {e}")))?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_actual);

    let (passed, detail) = match compare_mode {
        CompareMode::Exact => {
            if actual_bytes == expected_bytes {
                (true, "Files match exactly".to_string())
            } else {
                let actual_len = actual_bytes.len();
                let expected_len = expected_bytes.len();
                (
                    false,
                    format!("Files differ (actual: {actual_len} bytes, expected: {expected_len} bytes)"),
                )
            }
        }
        CompareMode::Normalized => {
            let actual_text = normalize_text(&String::from_utf8_lossy(&actual_bytes));
            let expected_text = normalize_text(&String::from_utf8_lossy(&expected_bytes));
            if actual_text == expected_text {
                (true, "Files match (normalized)".to_string())
            } else {
                let diff = first_diff_line(&actual_text, &expected_text);
                (false, format!("Files differ (normalized). {diff}"))
            }
        }
    };

    Ok(MetricResult {
        passed,
        metric: "file_compare".to_string(),
        expected: format!("file: {expected_path}"),
        actual: format!("container: {actual_path}"),
        detail,
    })
}

/// file_compare_semantic: Parse structured files and compare data structures.
async fn evaluate_file_compare_semantic(
    session: &DockerSession,
    actual_path: &str,
    expected_path: &str,
    format: &SemanticFormat,
    artifacts_dir: &Path,
) -> Result<MetricResult, AppError> {
    // Copy the actual file from the container
    let temp_actual = artifacts_dir.join("eval_semantic_actual");
    session.copy_from(actual_path, &temp_actual).await.map_err(|e| {
        AppError::Infra(format!(
            "Failed to copy file '{actual_path}' from container: {e}"
        ))
    })?;

    let actual_str = std::fs::read_to_string(&temp_actual)
        .map_err(|e| AppError::Infra(format!("Failed to read copied file: {e}")))?;
    let expected_str = std::fs::read_to_string(expected_path)
        .map_err(|e| AppError::Infra(format!("Failed to read expected file '{expected_path}': {e}")))?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_actual);

    let (passed, detail) = match format {
        SemanticFormat::Json => compare_json(&actual_str, &expected_str),
        SemanticFormat::Yaml => compare_yaml(&actual_str, &expected_str),
        SemanticFormat::Xml => compare_xml(&actual_str, &expected_str),
        SemanticFormat::Csv => compare_csv(&actual_str, &expected_str),
    };

    Ok(MetricResult {
        passed,
        metric: "file_compare_semantic".to_string(),
        expected: format!("file: {expected_path} (format: {format:?})"),
        actual: format!("container: {actual_path}"),
        detail,
    })
}

/// command_output: Run command in container, check stdout.
async fn evaluate_command_output(
    session: &DockerSession,
    command: &str,
    expected: &str,
    match_mode: &MatchMode,
) -> Result<MetricResult, AppError> {
    let output = session
        .exec(&["bash", "-c", command])
        .await
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
async fn evaluate_file_exists(
    session: &DockerSession,
    path: &str,
    should_not_exist: bool,
) -> Result<MetricResult, AppError> {
    let output = session
        .exec(&["bash", "-c", &format!("test -e {} && echo EXISTS || echo MISSING", shell_escape::escape(path.into()))])
        .await
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
async fn evaluate_exit_code(
    session: &DockerSession,
    command: &str,
    expected: i32,
) -> Result<MetricResult, AppError> {
    // Run the command and capture the exit code via $?
    let output = session
        .exec(&[
            "bash",
            "-c",
            &format!("{command}; echo \"EXIT_CODE:$?\""),
        ])
        .await
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

/// script_replay: Copy a Python script into the container, run it, check for REPLAY_COMPLETE.
/// If `screenshots_dir` is provided, copies that directory into the container so that
/// screenshot comparison assertions can find their expected files.
async fn evaluate_script_replay(
    session: &DockerSession,
    script_path: &str,
    screenshots_dir: Option<&str>,
) -> Result<MetricResult, AppError> {
    let host_path = std::path::Path::new(script_path);
    if !host_path.exists() {
        return Err(AppError::Config(format!(
            "Replay script not found: {script_path}"
        )));
    }

    // Copy expected screenshots into container (for --with-screenshots scripts)
    if let Some(dir) = screenshots_dir {
        let dir_path = std::path::Path::new(dir);
        if dir_path.exists() {
            session.copy_into(dir_path, "/home/tester/").await?;
            info!("Copied screenshots from {} into container", dir);
        } else {
            warn!("Screenshots directory not found: {dir}");
        }
    }

    // Copy script into container
    session.copy_into(host_path, "/home/tester/").await?;

    let script_name = host_path
        .file_name()
        .ok_or_else(|| AppError::Infra("No filename in script_path".into()))?
        .to_string_lossy();

    let container_script = format!("/home/tester/{script_name}");

    // Make executable and run
    session.exec(&["chmod", "+x", &container_script]).await?;
    let (output, exit_code) = session
        .exec_with_exit_code(&["python3", &container_script])
        .await?;

    let has_complete = output.contains("REPLAY_COMPLETE");
    let passed = exit_code == 0 && has_complete;

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

// --- Comparison helpers ---

/// Normalize text by trimming trailing whitespace on each line and trailing newlines.
fn normalize_text(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// Find the first line where two texts differ.
fn first_diff_line(actual: &str, expected: &str) -> String {
    let actual_lines: Vec<&str> = actual.lines().collect();
    let expected_lines: Vec<&str> = expected.lines().collect();

    for (i, (a, e)) in actual_lines.iter().zip(expected_lines.iter()).enumerate() {
        if a != e {
            return format!(
                "First difference at line {}: actual='{}', expected='{}'",
                i + 1,
                a,
                e
            );
        }
    }

    if actual_lines.len() != expected_lines.len() {
        format!(
            "Files have different number of lines (actual: {}, expected: {})",
            actual_lines.len(),
            expected_lines.len()
        )
    } else {
        "Files are identical".to_string()
    }
}

/// Compare two JSON strings semantically (ignoring key order, whitespace).
fn compare_json(actual: &str, expected: &str) -> (bool, String) {
    let actual_val: Result<serde_json::Value, _> = serde_json::from_str(actual);
    let expected_val: Result<serde_json::Value, _> = serde_json::from_str(expected);

    match (actual_val, expected_val) {
        (Ok(a), Ok(e)) => {
            if a == e {
                (true, "JSON data structures match".to_string())
            } else {
                (false, "JSON data structures differ".to_string())
            }
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as JSON: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as JSON: {e}")),
    }
}

/// Compare two YAML strings semantically.
fn compare_yaml(actual: &str, expected: &str) -> (bool, String) {
    let actual_val: Result<serde_yaml::Value, _> = serde_yaml::from_str(actual);
    let expected_val: Result<serde_yaml::Value, _> = serde_yaml::from_str(expected);

    match (actual_val, expected_val) {
        (Ok(a), Ok(e)) => {
            if a == e {
                (true, "YAML data structures match".to_string())
            } else {
                (false, "YAML data structures differ".to_string())
            }
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as YAML: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as YAML: {e}")),
    }
}

/// Compare two XML strings semantically.
///
/// Parses XML into serde_json::Value via quick-xml, then compares the resulting
/// data structures. This ignores whitespace differences and attribute ordering.
fn compare_xml(actual: &str, expected: &str) -> (bool, String) {
    let actual_val = parse_xml_to_value(actual);
    let expected_val = parse_xml_to_value(expected);

    match (actual_val, expected_val) {
        (Ok(a), Ok(e)) => {
            if a == e {
                (true, "XML data structures match".to_string())
            } else {
                (false, "XML data structures differ".to_string())
            }
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as XML: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as XML: {e}")),
    }
}

/// Parse XML string into a serde_json::Value for comparison.
fn parse_xml_to_value(xml: &str) -> Result<serde_json::Value, String> {
    quick_xml::de::from_str(xml).map_err(|e| format!("{e}"))
}

/// Compare two CSV strings semantically (row-by-row, cell-by-cell, ignoring
/// whitespace differences in cell values).
fn compare_csv(actual: &str, expected: &str) -> (bool, String) {
    let actual_rows = parse_csv(actual);
    let expected_rows = parse_csv(expected);

    match (actual_rows, expected_rows) {
        (Ok(a), Ok(e)) => {
            if a.len() != e.len() {
                return (
                    false,
                    format!(
                        "CSV row count differs (actual: {}, expected: {})",
                        a.len(),
                        e.len()
                    ),
                );
            }
            for (row_idx, (ar, er)) in a.iter().zip(e.iter()).enumerate() {
                if ar.len() != er.len() {
                    return (
                        false,
                        format!(
                            "CSV column count differs at row {} (actual: {}, expected: {})",
                            row_idx + 1,
                            ar.len(),
                            er.len()
                        ),
                    );
                }
                for (col_idx, (ac, ec)) in ar.iter().zip(er.iter()).enumerate() {
                    if ac.trim() != ec.trim() {
                        return (
                            false,
                            format!(
                                "CSV cell differs at row {}, col {}: actual='{}', expected='{}'",
                                row_idx + 1,
                                col_idx + 1,
                                ac,
                                ec
                            ),
                        );
                    }
                }
            }
            (true, "CSV data matches".to_string())
        }
        (Err(e), _) => (false, format!("Failed to parse actual file as CSV: {e}")),
        (_, Err(e)) => (false, format!("Failed to parse expected file as CSV: {e}")),
    }
}

/// Parse a CSV string into a Vec of rows, each a Vec of cell strings.
fn parse_csv(csv_str: &str) -> Result<Vec<Vec<String>>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_str.as_bytes());

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| format!("{e}"))?;
        rows.push(record.iter().map(|s| s.to_string()).collect());
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- normalize_text tests ---

    #[test]
    fn test_normalize_text_trims_trailing_whitespace() {
        assert_eq!(normalize_text("hello   \nworld  \n"), "hello\nworld");
    }

    #[test]
    fn test_normalize_text_trims_trailing_newlines() {
        assert_eq!(normalize_text("hello\nworld\n\n\n"), "hello\nworld");
    }

    #[test]
    fn test_normalize_text_preserves_leading_whitespace() {
        assert_eq!(normalize_text("  hello\n  world"), "  hello\n  world");
    }

    // --- first_diff_line tests ---

    #[test]
    fn test_first_diff_identical() {
        let result = first_diff_line("abc\ndef", "abc\ndef");
        assert!(result.contains("identical"));
    }

    #[test]
    fn test_first_diff_different_line() {
        let result = first_diff_line("abc\nXXX", "abc\ndef");
        assert!(result.contains("line 2"));
    }

    #[test]
    fn test_first_diff_different_length() {
        let result = first_diff_line("abc", "abc\ndef");
        assert!(result.contains("different number of lines"));
    }

    // --- compare_json tests ---

    #[test]
    fn test_json_equal_same_order() {
        let (passed, _) = compare_json(r#"{"a":1,"b":2}"#, r#"{"a":1,"b":2}"#);
        assert!(passed);
    }

    #[test]
    fn test_json_equal_different_order() {
        let (passed, _) = compare_json(r#"{"b":2,"a":1}"#, r#"{"a":1,"b":2}"#);
        assert!(passed);
    }

    #[test]
    fn test_json_equal_with_whitespace() {
        let actual = r#"{ "a" : 1, "b" : 2 }"#;
        let expected = r#"{"a":1,"b":2}"#;
        let (passed, _) = compare_json(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_json_not_equal() {
        let (passed, detail) = compare_json(r#"{"a":1}"#, r#"{"a":2}"#);
        assert!(!passed);
        assert!(detail.contains("differ"));
    }

    #[test]
    fn test_json_invalid_actual() {
        let (passed, detail) = compare_json("not json", r#"{"a":1}"#);
        assert!(!passed);
        assert!(detail.contains("parse actual"));
    }

    #[test]
    fn test_json_invalid_expected() {
        let (passed, detail) = compare_json(r#"{"a":1}"#, "not json");
        assert!(!passed);
        assert!(detail.contains("parse expected"));
    }

    // --- compare_yaml tests ---

    #[test]
    fn test_yaml_equal() {
        let actual = "a: 1\nb: 2\n";
        let expected = "b: 2\na: 1\n";
        let (passed, _) = compare_yaml(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_yaml_not_equal() {
        let (passed, _) = compare_yaml("a: 1\n", "a: 2\n");
        assert!(!passed);
    }

    #[test]
    fn test_yaml_invalid() {
        let (passed, detail) = compare_yaml(":\n  :\n    - invalid:", "a: 1\n");
        assert!(!passed);
        assert!(detail.contains("parse"));
    }

    // --- compare_xml tests ---

    #[test]
    fn test_xml_equal() {
        let actual = "<root><a>1</a><b>2</b></root>";
        let expected = "<root><a>1</a><b>2</b></root>";
        let (passed, _) = compare_xml(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_xml_equal_with_whitespace() {
        let actual = "<root>\n  <a>1</a>\n  <b>2</b>\n</root>";
        let expected = "<root><a>1</a><b>2</b></root>";
        let (passed, _) = compare_xml(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_xml_not_equal() {
        let actual = "<root><a>1</a></root>";
        let expected = "<root><a>2</a></root>";
        let (passed, _) = compare_xml(actual, expected);
        assert!(!passed);
    }

    #[test]
    fn test_xml_invalid() {
        let (passed, detail) = compare_xml("<not>closed", "<root/>");
        assert!(!passed);
        assert!(detail.contains("parse"));
    }

    // --- compare_csv tests ---

    #[test]
    fn test_csv_equal() {
        let actual = "a,b,c\n1,2,3\n";
        let expected = "a,b,c\n1,2,3\n";
        let (passed, _) = compare_csv(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_csv_equal_whitespace_trimmed() {
        let actual = "a , b , c\n1 , 2 , 3\n";
        let expected = "a,b,c\n1,2,3\n";
        let (passed, _) = compare_csv(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_csv_different_values() {
        let actual = "a,b,c\n1,X,3\n";
        let expected = "a,b,c\n1,2,3\n";
        let (passed, detail) = compare_csv(actual, expected);
        assert!(!passed);
        assert!(detail.contains("row 2"));
        assert!(detail.contains("col 2"));
    }

    #[test]
    fn test_csv_different_row_count() {
        let actual = "a,b\n1,2\n";
        let expected = "a,b\n1,2\n3,4\n";
        let (passed, detail) = compare_csv(actual, expected);
        assert!(!passed);
        assert!(detail.contains("row count"));
    }

    // --- combine_results tests ---

    #[test]
    fn test_combine_and_all_pass() {
        let results = vec![
            MetricResult {
                passed: true,
                metric: "a".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
            MetricResult {
                passed: true,
                metric: "b".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
        ];
        assert!(combine_results(&results, &Conjunction::And));
    }

    #[test]
    fn test_combine_and_one_fails() {
        let results = vec![
            MetricResult {
                passed: true,
                metric: "a".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
            MetricResult {
                passed: false,
                metric: "b".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
        ];
        assert!(!combine_results(&results, &Conjunction::And));
    }

    #[test]
    fn test_combine_or_one_passes() {
        let results = vec![
            MetricResult {
                passed: false,
                metric: "a".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
            MetricResult {
                passed: true,
                metric: "b".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
        ];
        assert!(combine_results(&results, &Conjunction::Or));
    }

    #[test]
    fn test_combine_or_none_pass() {
        let results = vec![
            MetricResult {
                passed: false,
                metric: "a".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
            MetricResult {
                passed: false,
                metric: "b".into(),
                expected: "".into(),
                actual: "".into(),
                detail: "".into(),
            },
        ];
        assert!(!combine_results(&results, &Conjunction::Or));
    }

    #[test]
    fn test_combine_empty_is_pass() {
        assert!(combine_results(&[], &Conjunction::And));
        assert!(combine_results(&[], &Conjunction::Or));
    }

    // --- MetricResult serialization ---

    #[test]
    fn test_metric_result_serializes() {
        let result = MetricResult {
            passed: true,
            metric: "file_exists".to_string(),
            expected: "/tmp/test".to_string(),
            actual: "exists".to_string(),
            detail: "File exists".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"passed\":true"));
        assert!(json.contains("\"metric\":\"file_exists\""));
    }

    #[test]
    fn test_evaluation_result_serializes() {
        let result = EvaluationResult {
            passed: true,
            mode: "programmatic".to_string(),
            metric_results: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"passed\":true"));
        assert!(json.contains("\"mode\":\"programmatic\""));
    }

    // --- metric_type_name tests ---

    #[test]
    fn test_metric_type_names() {
        assert_eq!(
            metric_type_name(&MetricConfig::FileCompare {
                actual_path: String::new(),
                expected_path: String::new(),
                compare_mode: CompareMode::Exact,
            }),
            "file_compare"
        );
        assert_eq!(
            metric_type_name(&MetricConfig::FileCompareSemantic {
                actual_path: String::new(),
                expected_path: String::new(),
                format: SemanticFormat::Json,
            }),
            "file_compare_semantic"
        );
        assert_eq!(
            metric_type_name(&MetricConfig::CommandOutput {
                command: String::new(),
                expected: String::new(),
                match_mode: MatchMode::Contains,
            }),
            "command_output"
        );
        assert_eq!(
            metric_type_name(&MetricConfig::FileExists {
                path: String::new(),
                should_not_exist: false,
            }),
            "file_exists"
        );
        assert_eq!(
            metric_type_name(&MetricConfig::ExitCode {
                command: String::new(),
                expected: 0,
            }),
            "exit_code"
        );
        assert_eq!(
            metric_type_name(&MetricConfig::ScriptReplay {
                script_path: String::new(),
                screenshots_dir: None,
            }),
            "script_replay"
        );
    }

    // --- parse_csv tests ---

    #[test]
    fn test_parse_csv_basic() {
        let rows = parse_csv("a,b,c\n1,2,3\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a", "b", "c"]);
        assert_eq!(rows[1], vec!["1", "2", "3"]);
    }

    #[test]
    fn test_parse_csv_empty() {
        let rows = parse_csv("").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_parse_csv_quoted_fields() {
        let rows = parse_csv(r#""hello, world",b,c"#).unwrap();
        assert_eq!(rows[0][0], "hello, world");
    }

    // --- JSON nested comparison ---

    #[test]
    fn test_json_nested_equal() {
        let actual = r#"{"outer":{"inner":[1,2,3]},"key":"value"}"#;
        let expected = r#"{"key":"value","outer":{"inner":[1,2,3]}}"#;
        let (passed, _) = compare_json(actual, expected);
        assert!(passed);
    }

    #[test]
    fn test_json_array_order_matters() {
        // JSON arrays are ordered, so different order should fail
        let (passed, _) = compare_json("[1,2,3]", "[3,2,1]");
        assert!(!passed);
    }

    // --- YAML with nested structures ---

    #[test]
    fn test_yaml_nested() {
        let actual = "outer:\n  inner:\n    - 1\n    - 2\n";
        let expected = "outer:\n  inner:\n    - 1\n    - 2\n";
        let (passed, _) = compare_yaml(actual, expected);
        assert!(passed);
    }

    // --- parse_xml_to_value ---

    #[test]
    fn test_parse_xml_simple() {
        let val = parse_xml_to_value("<root><a>1</a></root>").unwrap();
        assert!(val.is_object() || val.is_string());
    }
}
