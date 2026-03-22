//! Generate a self-contained HTML trajectory viewer.
//!
//! Reads trajectory.jsonl and screenshots from an artifacts directory,
//! embeds everything into a single HTML file with inline CSS and vanilla JS.

use std::path::Path;

use tracing::info;

use crate::error::AppError;
use crate::codify::{load_trajectory, TrajectoryRecord};

/// Generate a self-contained HTML review file from test artifacts.
pub fn generate_review_html(
    artifacts_dir: &Path,
    output_path: &Path,
) -> Result<(), AppError> {
    let trajectory_path = artifacts_dir.join("trajectory.jsonl");
    if !trajectory_path.exists() {
        return Err(AppError::Config(format!(
            "No trajectory.jsonl found in '{}'",
            artifacts_dir.display()
        )));
    }

    let entries = load_trajectory(&trajectory_path)?;

    // Load screenshots as base64
    let steps_json = build_steps_json(&entries, artifacts_dir);

    // Embed recording as base64 if present and under 50 MB
    let recording_b64 = {
        let recording_path = artifacts_dir.join("recording.mp4");
        if recording_path.exists() {
            const MAX_EMBED_BYTES: u64 = 50 * 1024 * 1024;
            let size = std::fs::metadata(&recording_path).map(|m| m.len()).unwrap_or(0);
            if size <= MAX_EMBED_BYTES {
                std::fs::read(&recording_path).ok().map(|bytes| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                })
            } else {
                info!("Recording too large to embed ({:.1} MB > 50 MB), skipping", size as f64 / 1_048_576.0);
                None
            }
        } else {
            None
        }
    };

    // Load task.json if present in artifacts, validating as JSON before embedding
    let task_json = {
        let task_path = artifacts_dir.join("task.json");
        if task_path.exists() {
            std::fs::read_to_string(&task_path)
                .ok()
                .and_then(|s| {
                    serde_json::from_str::<serde_json::Value>(&s)
                        .ok()
                        .map(|v| serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()))
                })
                .unwrap_or_else(|| {
                    info!("task.json is malformed, skipping task embedding");
                    "null".to_string()
                })
        } else {
            "null".to_string()
        }
    };
    // Escape closing script tags in JSON
    let task_json = task_json.replace("</", "<\\/");

    let trajectory_path_json = serde_json::to_string(&trajectory_path.to_string_lossy().as_ref())
        .unwrap_or_else(|_| "\"trajectory.jsonl\"".to_string())
        .replace("</", "<\\/");
    let html = build_html(&steps_json, &recording_b64, &trajectory_path_json, &task_json);

    std::fs::write(output_path, &html)
        .map_err(|e| AppError::Infra(format!("Cannot write review HTML: {e}")))?;

    info!("Review HTML written to {}", output_path.display());
    Ok(())
}

/// Build JSON array of step data with embedded screenshots.
fn build_steps_json(entries: &[TrajectoryRecord], artifacts_dir: &Path) -> String {
    let steps: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            let screenshot_b64 = entry.screenshot_path.as_ref().and_then(|p| {
                let full_path = artifacts_dir.join(p);
                std::fs::read(&full_path).ok().map(|bytes| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                })
            });

            serde_json::json!({
                "step": entry.step,
                "timestamp": entry.timestamp,
                "thought": entry.thought,
                "action_code": entry.action_code,
                "result": entry.result,
                "screenshot": screenshot_b64,
                "bash_output": entry.bash_output,
                "error_feedback": entry.error_feedback,
            })
        })
        .collect();

    serde_json::to_string(&steps)
        .unwrap_or_else(|_| "[]".to_string())
        .replace("</", "<\\/")
}

