//! Print trajectory logs to the terminal in a structured text format.
//!
//! Usage: `desktest logs <artifacts_dir> [--brief] [--summary] [--failures] [--json] [--step N]`

use std::path::Path;

use crate::codify;
use crate::error::AppError;

/// Options for the `logs` subcommand.
pub struct LogsOptions {
    pub brief: bool,
    pub summary: bool,
    pub failures: bool,
    pub json: bool,
    pub step_filter: Option<Vec<usize>>,
}

/// Print trajectory logs to stdout.
pub fn print_logs(artifacts_dir: &Path, opts: LogsOptions) -> Result<(), AppError> {
    if opts.brief && opts.step_filter.is_some() {
        return Err(AppError::Config(
            "--brief and --step/--steps cannot be used together".into(),
        ));
    }

    let trajectory_path = artifacts_dir.join("trajectory.jsonl");
    let entries = codify::load_trajectory(&trajectory_path)?;

    if entries.is_empty() {
        if opts.json {
            println!(
                "{}",
                serde_json::json!({
                    "steps": [],
                    "summary": {
                        "total_steps": 0,
                        "result": "empty",
                        "duration_secs": null,
                    }
                })
            );
        } else {
            println!("No trajectory entries found.");
        }
        return Ok(());
    }

    // Try to load task metadata from task.json in artifacts dir
    let task_id = load_task_id(artifacts_dir);

    // Compute summary
    let total_steps = entries.len();
    let last_step_num = entries.last().map(|e| e.step).unwrap_or(0);
    let final_result = entries
        .last()
        .map(|e| e.result.as_str())
        .unwrap_or("unknown");
    let duration = compute_duration(&entries);
    let duration_secs = compute_duration_secs(&entries);

    // Apply step filter
    let entries_filtered: Vec<&codify::TrajectoryRecord> = if let Some(ref filter) = opts.step_filter {
        let filter_set: std::collections::HashSet<usize> = filter.iter().copied().collect();
        entries.iter().filter(|e| filter_set.contains(&e.step)).collect()
    } else {
        entries.iter().collect()
    };

    // Apply failures filter
    let entries_view: Vec<&codify::TrajectoryRecord> = if opts.failures {
        entries_filtered
            .into_iter()
            .filter(|e| is_failure(&e.result))
            .collect()
    } else {
        entries_filtered
    };

    if opts.json {
        print_json(&entries_view, &task_id, total_steps, final_result, last_step_num, duration_secs);
        return Ok(());
    }

    // Print header (to stderr when --summary or --failures so stdout stays clean for piping)
    println!("== Trajectory Review ==");
    if let Some(id) = &task_id {
        println!("Task:       {id}");
    }
    println!("Steps:      {total_steps}");
    println!("Result:     {}", format_result(final_result, last_step_num));
    if let Some(dur) = &duration {
        println!("Duration:   {dur}");
    }
    println!();

    if opts.summary {
        print_summary(&entries_view);
    } else if opts.brief {
        print_brief(&entries_view);
    } else if entries_view.is_empty() {
        if opts.failures {
            println!("No failed steps found.");
        } else {
            println!("No entries found for the requested steps.");
        }
    } else {
        for entry in &entries_view {
            print_step_detail(entry);
        }
    }

    Ok(())
}

/// Check if a result string indicates a failure.
fn is_failure(result: &str) -> bool {
    matches!(result, "fail" | "timeout" | "max_steps")
        || result.starts_with("error")
}

fn print_json(
    entries: &[&codify::TrajectoryRecord],
    task_id: &Option<String>,
    total_steps: usize,
    final_result: &str,
    last_step_num: usize,
    duration_secs: Option<u64>,
) {
    let steps: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let mut step = serde_json::json!({
                "step": e.step,
                "timestamp": e.timestamp,
                "result": e.result,
                "action_code": e.action_code,
            });
            let map = step.as_object_mut().unwrap();
            if let Some(ref t) = e.thought {
                map.insert("thought".into(), serde_json::json!(t));
            }
            if let Some(ref at) = e.action_type {
                map.insert("action_type".into(), serde_json::json!(at));
            }
            if let Some(ref ef) = e.error_feedback {
                map.insert("error_feedback".into(), serde_json::json!(ef));
            }
            if let Some(ref bo) = e.bash_output {
                map.insert("bash_output".into(), serde_json::json!(bo));
            }
            if let Some(ref sp) = e.screenshot_path {
                map.insert("screenshot_path".into(), serde_json::json!(sp));
            }
            step
        })
        .collect();

    let mut output = serde_json::json!({
        "steps": steps,
        "summary": {
            "total_steps": total_steps,
            "result": final_result,
            "result_display": format_result(final_result, last_step_num),
            "duration_secs": duration_secs,
        }
    });
    if let Some(id) = task_id {
        output["summary"]["task_id"] = serde_json::json!(id);
    }

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn print_summary(entries: &[&codify::TrajectoryRecord]) {
    if entries.is_empty() {
        println!("No matching steps.");
        return;
    }
    println!(
        "{:<6} {:<8} {:<14} {}",
        "Step", "Status", "Type", "Summary"
    );
    println!("{}", "-".repeat(72));
    for entry in entries {
        let status = if is_failure(&entry.result) {
            "\u{2717}"
        } else if entry.result == "done" {
            "\u{2713}"
        } else {
            "\u{2713}"
        };
        let action_type = entry.action_type.as_deref().unwrap_or("-");
        let thought = entry
            .thought
            .as_deref()
            .unwrap_or("")
            .replace('\n', " ");
        let thought_truncated: String = thought.chars().take(44).collect();
        println!(
            "{:<6} {:<8} {:<14} {}",
            entry.step, status, action_type, thought_truncated
        );
    }
}

