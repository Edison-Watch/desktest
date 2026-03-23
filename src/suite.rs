use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::config::Config;
use crate::error::AppError;
use crate::results::{self, TestResult, TestStatus};
use crate::task::TaskDefinition;

/// Schema version for suite results.
const SUITE_RESULTS_SCHEMA_VERSION: &str = "1.0";

/// Summary counts for a suite run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
}

/// Structured suite result written as suite-results.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteResult {
    pub schema_version: String,
    pub summary: SuiteSummary,
    pub total_duration_ms: u64,
    pub results: Vec<TestResult>,
}

/// A single test entry discovered for suite execution.
#[derive(Debug, Clone)]
pub struct SuiteTestEntry {
    pub path: PathBuf,
    pub task_def: TaskDefinition,
}

/// Discover all *.json task files in a directory.
pub fn discover_tasks(dir: &Path, filter: Option<&str>) -> Result<Vec<SuiteTestEntry>, AppError> {
    if !dir.is_dir() {
        return Err(AppError::Config(format!(
            "Suite directory '{}' is not a directory or does not exist.",
            dir.display()
        )));
    }

    let mut entries: Vec<SuiteTestEntry> = Vec::new();

    let read_dir = std::fs::read_dir(dir).map_err(|e| {
        AppError::Config(format!(
            "Cannot read suite directory '{}': {e}",
            dir.display()
        ))
    })?;

    for entry in read_dir {
        let entry =
            entry.map_err(|e| AppError::Infra(format!("Error reading directory entry: {e}")))?;
        let path = entry.path();

        // Only process *.json files
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        // Try loading as a task definition
        match TaskDefinition::load(&path) {
            Ok(task_def) => {
                // Apply name filter if provided
                if let Some(pattern) = filter {
                    if !task_def.id.contains(pattern) {
                        info!(
                            "Skipping '{}' (doesn't match filter '{pattern}')",
                            task_def.id
                        );
                        continue;
                    }
                }
                entries.push(SuiteTestEntry { path, task_def });
            }
            Err(e) => {
                // Skip files that aren't valid task definitions (e.g. config files)
                info!("Skipping '{}': {e}", path.display());
            }
        }
    }

    // Sort by file name for deterministic order
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    if entries.is_empty() {
        return Err(AppError::Config(format!(
            "No valid task files found in '{}'.",
            dir.display()
        )));
    }

    Ok(entries)
}

/// Build a SuiteResult from individual test results.
pub fn build_suite_result(results: Vec<TestResult>, total_duration_ms: u64) -> SuiteResult {
    let total = results.len();
    let passed = results
        .iter()
        .filter(|r| r.status == TestStatus::Pass)
        .count();
    let errors = results
        .iter()
        .filter(|r| r.status == TestStatus::Error)
        .count();
    let failed = total - passed - errors;

    SuiteResult {
        schema_version: SUITE_RESULTS_SCHEMA_VERSION.to_string(),
        summary: SuiteSummary {
            total,
            passed,
            failed,
            errors,
        },
        total_duration_ms,
        results,
    }
}

/// Write suite-results.json to the output directory.
pub fn write_suite_results(result: &SuiteResult, output_dir: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(output_dir).map_err(|e| {
        AppError::Infra(format!(
            "Cannot create output directory '{}': {e}",
            output_dir.display()
        ))
    })?;

    let path = output_dir.join("suite-results.json");
    let json = serde_json::to_string_pretty(result)
        .map_err(|e| AppError::Infra(format!("Failed to serialize suite results: {e}")))?;

    std::fs::write(&path, &json).map_err(|e| {
        AppError::Infra(format!(
            "Failed to write suite results to '{}': {e}",
            path.display()
        ))
    })?;

    info!("Suite results written to {}", path.display());
    Ok(())
}

/// Print a summary table of suite results to stdout.
pub fn print_summary_table(suite_result: &SuiteResult) {
    println!("\n{}", "=".repeat(60));
    println!("  Test Suite Results");
    println!("{}", "=".repeat(60));

    // Header
    println!(
        "  {:<30} {:>8} {:>10} {}",
        "Test", "Status", "Duration", "Reason"
    );
    println!("  {}", "-".repeat(56));

    for result in &suite_result.results {
        let status = match result.status {
            TestStatus::Pass => "PASS",
            TestStatus::Fail => "FAIL",
            TestStatus::Error => "ERROR",
        };

        let duration = format_duration_ms(result.duration_ms);

        let reason = match result.status {
            TestStatus::Pass => String::new(),
            _ => result
                .error_detail
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>(),
        };

        println!(
            "  {:<30} {:>8} {:>10} {}",
            truncate_str(&result.test_id, 30),
            status,
            duration,
            reason
        );
    }

    println!("  {}", "-".repeat(56));

    // Summary line
    let s = &suite_result.summary;
    println!(
        "  Total: {} | Passed: {} | Failed: {} | Errors: {} | Duration: {}",
        s.total,
        s.passed,
        s.failed,
        s.errors,
        format_duration_ms(suite_result.total_duration_ms),
    );
    println!("{}\n", "=".repeat(60));
}

