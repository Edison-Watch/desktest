mod command;
mod file_compare;
mod script;

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::docker::DockerSession;
use crate::error::AppError;
use crate::task::{Conjunction, EvaluatorConfig, EvaluatorMode, MetricConfig};

/// Default timeout for individual evaluator exec calls (seconds).
const DEFAULT_EVAL_TIMEOUT_SECS: u64 = 120;

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
    let eval_timeout = Duration::from_secs(
        evaluator
            .eval_timeout_secs
            .unwrap_or(DEFAULT_EVAL_TIMEOUT_SECS),
    );

    info!(
        "Running programmatic evaluation ({} metrics, conjunction: {:?}, timeout: {}s)",
        evaluator.metrics.len(),
        evaluator.conjunction,
        eval_timeout.as_secs()
    );

    let mut metric_results = Vec::new();

    for (i, metric) in evaluator.metrics.iter().enumerate() {
        debug!("Evaluating metric {i}: {}", metric_type_name(metric));
        let result = evaluate_metric(session, metric, artifacts_dir, eval_timeout).await;
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
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    match metric {
        MetricConfig::FileCompare {
            actual_path,
            expected_path,
            compare_mode,
        } => {
            file_compare::evaluate_file_compare(
                session,
                actual_path,
                expected_path,
                compare_mode,
                artifacts_dir,
                eval_timeout,
            )
            .await
        }
        MetricConfig::FileCompareSemantic {
            actual_path,
            expected_path,
            format,
        } => {
            file_compare::evaluate_file_compare_semantic(
                session,
                actual_path,
                expected_path,
                format,
                artifacts_dir,
                eval_timeout,
            )
            .await
        }
        MetricConfig::CommandOutput {
            command,
            expected,
            match_mode,
        } => {
            command::evaluate_command_output(session, command, expected, match_mode, eval_timeout)
                .await
        }
        MetricConfig::FileExists {
            path,
            should_not_exist,
        } => command::evaluate_file_exists(session, path, *should_not_exist, eval_timeout).await,
        MetricConfig::ExitCode { command, expected } => {
            command::evaluate_exit_code(session, command, *expected, eval_timeout).await
        }
        MetricConfig::ScriptReplay {
            script_path,
            screenshots_dir,
        } => {
            script::evaluate_script_replay(
                session,
                script_path,
                screenshots_dir.as_deref(),
                artifacts_dir,
                eval_timeout,
            )
            .await
        }
    }
}

/// Validate that a host path from task JSON doesn't escape outside the current
/// working directory. Canonicalizes the path and checks it starts with cwd.
/// This prevents task JSON from reading arbitrary files via `expected_path`,
/// `script_path`, etc.
pub(super) fn validate_host_path(path: &str, field_name: &str) -> Result<(), AppError> {
    let p = std::path::Path::new(path);
    let canonical = std::fs::canonicalize(p)
        .map_err(|e| AppError::Config(format!("Cannot resolve {field_name} '{path}': {e}")))?;
    let cwd = std::env::current_dir()
        .map_err(|e| AppError::Infra(format!("Cannot determine working directory: {e}")))?;
    let cwd_canonical = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    if !canonical.starts_with(&cwd_canonical) {
        return Err(AppError::Config(format!(
            "{field_name} '{}' resolves to '{}' which is outside the working directory '{}'",
            path,
            canonical.display(),
            cwd_canonical.display()
        )));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{CompareMode, MatchMode, SemanticFormat};

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
}
