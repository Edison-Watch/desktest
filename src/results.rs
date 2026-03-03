use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{AgentOutcome, AppError};
use crate::evaluator::{EvaluationResult, MetricResult};

/// Schema version for the results output format.
const RESULTS_SCHEMA_VERSION: &str = "1.0";

/// Default output directory for test results.
pub const DEFAULT_OUTPUT_DIR: &str = "./test-results";

/// Status of a test run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Pass,
    Fail,
    Error,
}

/// Agent verdict details, included when an agent loop was run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVerdict {
    pub passed: bool,
    pub reasoning: String,
    pub steps: usize,
}

/// Structured test result written as results.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub schema_version: String,
    pub test_id: String,
    pub status: TestStatus,
    pub duration_ms: u64,
    pub metric_results: Vec<MetricResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_verdict: Option<AgentVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_detail: Option<String>,
}

/// Build a TestResult from a successful test run (Ok(AgentOutcome)).
pub fn from_outcome(
    test_id: &str,
    outcome: &AgentOutcome,
    eval_result: Option<&EvaluationResult>,
    duration_ms: u64,
) -> TestResult {
    let status = if outcome.passed {
        TestStatus::Pass
    } else {
        TestStatus::Fail
    };

    let agent_verdict = Some(AgentVerdict {
        passed: outcome.passed,
        reasoning: outcome.reasoning.clone(),
        steps: outcome.screenshot_count,
    });

    let metric_results = eval_result
        .map(|e| e.metric_results.clone())
        .unwrap_or_default();

    let (error_category, error_detail) = if !outcome.passed {
        (
            Some("test_failure".to_string()),
            Some(outcome.reasoning.clone()),
        )
    } else {
        (None, None)
    };

    TestResult {
        schema_version: RESULTS_SCHEMA_VERSION.to_string(),
        test_id: test_id.to_string(),
        status,
        duration_ms,
        metric_results,
        agent_verdict,
        error_category,
        error_detail,
    }
}

/// Build a TestResult from a programmatic-only evaluation (no agent loop).
pub fn from_evaluation(
    test_id: &str,
    eval_result: &EvaluationResult,
    duration_ms: u64,
) -> TestResult {
    let status = if eval_result.passed {
        TestStatus::Pass
    } else {
        TestStatus::Fail
    };

    let (error_category, error_detail) = if !eval_result.passed {
        let failures: Vec<String> = eval_result
            .metric_results
            .iter()
            .filter(|m| !m.passed)
            .map(|m| format!("{}: {}", m.metric, m.detail))
            .collect();
        (
            Some("test_failure".to_string()),
            Some(failures.join("; ")),
        )
    } else {
        (None, None)
    };

    TestResult {
        schema_version: RESULTS_SCHEMA_VERSION.to_string(),
        test_id: test_id.to_string(),
        status,
        duration_ms,
        metric_results: eval_result.metric_results.clone(),
        agent_verdict: None,
        error_category,
        error_detail,
    }
}

/// Build a TestResult from an error (Err(AppError)).
pub fn from_error(test_id: &str, error: &AppError, duration_ms: u64) -> TestResult {
    let error_category = match error {
        AppError::Config(_) => "config_error",
        AppError::Infra(_) | AppError::Docker(_) | AppError::Io(_) => "infra_error",
        AppError::Agent(_) | AppError::Http(_) => "agent_error",
    };

    TestResult {
        schema_version: RESULTS_SCHEMA_VERSION.to_string(),
        test_id: test_id.to_string(),
        status: TestStatus::Error,
        duration_ms,
        metric_results: vec![],
        agent_verdict: None,
        error_category: Some(error_category.to_string()),
        error_detail: Some(error.to_string()),
    }
}