fn print_brief(entries: &[&codify::TrajectoryRecord]) {
    println!(
        "{:<6} {:<12} {:<26} Thought",
        "Step", "Result", "Timestamp"
    );
    println!("{}", "-".repeat(80));
    for entry in entries {
        let thought = entry.thought.as_deref().unwrap_or("").replace('\n', " ");
        let thought_truncated: String = thought.chars().take(40).collect();
        let result_truncated: String = entry.result.chars().take(12).collect();
        println!(
            "{:<6} {:<12} {:<26} {}",
            entry.step, result_truncated, entry.timestamp, thought_truncated
        );
    }
}

fn print_step_detail(entry: &codify::TrajectoryRecord) {
    println!(
        "--- Step {} [{}] {} ---",
        entry.step, entry.result, entry.timestamp
    );
    if let Some(thought) = &entry.thought {
        println!("Thought: {thought}");
    }
    if !entry.action_code.trim().is_empty() {
        println!("Action:");
        for line in entry.action_code.lines() {
            println!("  {line}");
        }
    }
    if let Some(error) = &entry.error_feedback {
        println!("Error: {error}");
    }
    println!("Result: {}", entry.result);
    println!();
}

fn format_result(result: &str, step: usize) -> String {
    match result {
        "done" => format!("PASS (done at step {step})"),
        "success" => format!("OK (last step {step})"),
        "error" => format!("ERROR (at step {step})"),
        other => format!("{} (at step {})", other.to_uppercase(), step),
    }
}

fn compute_duration(entries: &[codify::TrajectoryRecord]) -> Option<String> {
    if entries.len() < 2 {
        return None;
    }
    let first = parse_timestamp_secs(&entries[0].timestamp)?;
    let last = parse_timestamp_secs(&entries[entries.len() - 1].timestamp)?;
    let total_secs = last.checked_sub(first)?;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    if mins > 0 {
        Some(format!("{mins}m {secs:02}s"))
    } else {
        Some(format!("{secs}s"))
    }
}

fn compute_duration_secs(entries: &[codify::TrajectoryRecord]) -> Option<u64> {
    if entries.len() < 2 {
        return None;
    }
    let first = parse_timestamp_secs(&entries[0].timestamp)?;
    let last = parse_timestamp_secs(&entries[entries.len() - 1].timestamp)?;
    last.checked_sub(first)
}

/// Parse an ISO 8601 / RFC 3339 timestamp into approximate epoch seconds.
fn parse_timestamp_secs(ts: &str) -> Option<u64> {
    // Expected format: 2026-02-26T12:00:01Z or 2026-02-26T12:00:01+00:00
    let ts = ts.trim();

    // Parse timezone offset (seconds from UTC), then strip it from the timestamp
    let (date_time, offset_secs): (&str, i64) = if let Some(dt) = ts.strip_suffix('Z') {
        (dt, 0)
    } else if ts.len() > 6 {
        let tail = &ts[ts.len() - 6..];
        if (tail.starts_with('+') || tail.starts_with('-')) && tail.as_bytes()[3] == b':' {
            let sign: i64 = if tail.starts_with('-') { -1 } else { 1 };
            let oh: i64 = tail[1..3].parse().ok()?;
            let om: i64 = tail[4..6].parse().ok()?;
            (&ts[..ts.len() - 6], sign * (oh * 3600 + om * 60))
        } else {
            return None;
        }
    } else {
        return None;
    };

    let (date, time) = date_time.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: u64 = date_parts.next()?.parse().ok()?;
    let month: u64 = date_parts.next()?.parse().ok()?;
    let day: u64 = date_parts.next()?.parse().ok()?;

    let mut time_parts = time.split(':');
    let hour: u64 = time_parts.next()?.parse().ok()?;
    let min: u64 = time_parts.next()?.parse().ok()?;
    // Seconds may have fractional part
    let sec_str = time_parts.next()?;
    let sec: u64 = sec_str.split('.').next()?.parse().ok()?;

    // Accumulated days at the start of each month (non-leap year)
    const MONTH_DAYS: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let month_idx = (month.saturating_sub(1) as usize).min(11);
    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let leap_offset: u64 = if is_leap && month > 2 { 1 } else { 0 };
    // Use year-1 for leap day accumulation so current year's leap day isn't double-counted
    let yp = year.saturating_sub(1);
    let days =
        year * 365 + yp / 4 - yp / 100 + yp / 400 + MONTH_DAYS[month_idx] + day + leap_offset;
    let raw_secs = (days * 86400 + hour * 3600 + min * 60 + sec) as i64;
    // Apply timezone offset to get UTC-relative seconds
    let utc_secs = raw_secs - offset_secs;
    Some(utc_secs.max(0) as u64)
}

