//! Print trajectory logs to the terminal in a structured text format.
//!
//! Usage: `desktest logs <artifacts_dir> [--brief] [--step N]`

use std::path::Path;

use crate::codify;
use crate::error::AppError;

/// Print trajectory logs to stdout.
pub fn print_logs(artifacts_dir: &Path, brief: bool, step: Option<usize>) -> Result<(), AppError> {
    if brief && step.is_some() {
        return Err(AppError::Config("--brief and --step cannot be used together".into()));
    }

    let trajectory_path = artifacts_dir.join("trajectory.jsonl");
    let entries = codify::load_trajectory(&trajectory_path)?;

    if entries.is_empty() {
        println!("No trajectory entries found.");
        return Ok(());
    }

    // Try to load task metadata from task.json in artifacts dir
    let task_id = load_task_id(artifacts_dir);

    // Compute summary
    let total_steps = entries.len();
    let last_step_num = entries.last().map(|e| e.step).unwrap_or(0);
    let final_result = entries.last().map(|e| e.result.as_str()).unwrap_or("unknown");
    let duration = compute_duration(&entries);

    // Print header
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

    if brief {
        print_brief(&entries);
    } else if let Some(n) = step {
        let matching: Vec<_> = entries.iter().filter(|e| e.step == n).collect();
        if matching.is_empty() {
            println!("No entry found for step {n}.");
        } else {
            for entry in matching {
                print_step_detail(entry);
            }
        }
    } else {
        for entry in &entries {
            print_step_detail(entry);
        }
    }

    Ok(())
}

fn print_brief(entries: &[codify::TrajectoryRecord]) {
    println!("{:<6} {:<10} {:<26} {}", "Step", "Result", "Timestamp", "Thought");
    println!("{}", "-".repeat(80));
    for entry in entries {
        let thought = entry
            .thought
            .as_deref()
            .unwrap_or("")
            .replace('\n', " ");
        let thought_truncated: String = thought.chars().take(40).collect();
        println!(
            "{:<6} {:<10} {:<26} {}",
            entry.step, entry.result, entry.timestamp, thought_truncated
        );
    }
}

fn print_step_detail(entry: &codify::TrajectoryRecord) {
    println!("--- Step {} [{}] {} ---", entry.step, entry.result, entry.timestamp);
    if let Some(thought) = &entry.thought {
        println!("Thought: {thought}");
    }
    if !entry.action_code.trim().is_empty() {
        println!("Action:");
        for line in entry.action_code.lines() {
            println!("  {line}");
        }
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
    let yp = year - 1;
    let days = year * 365 + yp / 4 - yp / 100 + yp / 400 + MONTH_DAYS[month_idx] + day + leap_offset;
    let raw_secs = (days * 86400 + hour * 3600 + min * 60 + sec) as i64;
    // Apply timezone offset to get UTC-relative seconds
    let utc_secs = raw_secs - offset_secs;
    Some(utc_secs.max(0) as u64)
}

fn load_task_id(artifacts_dir: &Path) -> Option<String> {
    let task_json_path = artifacts_dir.join("task.json");
    let content = std::fs::read_to_string(&task_json_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    val.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
}