/// Write a TestResult as results.json to the specified output directory.
pub fn write_results(result: &TestResult, output_dir: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(output_dir).map_err(|e| {
        AppError::Infra(format!(
            "Cannot create output directory '{}': {e}",
            output_dir.display()
        ))
    })?;

    let results_path = output_dir.join("results.json");
    let json = serde_json::to_string_pretty(result).map_err(|e| {
        AppError::Infra(format!("Failed to serialize results: {e}"))
    })?;

    std::fs::write(&results_path, &json).map_err(|e| {
        AppError::Infra(format!(
            "Failed to write results to '{}': {e}",
            results_path.display()
        ))
    })?;

    info!("Results written to {}", results_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_outcome(passed: bool, reasoning: &str) -> AgentOutcome {
        AgentOutcome {
            passed,
            reasoning: reasoning.into(),
            screenshot_count: 5,
        }
    }

    fn make_eval_result(passed: bool, metrics: Vec<MetricResult>) -> EvaluationResult {
        EvaluationResult {
            passed,
            mode: "programmatic".into(),
            metric_results: metrics,
        }
    }

    fn make_metric(passed: bool, name: &str, detail: &str) -> MetricResult {
        MetricResult {
            passed,
            metric: name.into(),
            expected: "expected".into(),
            actual: "actual".into(),
            detail: detail.into(),
        }
    }

    // --- from_outcome tests ---

    #[test]
    fn test_from_outcome_passed() {
        let outcome = make_outcome(true, "All good");
        let result = from_outcome("test-1", &outcome, None, 1234);

        assert_eq!(result.schema_version, "1.0");
        assert_eq!(result.test_id, "test-1");
        assert_eq!(result.status, TestStatus::Pass);
        assert_eq!(result.duration_ms, 1234);
        assert!(result.metric_results.is_empty());
        assert!(result.agent_verdict.is_some());
        let v = result.agent_verdict.unwrap();
        assert!(v.passed);
        assert_eq!(v.reasoning, "All good");
        assert_eq!(v.steps, 5);
        assert!(result.error_category.is_none());
        assert!(result.error_detail.is_none());
    }

    #[test]
    fn test_from_outcome_failed() {
        let outcome = make_outcome(false, "Button not found");
        let result = from_outcome("test-2", &outcome, None, 5000);

        assert_eq!(result.status, TestStatus::Fail);
        assert_eq!(result.error_category.as_deref(), Some("test_failure"));
        assert_eq!(result.error_detail.as_deref(), Some("Button not found"));
    }

    #[test]
    fn test_from_outcome_with_eval_result() {
        let outcome = make_outcome(true, "Done");
        let metrics = vec![
            make_metric(true, "file_exists", "OK"),
            make_metric(true, "exit_code", "Exit 0"),
        ];
        let eval = make_eval_result(true, metrics);
        let result = from_outcome("test-3", &outcome, Some(&eval), 2000);

        assert_eq!(result.status, TestStatus::Pass);
        assert_eq!(result.metric_results.len(), 2);
        assert!(result.agent_verdict.is_some());
    }

    #[test]
    fn test_from_outcome_hybrid_agent_pass_eval_fail() {
        // In hybrid mode, the combined outcome has passed=false
        let outcome = AgentOutcome {
            passed: false,
            reasoning: "Agent passed: Done. Programmatic evaluation failed (1/1 metrics failed: file_exists: File not found)".into(),
            screenshot_count: 3,
        };
        let metrics = vec![make_metric(false, "file_exists", "File not found")];
        let eval = make_eval_result(false, metrics);
        let result = from_outcome("test-4", &outcome, Some(&eval), 3000);

        assert_eq!(result.status, TestStatus::Fail);
        assert_eq!(result.metric_results.len(), 1);
        assert!(!result.metric_results[0].passed);
    }

    // --- from_evaluation tests ---

    #[test]
    fn test_from_evaluation_passed() {
        let metrics = vec![make_metric(true, "file_exists", "OK")];
        let eval = make_eval_result(true, metrics);
        let result = from_evaluation("test-5", &eval, 800);

        assert_eq!(result.status, TestStatus::Pass);
        assert_eq!(result.metric_results.len(), 1);
        assert!(result.agent_verdict.is_none());
        assert!(result.error_category.is_none());
    }

    #[test]
    fn test_from_evaluation_failed() {
        let metrics = vec![
            make_metric(true, "file_exists", "OK"),
            make_metric(false, "exit_code", "Exit 1"),
        ];
        let eval = make_eval_result(false, metrics);
        let result = from_evaluation("test-6", &eval, 1200);

        assert_eq!(result.status, TestStatus::Fail);
        assert_eq!(result.error_category.as_deref(), Some("test_failure"));
        assert!(result.error_detail.as_ref().unwrap().contains("exit_code"));
    }

    // --- from_error tests ---

    #[test]
    fn test_from_error_config() {
        let err = AppError::Config("bad schema".into());
        let result = from_error("test-7", &err, 100);

        assert_eq!(result.status, TestStatus::Error);
        assert_eq!(result.error_category.as_deref(), Some("config_error"));
        assert!(result.error_detail.as_ref().unwrap().contains("bad schema"));
        assert!(result.metric_results.is_empty());
        assert!(result.agent_verdict.is_none());
    }

    #[test]
    fn test_from_error_infra() {
        let err = AppError::Infra("container crashed".into());
        let result = from_error("test-8", &err, 200);

        assert_eq!(result.status, TestStatus::Error);
        assert_eq!(result.error_category.as_deref(), Some("infra_error"));
    }

    #[test]
    fn test_from_error_agent() {
        let err = AppError::Agent("LLM timeout".into());
        let result = from_error("test-9", &err, 300);

        assert_eq!(result.status, TestStatus::Error);
        assert_eq!(result.error_category.as_deref(), Some("agent_error"));
    }

    // --- Serialization tests ---

    #[test]
    fn test_result_serializes_to_json() {
        let outcome = make_outcome(true, "Done");
        let result = from_outcome("test-10", &outcome, None, 1500);
        let json = serde_json::to_string_pretty(&result).unwrap();

        assert!(json.contains("\"schema_version\": \"1.0\""));
        assert!(json.contains("\"test_id\": \"test-10\""));
        assert!(json.contains("\"status\": \"pass\""));
        assert!(json.contains("\"duration_ms\": 1500"));
        // Optional None fields should not be present
        assert!(!json.contains("error_category"));
        assert!(!json.contains("error_detail"));
    }

    #[test]
    fn test_result_roundtrips() {
        let outcome = make_outcome(false, "Timed out");
        let metrics = vec![make_metric(false, "file_exists", "Missing")];
        let eval = make_eval_result(false, metrics);
        let result = from_outcome("test-11", &outcome, Some(&eval), 5000);

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: TestResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.schema_version, "1.0");
        assert_eq!(deserialized.test_id, "test-11");
        assert_eq!(deserialized.status, TestStatus::Fail);
        assert_eq!(deserialized.duration_ms, 5000);
        assert_eq!(deserialized.metric_results.len(), 1);
        assert!(deserialized.agent_verdict.is_some());
        assert!(deserialized.error_category.is_some());
    }

    #[test]
    fn test_error_result_serializes() {
        let err = AppError::Config("invalid field".into());
        let result = from_error("test-12", &err, 50);
        let json = serde_json::to_string_pretty(&result).unwrap();

        assert!(json.contains("\"status\": \"error\""));
        assert!(json.contains("\"error_category\": \"config_error\""));
        assert!(json.contains("invalid field"));
    }

    // --- write_results tests ---

    #[test]
    fn test_write_results_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome = make_outcome(true, "OK");
        let result = from_outcome("test-write", &outcome, None, 100);

        write_results(&result, tmp.path()).unwrap();

        let path = tmp.path().join("results.json");
        assert!(path.exists());

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: TestResult = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed.test_id, "test-write");
        assert_eq!(parsed.status, TestStatus::Pass);
    }

    #[test]
    fn test_write_results_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("nested").join("dir");
        let outcome = make_outcome(true, "OK");
        let result = from_outcome("test-nested", &outcome, None, 200);

        write_results(&result, &nested).unwrap();

        let path = nested.join("results.json");
        assert!(path.exists());
    }

    // --- Exit code alignment ---

    #[test]
    fn test_status_matches_exit_codes() {
        // TestStatus::Pass should correspond to exit code 0
        // TestStatus::Fail should correspond to exit code 1
        // TestStatus::Error can be 2 (config) or 3 (infra)
        // This test verifies the mapping is correct via from_error
        let config_err = AppError::Config("x".into());
        let infra_err = AppError::Infra("x".into());

        assert_eq!(config_err.exit_code(), 2);
        assert_eq!(infra_err.exit_code(), 3);

        let r1 = from_error("t1", &config_err, 0);
        let r2 = from_error("t2", &infra_err, 0);
        assert_eq!(r1.status, TestStatus::Error);
        assert_eq!(r2.status, TestStatus::Error);
    }
}
