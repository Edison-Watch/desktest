//! Trajectory logging for agent test runs.
//!
//! Produces a `trajectory.jsonl` file with one JSON line per step,
//! flushed after each write for crash resilience. Optionally includes
//! the full raw LLM response when verbose mode is enabled.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tracing::warn;

/// A single entry in the trajectory log.
#[derive(Debug, Serialize)]
pub struct TrajectoryEntry {
    /// Step number (1-indexed).
    pub step: usize,
    /// ISO 8601 timestamp of when this step was recorded.
    pub timestamp: String,
    /// Extracted PyAutoGUI code blocks executed in this step (joined with newlines).
    pub action_code: String,
    /// Agent's reasoning/reflection text before the action (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<String>,
    /// Path to the screenshot file for this step (relative to artifacts dir).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_path: Option<String>,
    /// Path to the a11y tree file for this step (relative to artifacts dir).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a11y_tree_path: Option<String>,
    /// Result of this step: "success", "error:<message>", "done", "fail", "wait", "timeout", "max_steps".
    pub result: String,
    /// Full raw LLM response (only included with --verbose flag).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_raw_response: Option<String>,
}

/// Incremental trajectory logger that writes JSONL (one JSON object per line).
///
/// Uses `BufWriter` with explicit `flush()` after each entry for crash resilience.
pub struct TrajectoryLogger {
    writer: BufWriter<File>,
    artifacts_dir: PathBuf,
    verbose: bool,
}

impl TrajectoryLogger {
    /// Create a new trajectory logger writing to `trajectory.jsonl` in the given directory.
    ///
    /// The file is created (or truncated) on construction.
    pub fn new(artifacts_dir: &Path, verbose: bool) -> Result<Self, std::io::Error> {
        let path = artifacts_dir.join("trajectory.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)?;

        Ok(Self {
            writer: BufWriter::new(file),
            artifacts_dir: artifacts_dir.to_path_buf(),
            verbose,
        })
    }

    /// Log a trajectory entry, serializing to JSON and flushing immediately.
    pub fn log_entry(&mut self, entry: &TrajectoryEntry) {
        match serde_json::to_string(entry) {
            Ok(json) => {
                if let Err(e) = writeln!(self.writer, "{json}") {
                    warn!("Failed to write trajectory entry: {e}");
                    return;
                }
                if let Err(e) = self.writer.flush() {
                    warn!("Failed to flush trajectory: {e}");
                }
            }
            Err(e) => {
                warn!("Failed to serialize trajectory entry: {e}");
            }
        }
    }

    /// Build a trajectory entry from the step data.
    ///
    /// This is a convenience helper that extracts action code and thought
    /// from the LLM response text, and populates paths from observation data.
    pub fn build_entry(
        &self,
        step: usize,
        response_text: &str,
        code_blocks: &[String],
        screenshot_path: Option<&Path>,
        a11y_tree_text: Option<&str>,
        result: &str,
        raw_response: Option<&str>,
    ) -> TrajectoryEntry {
        let action_code = code_blocks.join("\n\n");
        let thought = extract_thought(response_text, code_blocks);

        // Convert absolute paths to relative (just the filename)
        let screenshot_rel = screenshot_path
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string());

        // Save a11y tree to file and get the relative path
        let a11y_tree_path = if let Some(text) = a11y_tree_text {
            let a11y_path = self.artifacts_dir.join(format!("step_{:03}_a11y.txt", step));
            match std::fs::write(&a11y_path, text) {
                Ok(()) => a11y_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string()),
                Err(e) => {
                    warn!("Failed to save a11y tree for step {step}: {e}");
                    None
                }
            }
        } else {
            None
        };

        let llm_raw_response = if self.verbose {
            raw_response.map(|s| s.to_string())
        } else {
            None
        };

        let now = chrono_iso8601_now();

        TrajectoryEntry {
            step,
            timestamp: now,
            action_code,
            thought,
            screenshot_path: screenshot_rel,
            a11y_tree_path,
            result: result.to_string(),
            llm_raw_response,
        }
    }
}