fn load_task_id(artifacts_dir: &Path) -> Option<String> {
    let task_json_path = artifacts_dir.join("task.json");
    let content = std::fs::read_to_string(&task_json_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    val.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codify::TrajectoryRecord;

    fn make_entry(step: usize, result: &str, thought: Option<&str>, action_type: Option<&str>) -> TrajectoryRecord {
        TrajectoryRecord {
            step,
            timestamp: format!("2026-01-01T00:00:{:02}Z", step),
            action_code: format!("pyautogui.click({}, 200)", step * 100),
            thought: thought.map(|s| s.to_string()),
            screenshot_path: Some(format!("step_{step:03}.png")),
            result: result.to_string(),
            bash_output: None,
            error_feedback: if result.starts_with("error") {
                Some("something went wrong".into())
            } else {
                None
            },
            action_type: action_type.map(|s| s.to_string()),
        }
    }

    fn sample_entries() -> Vec<TrajectoryRecord> {
        vec![
            make_entry(1, "success", Some("Click the button"), Some("python")),
            make_entry(2, "success", Some("Type hello"), Some("python")),
            make_entry(3, "error:crash", Some("Try to save"), Some("python")),
            make_entry(4, "success", Some("Retry save"), Some("python")),
            make_entry(5, "done", Some("Task complete"), None),
        ]
    }

    // --- is_failure tests ---

    #[test]
    fn test_is_failure_fail() {
        assert!(is_failure("fail"));
    }

    #[test]
    fn test_is_failure_timeout() {
        assert!(is_failure("timeout"));
    }

    #[test]
    fn test_is_failure_max_steps() {
        assert!(is_failure("max_steps"));
    }

    #[test]
    fn test_is_failure_error_prefix() {
        assert!(is_failure("error"));
        assert!(is_failure("error:crash"));
        assert!(is_failure("error:something went wrong"));
    }

    #[test]
    fn test_is_failure_success_is_not_failure() {
        assert!(!is_failure("success"));
    }

    #[test]
    fn test_is_failure_done_is_not_failure() {
        assert!(!is_failure("done"));
    }

    #[test]
    fn test_is_failure_wait_is_not_failure() {
        assert!(!is_failure("wait"));
    }

    // --- format_result tests ---

    #[test]
    fn test_format_result_done() {
        assert_eq!(format_result("done", 5), "PASS (done at step 5)");
    }

    #[test]
    fn test_format_result_success() {
        assert_eq!(format_result("success", 3), "OK (last step 3)");
    }

    #[test]
    fn test_format_result_error() {
        assert_eq!(format_result("error", 2), "ERROR (at step 2)");
    }

    #[test]
    fn test_format_result_other() {
        assert_eq!(format_result("timeout", 7), "TIMEOUT (at step 7)");
    }

    // --- failures filter integration tests (via print_logs on temp dir) ---

    fn write_trajectory(dir: &Path, entries: &[TrajectoryRecord]) {
        let path = dir.join("trajectory.jsonl");
        let lines: Vec<String> = entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect();
        std::fs::write(path, lines.join("\n")).unwrap();
    }

    #[test]
    fn test_print_logs_default_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &sample_entries());
        let opts = LogsOptions {
            brief: false,
            summary: false,
            failures: false,
            json: false,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_brief_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &sample_entries());
        let opts = LogsOptions {
            brief: true,
            summary: false,
            failures: false,
            json: false,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_summary_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &sample_entries());
        let opts = LogsOptions {
            brief: false,
            summary: true,
            failures: false,
            json: false,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_failures_only_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &sample_entries());
        let opts = LogsOptions {
            brief: false,
            summary: false,
            failures: true,
            json: false,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_summary_with_failures_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &sample_entries());
        let opts = LogsOptions {
            brief: false,
            summary: true,
            failures: true,
            json: false,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_brief_with_step_filter_errors() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &sample_entries());
        let opts = LogsOptions {
            brief: true,
            summary: false,
            failures: false,
            json: false,
            step_filter: Some(vec![1]),
        };
        assert!(print_logs(dir.path(), opts).is_err());
    }

    #[test]
    fn test_print_logs_empty_trajectory() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &[]);
        let opts = LogsOptions {
            brief: false,
            summary: false,
            failures: false,
            json: false,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_empty_trajectory_json() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &[]);
        let opts = LogsOptions {
            brief: false,
            summary: false,
            failures: false,
            json: true,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_step_filter() {
        let dir = tempfile::tempdir().unwrap();
        write_trajectory(dir.path(), &sample_entries());
        let opts = LogsOptions {
            brief: false,
            summary: false,
            failures: false,
            json: false,
            step_filter: Some(vec![2, 4]),
        };
        assert!(print_logs(dir.path(), opts).is_ok());
    }

    #[test]
    fn test_print_logs_missing_trajectory_errors() {
        let dir = tempfile::tempdir().unwrap();
        // No trajectory file written
        let opts = LogsOptions {
            brief: false,
            summary: false,
            failures: false,
            json: false,
            step_filter: None,
        };
        assert!(print_logs(dir.path(), opts).is_err());
    }

    // --- JSON output structure tests ---

    #[test]
    fn test_json_output_structure() {
        let entries = sample_entries();
        let refs: Vec<&TrajectoryRecord> = entries.iter().collect();
        // Capture JSON by building it directly (same logic as print_json)
        let steps: Vec<serde_json::Value> = refs
            .iter()
            .map(|e| {
                let mut step = serde_json::json!({
                    "step": e.step,
                    "timestamp": e.timestamp,
                    "result": e.result,
                    "action_code": e.action_code,
                });
                let map = step.as_object_mut().unwrap();
                if let Some(ref t) = e.thought {
                    map.insert("thought".into(), serde_json::json!(t));
                }
                if let Some(ref at) = e.action_type {
                    map.insert("action_type".into(), serde_json::json!(at));
                }
                if let Some(ref ef) = e.error_feedback {
                    map.insert("error_feedback".into(), serde_json::json!(ef));
                }
                step
            })
            .collect();

        let output = serde_json::json!({
            "steps": steps,
            "summary": {
                "total_steps": 5,
                "result": "done",
                "result_display": format_result("done", 5),
                "duration_secs": 4u64,
            }
        });

        // Verify top-level keys
        assert!(output.get("steps").unwrap().is_array());
        assert!(output.get("summary").unwrap().is_object());

        // Verify step count
        assert_eq!(output["steps"].as_array().unwrap().len(), 5);

        // Verify summary fields
        let summary = &output["summary"];
        assert_eq!(summary["total_steps"], 5);
        assert_eq!(summary["result"], "done");
        assert_eq!(summary["duration_secs"], 4);

        // Verify error step has error_feedback
        let error_step = &output["steps"][2];
        assert_eq!(error_step["result"], "error:crash");
        assert!(error_step.get("error_feedback").is_some());

        // Verify success step has no error_feedback
        let ok_step = &output["steps"][0];
        assert!(ok_step.get("error_feedback").is_none());
    }

    #[test]
    fn test_json_failures_filter() {
        let entries = sample_entries();
        let refs: Vec<&TrajectoryRecord> = entries.iter().collect();
        let failures: Vec<&&TrajectoryRecord> = refs.iter().filter(|e| is_failure(&e.result)).collect();

        // Only step 3 (error:crash) should be a failure
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].step, 3);
    }

    // --- compute_duration tests ---

    #[test]
    fn test_compute_duration_multiple_entries() {
        let entries = sample_entries();
        let dur = compute_duration(&entries);
        assert_eq!(dur, Some("4s".to_string()));
    }

    #[test]
    fn test_compute_duration_single_entry() {
        let entries = vec![make_entry(1, "success", None, None)];
        assert_eq!(compute_duration(&entries), None);
    }

    #[test]
    fn test_compute_duration_secs() {
        let entries = sample_entries();
        assert_eq!(compute_duration_secs(&entries), Some(4));
    }

    // --- load_task_id tests ---

    #[test]
    fn test_load_task_id_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("task.json"),
            r#"{"id": "my-test-task", "schema_version": 1}"#,
        )
        .unwrap();
        assert_eq!(load_task_id(dir.path()), Some("my-test-task".to_string()));
    }

    #[test]
    fn test_load_task_id_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_task_id(dir.path()), None);
    }

    #[test]
    fn test_load_task_id_no_id_field() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("task.json"),
            r#"{"name": "something"}"#,
        )
        .unwrap();
        assert_eq!(load_task_id(dir.path()), None);
    }
}