/// Build the complete HTML document using the shared dashboard template.
fn build_html(steps_json: &str, recording_b64: &Option<String>, trajectory_path_json: &str, task_json: &str) -> String {
    let has_recording = recording_b64.is_some();
    let recording_data_uri = recording_b64
        .as_ref()
        .map(|b64| format!("data:video/mp4;base64,{b64}"))
        .unwrap_or_default();

    let template = include_str!("dashboard.html");
    template
        .replace("/*__STEPS__*/[]", &format!("/*__STEPS__*/{steps_json}"))
        .replace(
            "/*__HAS_RECORDING__*/false",
            &format!("/*__HAS_RECORDING__*/{}", if has_recording { "true" } else { "false" }),
        )
        .replace(
            "/*__RECORDING_URI__*/\"\"",
            &format!("/*__RECORDING_URI__*/\"{}\"", recording_data_uri),
        )
        .replace(
            "/*__TRAJECTORY_PATH__*/\"\"",
            &format!("/*__TRAJECTORY_PATH__*/{trajectory_path_json}"),
        )
        .replace(
            "/*__TASK_JSON__*/null",
            &format!("/*__TASK_JSON__*/{task_json}"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_html_contains_structure() {
        let html = build_html("[]", &None, "\"trajectory.jsonl\"", "null");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("desktest"));
        assert!(html.contains("Trajectory Review"));
        assert!(html.contains("STEPS"));
    }

    #[test]
    fn test_build_steps_json_empty() {
        let dir = tempfile::tempdir().unwrap();
        let json = build_steps_json(&[], dir.path());
        assert_eq!(json, "[]");
    }

    #[test]
    fn test_build_steps_json_with_entry() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![TrajectoryRecord {
            step: 1,
            timestamp: "2026-01-01T00:00:00Z".into(),
            action_code: "pyautogui.click(100, 200)".into(),
            thought: Some("Click button".into()),
            screenshot_path: None,
            result: "success".into(),
            bash_output: None,
            error_feedback: None,
        }];
        let json = build_steps_json(&entries, dir.path());
        assert!(json.contains("Click button"));
        assert!(json.contains("pyautogui.click"));
    }

    #[test]
    fn test_generate_review_html() {
        let dir = tempfile::tempdir().unwrap();
        let trajectory = dir.path().join("trajectory.jsonl");
        std::fs::write(&trajectory, "{\"step\":1,\"timestamp\":\"2026-01-01T00:00:00Z\",\"action_code\":\"pyautogui.click(100,200)\",\"result\":\"success\"}\n").unwrap();

        let output = dir.path().join("review.html");
        generate_review_html(dir.path(), &output).unwrap();

        let html = std::fs::read_to_string(&output).unwrap();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("pyautogui.click"));
    }

    #[test]
    fn test_generate_review_html_with_task() {
        let dir = tempfile::tempdir().unwrap();
        let trajectory = dir.path().join("trajectory.jsonl");
        std::fs::write(&trajectory, "{\"step\":1,\"timestamp\":\"2026-01-01T00:00:00Z\",\"action_code\":\"pyautogui.click(100,200)\",\"result\":\"success\"}\n").unwrap();

        // Write a task.json (internally-tagged format matching serde output)
        let task_json = serde_json::json!({
            "schema_version": "1.0",
            "id": "test-task-42",
            "instruction": "Open the file and save it",
            "app": { "type": "appimage", "path": "/tmp/app.AppImage" },
            "timeout": 120,
            "max_steps": 10
        });
        std::fs::write(dir.path().join("task.json"), serde_json::to_string_pretty(&task_json).unwrap()).unwrap();

        let output = dir.path().join("review.html");
        generate_review_html(dir.path(), &output).unwrap();

        let html = std::fs::read_to_string(&output).unwrap();
        assert!(html.contains("test-task-42"));
        assert!(html.contains("Open the file and save it"));
        assert!(html.contains("\"id\":\"test-task-42\""));
    }

    #[test]
    fn test_generate_review_html_with_malformed_task() {
        let dir = tempfile::tempdir().unwrap();
        let trajectory = dir.path().join("trajectory.jsonl");
        std::fs::write(&trajectory, "{\"step\":1,\"timestamp\":\"2026-01-01T00:00:00Z\",\"action_code\":\"pyautogui.click(100,200)\",\"result\":\"success\"}\n").unwrap();

        // Write a malformed task.json
        std::fs::write(dir.path().join("task.json"), "{broken json").unwrap();

        let output = dir.path().join("review.html");
        generate_review_html(dir.path(), &output).unwrap();

        let html = std::fs::read_to_string(&output).unwrap();
        // Should fall back to null, not inject broken content
        assert!(html.contains("TASK_JSON = /*__TASK_JSON__*/null"));
    }

    #[test]
    fn test_generate_review_no_trajectory() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("review.html");
        let err = generate_review_html(dir.path(), &output).unwrap_err();
        assert!(err.to_string().contains("trajectory.jsonl"));
    }
}