/// Compute the suite exit code based on results.
///
/// Returns: 0 if all pass, 1 if any fail, 3 if any infra error (masks test results per AC).
pub fn suite_exit_code(suite_result: &SuiteResult) -> i32 {
    if suite_result.summary.errors > 0 {
        3
    } else if suite_result.summary.failed > 0 {
        1
    } else {
        0
    }
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        let secs = ms as f64 / 1000.0;
        format!("{secs:.1}s")
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Run a suite of tests sequentially, each in a fresh container.
///
/// This is the main orchestration function. It calls `run_single_test` for each
/// discovered task file, collects results, and produces the suite output.
pub async fn run_suite(
    dir: &Path,
    config: Option<&Path>,
    filter: Option<&str>,
    output_dir: &Path,
    debug: bool,
    verbose: bool,
    bash_enabled: bool,
    no_recording: bool,
    resolution: Option<&str>,
    monitor: Option<crate::monitor::MonitorHandle>,
    qa: bool,
) -> Result<SuiteResult, AppError> {
    let entries = discover_tasks(dir, filter)?;

    info!(
        "Discovered {} task(s) in '{}'",
        entries.len(),
        dir.display()
    );
    println!("Running {} test(s)...\n", entries.len());

    let mut run_config = if let Some(config_path) = config {
        Config::load_and_validate(config_path)?
    } else {
        Config::from_task_defaults()
    };

    if let Some(res) = resolution {
        match crate::parse_resolution(res) {
            Ok((w, h)) => {
                run_config.display_width = w;
                run_config.display_height = h;
            }
            Err(e) => {
                eprintln!("Resolution error: {e}");
                std::process::exit(2);
            }
        }
    }

    let suite_start = Instant::now();
    let mut test_results: Vec<TestResult> = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        println!(
            "[{}/{}] Running: {} ({})",
            i + 1,
            entries.len(),
            entry.task_def.id,
            entry.path.display()
        );

        if let Some(ref m) = monitor {
            m.send(crate::monitor::MonitorEvent::SuiteProgress {
                completed: i,
                total: entries.len(),
                current_test_id: entry.task_def.id.clone(),
            });
        }

        let test_output_dir = output_dir.join(&entry.task_def.id);
        let test_start = Instant::now();

        // Run the single test using the existing run_task flow
        let result = crate::run_task(
            entry.task_def.clone(),
            run_config.clone(),
            debug,
            verbose,
            bash_enabled,
            no_recording,
            test_output_dir.clone(),
            monitor.clone(),
            qa,
        )
        .await;

        let duration_ms = test_start.elapsed().as_millis() as u64;

        let test_result = match result {
            Ok(outcome) => {
                let eval_result = None; // Results already written by run_task
                let status = if outcome.passed { "PASS" } else { "FAIL" };
                println!("  Result: {status} ({:.1}s)\n", duration_ms as f64 / 1000.0);
                results::from_outcome(&entry.task_def.id, &outcome, eval_result, duration_ms, qa)
            }
            Err(ref e) => {
                println!(
                    "  Result: ERROR - {e} ({:.1}s)\n",
                    duration_ms as f64 / 1000.0
                );
                results::from_error(&entry.task_def.id, e, duration_ms)
            }
        };

        test_results.push(test_result);
    }

    // Emit final progress event so the dashboard reaches 100%
    if let Some(ref m) = monitor {
        m.send(crate::monitor::MonitorEvent::SuiteProgress {
            completed: entries.len(),
            total: entries.len(),
            current_test_id: String::new(),
        });
    }

    let total_duration_ms = suite_start.elapsed().as_millis() as u64;
    let suite_result = build_suite_result(test_results, total_duration_ms);

    // Write suite-results.json
    write_suite_results(&suite_result, output_dir)?;

    // Print summary table
    print_summary_table(&suite_result);

    Ok(suite_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_test_result(id: &str, status: TestStatus, duration_ms: u64) -> TestResult {
        let error_category = match &status {
            TestStatus::Pass => None,
            TestStatus::Fail => Some("test_failure".into()),
            TestStatus::Error => Some("infra_error".into()),
        };
        let error_detail = match &status {
            TestStatus::Pass => None,
            TestStatus::Fail => Some("Test failed".into()),
            TestStatus::Error => Some("Container crashed".into()),
        };
        TestResult {
            schema_version: "1.0".into(),
            test_id: id.into(),
            status,
            duration_ms,
            metric_results: vec![],
            agent_verdict: None,
            error_category,
            error_detail,
            bugs_found: None,
        }
    }

    // --- discover_tasks tests ---

    #[test]
    fn test_discover_tasks_nonexistent_dir() {
        let err = discover_tasks(Path::new("/nonexistent/dir"), None).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(err.to_string().contains("not a directory"));
    }

    #[test]
    fn test_discover_tasks_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let err = discover_tasks(tmp.path(), None).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(err.to_string().contains("No valid task files"));
    }

    #[test]
    fn test_discover_tasks_finds_valid_tasks() {
        let tmp = tempfile::tempdir().unwrap();

        // Write a valid task file
        let task_json = r#"{
            "schema_version": "1.0",
            "id": "test-001",
            "instruction": "Do something",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;
        std::fs::write(tmp.path().join("task1.json"), task_json).unwrap();

        let entries = discover_tasks(tmp.path(), None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].task_def.id, "test-001");
    }

    #[test]
    fn test_discover_tasks_skips_non_json_files() {
        let tmp = tempfile::tempdir().unwrap();

        let task_json = r#"{
            "schema_version": "1.0",
            "id": "test-001",
            "instruction": "Do something",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;
        std::fs::write(tmp.path().join("task1.json"), task_json).unwrap();
        std::fs::write(tmp.path().join("README.md"), "# readme").unwrap();
        std::fs::write(tmp.path().join("config.txt"), "config").unwrap();

        let entries = discover_tasks(tmp.path(), None).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_discover_tasks_skips_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();

        let valid = r#"{
            "schema_version": "1.0",
            "id": "valid-test",
            "instruction": "Do something",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;
        std::fs::write(tmp.path().join("valid.json"), valid).unwrap();
        std::fs::write(tmp.path().join("invalid.json"), "not a task").unwrap();

        let entries = discover_tasks(tmp.path(), None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].task_def.id, "valid-test");
    }

    #[test]
    fn test_discover_tasks_sorted_by_path() {
        let tmp = tempfile::tempdir().unwrap();

        for name in ["c_task.json", "a_task.json", "b_task.json"] {
            let id = name.trim_end_matches(".json");
            let json = format!(
                r#"{{"schema_version":"1.0","id":"{id}","instruction":"test","app":{{"type":"appimage","path":"/apps/t.AppImage"}}}}"#
            );
            std::fs::write(tmp.path().join(name), json).unwrap();
        }

        let entries = discover_tasks(tmp.path(), None).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].task_def.id, "a_task");
        assert_eq!(entries[1].task_def.id, "b_task");
        assert_eq!(entries[2].task_def.id, "c_task");
    }

    #[test]
    fn test_discover_tasks_with_filter() {
        let tmp = tempfile::tempdir().unwrap();

        for (name, id) in [("gedit.json", "gedit-save"), ("calc.json", "calc-add")] {
            let json = format!(
                r#"{{"schema_version":"1.0","id":"{id}","instruction":"test","app":{{"type":"appimage","path":"/apps/t.AppImage"}}}}"#
            );
            std::fs::write(tmp.path().join(name), json).unwrap();
        }

        let entries = discover_tasks(tmp.path(), Some("gedit")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].task_def.id, "gedit-save");
    }

    #[test]
    fn test_discover_tasks_filter_no_match() {
        let tmp = tempfile::tempdir().unwrap();

        let json = r#"{"schema_version":"1.0","id":"test-001","instruction":"test","app":{"type":"appimage","path":"/apps/t.AppImage"}}"#;
        std::fs::write(tmp.path().join("task.json"), json).unwrap();

        let err = discover_tasks(tmp.path(), Some("nonexistent")).unwrap_err();
        assert!(err.to_string().contains("No valid task files"));
    }

    // --- build_suite_result tests ---

    #[test]
    fn test_build_suite_result_all_pass() {
        let results = vec![
            make_test_result("t1", TestStatus::Pass, 1000),
            make_test_result("t2", TestStatus::Pass, 2000),
        ];
        let suite = build_suite_result(results, 3000);

        assert_eq!(suite.schema_version, "1.0");
        assert_eq!(suite.summary.total, 2);
        assert_eq!(suite.summary.passed, 2);
        assert_eq!(suite.summary.failed, 0);
        assert_eq!(suite.summary.errors, 0);
        assert_eq!(suite.total_duration_ms, 3000);
        assert_eq!(suite.results.len(), 2);
    }

    #[test]
    fn test_build_suite_result_mixed() {
        let results = vec![
            make_test_result("t1", TestStatus::Pass, 1000),
            make_test_result("t2", TestStatus::Fail, 2000),
            make_test_result("t3", TestStatus::Error, 500),
        ];
        let suite = build_suite_result(results, 3500);

        assert_eq!(suite.summary.total, 3);
        assert_eq!(suite.summary.passed, 1);
        assert_eq!(suite.summary.failed, 1);
        assert_eq!(suite.summary.errors, 1);
    }

    #[test]
    fn test_build_suite_result_empty() {
        let suite = build_suite_result(vec![], 0);

        assert_eq!(suite.summary.total, 0);
        assert_eq!(suite.summary.passed, 0);
        assert_eq!(suite.summary.failed, 0);
        assert_eq!(suite.summary.errors, 0);
    }

    // --- suite_exit_code tests ---

    #[test]
    fn test_exit_code_all_pass() {
        let results = vec![make_test_result("t1", TestStatus::Pass, 1000)];
        let suite = build_suite_result(results, 1000);
        assert_eq!(suite_exit_code(&suite), 0);
    }

    #[test]
    fn test_exit_code_with_failure() {
        let results = vec![
            make_test_result("t1", TestStatus::Pass, 1000),
            make_test_result("t2", TestStatus::Fail, 2000),
        ];
        let suite = build_suite_result(results, 3000);
        assert_eq!(suite_exit_code(&suite), 1);
    }

    #[test]
    fn test_exit_code_with_error_masks_failures() {
        let results = vec![
            make_test_result("t1", TestStatus::Fail, 1000),
            make_test_result("t2", TestStatus::Error, 500),
        ];
        let suite = build_suite_result(results, 1500);
        // Error (3) takes priority over failure (1)
        assert_eq!(suite_exit_code(&suite), 3);
    }

    // --- write_suite_results tests ---

    #[test]
    fn test_write_suite_results_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let results = vec![make_test_result("t1", TestStatus::Pass, 1000)];
        let suite = build_suite_result(results, 1000);

        write_suite_results(&suite, tmp.path()).unwrap();

        let path = tmp.path().join("suite-results.json");
        assert!(path.exists());

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: SuiteResult = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed.summary.total, 1);
        assert_eq!(parsed.summary.passed, 1);
    }

    #[test]
    fn test_write_suite_results_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("nested").join("dir");
        let suite = build_suite_result(vec![], 0);

        write_suite_results(&suite, &nested).unwrap();

        let path = nested.join("suite-results.json");
        assert!(path.exists());
    }

    // --- format helpers tests ---

    #[test]
    fn test_format_duration_ms() {
        assert_eq!(format_duration_ms(500), "500ms");
        assert_eq!(format_duration_ms(999), "999ms");
        assert_eq!(format_duration_ms(1000), "1.0s");
        assert_eq!(format_duration_ms(1500), "1.5s");
        assert_eq!(format_duration_ms(60000), "60.0s");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello", 5), "hello");
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    // --- print_summary_table does not panic ---

    #[test]
    fn test_print_summary_table_no_panic() {
        let results = vec![
            make_test_result("test-pass", TestStatus::Pass, 1234),
            make_test_result("test-fail", TestStatus::Fail, 5678),
            make_test_result("test-error", TestStatus::Error, 100),
        ];
        let suite = build_suite_result(results, 7012);
        // Should not panic
        print_summary_table(&suite);
    }

    #[test]
    fn test_print_summary_table_empty() {
        let suite = build_suite_result(vec![], 0);
        print_summary_table(&suite);
    }

    // --- SuiteResult serialization ---

    #[test]
    fn test_suite_result_roundtrips() {
        let results = vec![
            make_test_result("t1", TestStatus::Pass, 1000),
            make_test_result("t2", TestStatus::Fail, 2000),
        ];
        let suite = build_suite_result(results, 3000);

        let json = serde_json::to_string(&suite).unwrap();
        let parsed: SuiteResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, "1.0");
        assert_eq!(parsed.summary.total, 2);
        assert_eq!(parsed.summary.passed, 1);
        assert_eq!(parsed.summary.failed, 1);
        assert_eq!(parsed.results.len(), 2);
    }
}