/// Extract the "thought" (reasoning) from an LLM response by removing code blocks
/// and special commands. Returns None if no meaningful text remains.
pub fn extract_thought(response_text: &str, code_blocks: &[String]) -> Option<String> {
    let mut text = response_text.to_string();

    // Remove code blocks from the text (python blocks)
    for block in code_blocks {
        // Skip blocks that were prefixed with "# [bash]\n" by all_blocks merging
        let raw_block = block.strip_prefix("# [bash]\n").unwrap_or(block);
        // Remove the fenced code block including markers
        let patterns = [
            format!("```python\n{}\n```", raw_block),
            format!("```py\n{}\n```", raw_block),
            format!("```python\n{raw_block}\n```"),
            format!("```py\n{raw_block}\n```"),
            format!("```bash\n{}\n```", raw_block),
            format!("```sh\n{}\n```", raw_block),
            format!("```bash\n{raw_block}\n```"),
            format!("```sh\n{raw_block}\n```"),
        ];
        for pattern in &patterns {
            text = text.replace(pattern, "");
        }
    }

    // Remove special command lines
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != "DONE" && trimmed != "FAIL" && trimmed != "WAIT"
        })
        .collect();

    let thought = lines.join("\n").trim().to_string();
    if thought.is_empty() {
        None
    } else {
        Some(thought)
    }
}

/// Get current UTC time as ISO 8601 string.
///
/// Uses a simple implementation without requiring the chrono crate.
pub(crate) fn chrono_iso8601_now() -> String {
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();

    let secs = now.as_secs();

    // Convert to date/time components
    // Days since epoch
    let days = secs / 86400;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year, month, day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Civil from days algorithm
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_thought_with_code_and_text() {
        let response = "I see the editor. Let me click.\n\n```python\npyautogui.click(100, 200)\n```\n\nNow I'll wait.";
        let code_blocks = vec!["pyautogui.click(100, 200)".to_string()];
        let thought = extract_thought(response, &code_blocks);
        assert!(thought.is_some());
        let t = thought.unwrap();
        assert!(t.contains("I see the editor"));
        assert!(!t.contains("pyautogui.click"));
    }

    #[test]
    fn test_extract_thought_code_only() {
        let response = "```python\npyautogui.click(100, 200)\n```";
        let code_blocks = vec!["pyautogui.click(100, 200)".to_string()];
        let thought = extract_thought(response, &code_blocks);
        assert!(thought.is_none());
    }

    #[test]
    fn test_extract_thought_removes_special_commands() {
        let response = "Task completed successfully.\n\nDONE";
        let thought = extract_thought(response, &[]);
        assert!(thought.is_some());
        assert!(!thought.unwrap().contains("DONE"));
    }

    #[test]
    fn test_extract_thought_empty() {
        let thought = extract_thought("", &[]);
        assert!(thought.is_none());
    }

    #[test]
    fn test_extract_thought_only_special_command() {
        let thought = extract_thought("DONE", &[]);
        assert!(thought.is_none());
    }

    #[test]
    fn test_extract_thought_strips_bash_blocks() {
        let response = "Let me check the process.\n\n```bash\nps aux | grep myapp\n```\n\nNow clicking.";
        // all_blocks has bash blocks prefixed with "# [bash]\n"
        let all_blocks = vec!["# [bash]\nps aux | grep myapp".to_string()];
        let thought = extract_thought(response, &all_blocks);
        assert!(thought.is_some());
        let t = thought.unwrap();
        assert!(t.contains("Let me check"));
        assert!(t.contains("Now clicking"));
        assert!(!t.contains("ps aux"));
    }

    #[test]
    fn test_extract_thought_strips_sh_blocks() {
        let response = "```sh\nls -la /tmp\n```";
        let all_blocks = vec!["# [bash]\nls -la /tmp".to_string()];
        let thought = extract_thought(response, &all_blocks);
        assert!(thought.is_none());
    }

    #[test]
    fn test_extract_thought_mixed_bash_and_python() {
        let response = "Checking state first.\n\n```bash\ncat /tmp/log\n```\n\nNow acting.\n\n```python\npyautogui.click(100, 200)\n```";
        let all_blocks = vec![
            "# [bash]\ncat /tmp/log".to_string(),
            "pyautogui.click(100, 200)".to_string(),
        ];
        let thought = extract_thought(response, &all_blocks);
        assert!(thought.is_some());
        let t = thought.unwrap();
        assert!(t.contains("Checking state"));
        assert!(t.contains("Now acting"));
        assert!(!t.contains("cat /tmp/log"));
        assert!(!t.contains("pyautogui.click"));
    }

    #[test]
    fn test_trajectory_entry_serialization() {
        let entry = TrajectoryEntry {
            step: 1,
            timestamp: "2026-02-26T12:00:00Z".to_string(),
            action_code: "pyautogui.click(100, 200)".to_string(),
            thought: Some("I see the button".to_string()),
            screenshot_path: Some("step_001.png".to_string()),
            a11y_tree_path: Some("step_001_a11y.txt".to_string()),
            result: "success".to_string(),
            llm_raw_response: None,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"step\":1"));
        assert!(json.contains("\"action_code\":\"pyautogui.click(100, 200)\""));
        assert!(json.contains("\"thought\":\"I see the button\""));
        assert!(json.contains("\"result\":\"success\""));
        // llm_raw_response should be omitted when None
        assert!(!json.contains("llm_raw_response"));
    }

    #[test]
    fn test_trajectory_entry_with_verbose() {
        let entry = TrajectoryEntry {
            step: 2,
            timestamp: "2026-02-26T12:00:01Z".to_string(),
            action_code: "pyautogui.press('enter')".to_string(),
            thought: None,
            screenshot_path: None,
            a11y_tree_path: None,
            result: "error:timeout".to_string(),
            llm_raw_response: Some("Full LLM response here".to_string()),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"llm_raw_response\":\"Full LLM response here\""));
        // thought and screenshot_path should be omitted when None
        assert!(!json.contains("\"thought\""));
        assert!(!json.contains("\"screenshot_path\""));
    }

    #[test]
    fn test_trajectory_entry_special_results() {
        for result_str in &["done", "fail", "wait", "timeout", "max_steps"] {
            let entry = TrajectoryEntry {
                step: 1,
                timestamp: "2026-02-26T12:00:00Z".to_string(),
                action_code: String::new(),
                thought: None,
                screenshot_path: None,
                a11y_tree_path: None,
                result: result_str.to_string(),
                llm_raw_response: None,
            };
            let json = serde_json::to_string(&entry).unwrap();
            assert!(json.contains(&format!("\"result\":\"{result_str}\"")));
        }
    }

    #[test]
    fn test_trajectory_logger_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let _logger = TrajectoryLogger::new(dir.path(), false).unwrap();
        assert!(dir.path().join("trajectory.jsonl").exists());
    }

    #[test]
    fn test_trajectory_logger_verbose_vs_non_verbose() {
        let dir = tempfile::tempdir().unwrap();
        // Non-verbose: build_entry should NOT include raw response
        let logger = TrajectoryLogger::new(dir.path(), false).unwrap();
        let entry = logger.build_entry(1, "text", &[], None, None, "done", Some("raw"));
        assert!(entry.llm_raw_response.is_none());

        // Verbose: build_entry SHOULD include raw response
        let dir2 = tempfile::tempdir().unwrap();
        let logger2 = TrajectoryLogger::new(dir2.path(), true).unwrap();
        let entry2 = logger2.build_entry(1, "text", &[], None, None, "done", Some("raw"));
        assert!(entry2.llm_raw_response.is_some());
    }

    #[test]
    fn test_trajectory_logger_writes_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let mut logger = TrajectoryLogger::new(dir.path(), false).unwrap();

        let entry1 = TrajectoryEntry {
            step: 1,
            timestamp: "2026-02-26T12:00:00Z".to_string(),
            action_code: "pyautogui.click(100, 200)".to_string(),
            thought: Some("Click button".to_string()),
            screenshot_path: Some("step_001.png".to_string()),
            a11y_tree_path: None,
            result: "success".to_string(),
            llm_raw_response: None,
        };

        let entry2 = TrajectoryEntry {
            step: 2,
            timestamp: "2026-02-26T12:00:01Z".to_string(),
            action_code: "pyautogui.press('enter')".to_string(),
            thought: None,
            screenshot_path: Some("step_002.png".to_string()),
            a11y_tree_path: None,
            result: "done".to_string(),
            llm_raw_response: None,
        };

        logger.log_entry(&entry1);
        logger.log_entry(&entry2);

        let content = std::fs::read_to_string(dir.path().join("trajectory.jsonl")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        let parsed1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed1["step"], 1);
        assert_eq!(parsed1["result"], "success");

        let parsed2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(parsed2["step"], 2);
        assert_eq!(parsed2["result"], "done");
    }

    #[test]
    fn test_trajectory_logger_build_entry_basic() {
        let dir = tempfile::tempdir().unwrap();
        let logger = TrajectoryLogger::new(dir.path(), false).unwrap();

        let entry = logger.build_entry(
            1,
            "I see the editor.\n\n```python\npyautogui.click(100, 200)\n```",
            &["pyautogui.click(100, 200)".to_string()],
            Some(&dir.path().join("step_001.png")),
            None,
            "success",
            Some("full response"),
        );

        assert_eq!(entry.step, 1);
        assert_eq!(entry.action_code, "pyautogui.click(100, 200)");
        assert!(entry.thought.is_some());
        assert_eq!(entry.screenshot_path.as_deref(), Some("step_001.png"));
        assert_eq!(entry.result, "success");
        // Not verbose, so no raw response
        assert!(entry.llm_raw_response.is_none());
    }

    #[test]
    fn test_trajectory_logger_build_entry_verbose() {
        let dir = tempfile::tempdir().unwrap();
        let logger = TrajectoryLogger::new(dir.path(), true).unwrap();

        let entry = logger.build_entry(
            1,
            "text",
            &[],
            None,
            None,
            "done",
            Some("full LLM response"),
        );

        assert!(entry.llm_raw_response.is_some());
        assert_eq!(entry.llm_raw_response.as_deref(), Some("full LLM response"));
    }

    #[test]
    fn test_trajectory_logger_build_entry_saves_a11y() {
        let dir = tempfile::tempdir().unwrap();
        let logger = TrajectoryLogger::new(dir.path(), false).unwrap();

        let a11y_text = "button\tOK\t\tGtkButton";
        let entry = logger.build_entry(
            3,
            "text",
            &[],
            None,
            Some(a11y_text),
            "success",
            None,
        );

        assert_eq!(entry.a11y_tree_path.as_deref(), Some("step_003_a11y.txt"));
        // Verify the file was actually written
        let saved = std::fs::read_to_string(dir.path().join("step_003_a11y.txt")).unwrap();
        assert_eq!(saved, a11y_text);
    }

    #[test]
    fn test_chrono_iso8601_now_format() {
        let ts = chrono_iso8601_now();
        // Should match ISO 8601 pattern: YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn test_days_to_date_epoch() {
        let (y, m, d) = days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_date_known() {
        // 2026-02-26 is day 20509 since epoch (calculated)
        // Let's verify a known date: 2000-01-01 = day 10957
        let (y, m, d) = days_to_date(10957);
        assert_eq!((y, m, d), (2000, 1, 1));
    }

    #[test]
    fn test_build_entry_multiple_code_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let logger = TrajectoryLogger::new(dir.path(), false).unwrap();

        let entry = logger.build_entry(
            1,
            "text",
            &[
                "pyautogui.click(100, 200)".to_string(),
                "pyautogui.press('enter')".to_string(),
            ],
            None,
            None,
            "success",
            None,
        );

        // Code blocks should be joined with double newlines
        assert!(entry.action_code.contains("pyautogui.click(100, 200)"));
        assert!(entry.action_code.contains("pyautogui.press('enter')"));
        assert!(entry.action_code.contains("\n\n"));
    }
}
