#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::AppError;
use crate::session::{Session, SessionKind};

/// The type of observation to capture after each action.
#[derive(Debug, Default, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationType {
    /// Screenshot only.
    Screenshot,
    /// Accessibility tree only.
    A11yTree,
    /// Both screenshot and accessibility tree (default).
    #[default]
    ScreenshotA11yTree,
}

/// A captured observation from the container.
#[derive(Debug, Clone)]
pub struct Observation {
    /// Local path to the saved screenshot file (None if a11y-tree-only mode).
    pub screenshot_path: Option<PathBuf>,
    /// Base64 data URL of the screenshot (None if a11y-tree-only mode).
    pub screenshot_data_url: Option<String>,
    /// Linearized accessibility tree text (None if screenshot-only mode or extraction failed).
    pub a11y_tree_text: Option<String>,
}

/// Configuration for the observation pipeline.
#[derive(Debug, Clone)]
pub struct ObservationConfig {
    /// What to capture: screenshot, a11y_tree, or both.
    pub observation_type: ObservationType,
    /// Max approximate tokens for the a11y tree (chars / 4). Default: 10_000.
    pub max_a11y_tokens: usize,
    /// Seconds to pause after an action before capturing observation. Default: 2.0.
    pub sleep_after_action: f64,
    /// Timeout for a11y tree extraction. Default: 15s.
    pub a11y_timeout: Duration,
    /// Maximum number of a11y tree nodes to extract (0 = unlimited). Default: 10_000.
    pub max_a11y_nodes: usize,
    /// Command to capture a screenshot to `/tmp/screenshot.png`.
    /// Default (Linux): `["scrot", "-o", "-p", "/tmp/screenshot.png"]`
    pub screenshot_cmd: Vec<String>,
    /// Base command for a11y tree extraction (`--max-nodes N` appended automatically).
    /// Default (Linux): `["/usr/local/bin/get-a11y-tree"]`
    pub a11y_cmd: Vec<String>,
}

/// Default screenshot command for Linux (scrot).
pub const LINUX_SCREENSHOT_CMD: &[&str] = &["scrot", "-o", "-p", "/tmp/screenshot.png"];

/// Default a11y tree command for Linux (pyatspi-based script).
pub const LINUX_A11Y_CMD: &[&str] = &["/usr/local/bin/get-a11y-tree"];

/// Screenshot command for macOS (screencapture).
pub const MACOS_SCREENSHOT_CMD: &[&str] = &["screencapture", "-x", "/tmp/screenshot.png"];

/// A11y tree command for macOS (Swift AXUIElement helper).
pub const MACOS_A11Y_CMD: &[&str] = &["/usr/local/bin/a11y-helper"];

impl Default for ObservationConfig {
    fn default() -> Self {
        ObservationConfig {
            observation_type: ObservationType::default(),
            max_a11y_tokens: 10_000,
            sleep_after_action: 2.0,
            a11y_timeout: Duration::from_secs(15),
            max_a11y_nodes: 10_000,
            screenshot_cmd: LINUX_SCREENSHOT_CMD
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            a11y_cmd: LINUX_A11Y_CMD.iter().map(|s| (*s).to_string()).collect(),
        }
    }
}

impl ObservationConfig {
    /// Create an `ObservationConfig` with commands appropriate for the session type.
    pub fn for_session(session: &SessionKind) -> Self {
        match session {
            SessionKind::Docker(_) => Self::default(),
            SessionKind::Tart(_) => Self {
                screenshot_cmd: MACOS_SCREENSHOT_CMD
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                a11y_cmd: MACOS_A11Y_CMD.iter().map(|s| (*s).to_string()).collect(),
                ..Self::default()
            },
        }
    }
}

/// Screenshot capture retry configuration.
const SCREENSHOT_MAX_RETRIES: u32 = 3;
const SCREENSHOT_RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// Capture an observation from the container according to the given config.
///
/// When configured for both screenshot and a11y tree, captures are run in parallel.
/// If a11y tree extraction fails, falls back to screenshot-only with a warning.
pub async fn capture_observation(
    session: &SessionKind,
    artifacts_dir: &Path,
    step_index: usize,
    config: &ObservationConfig,
) -> Result<Observation, AppError> {
    // Wait after action before capturing
    if config.sleep_after_action > 0.0 {
        debug!(
            "Waiting {:.1}s before capturing observation",
            config.sleep_after_action
        );
        tokio::time::sleep(Duration::from_secs_f64(config.sleep_after_action)).await;
    }

    match config.observation_type {
        ObservationType::Screenshot => {
            let (path, data_url) = capture_screenshot_with_retry(
                session,
                artifacts_dir,
                step_index,
                &config.screenshot_cmd,
            )
            .await?;
            Ok(Observation {
                screenshot_path: Some(path),
                screenshot_data_url: Some(data_url),
                a11y_tree_text: None,
            })
        }
        ObservationType::A11yTree => {
            let a11y_text = extract_a11y_tree(
                session,
                config.max_a11y_tokens,
                config.a11y_timeout,
                config.max_a11y_nodes,
                &config.a11y_cmd,
            )
            .await;
            match a11y_text {
                Ok(text) => Ok(Observation {
                    screenshot_path: None,
                    screenshot_data_url: None,
                    a11y_tree_text: Some(text),
                }),
                Err(e) => {
                    warn!("A11y tree extraction failed in a11y-only mode: {e}");
                    // In a11y-only mode, a failure is still an error since there's no fallback
                    Err(AppError::Infra(format!("A11y tree extraction failed: {e}")))
                }
            }
        }
        ObservationType::ScreenshotA11yTree => {
            // Capture screenshot and a11y tree in parallel
            let (screenshot_result, a11y_result) = tokio::join!(
                capture_screenshot_with_retry(
                    session,
                    artifacts_dir,
                    step_index,
                    &config.screenshot_cmd
                ),
                extract_a11y_tree(
                    session,
                    config.max_a11y_tokens,
                    config.a11y_timeout,
                    config.max_a11y_nodes,
                    &config.a11y_cmd,
                ),
            );

            let (path, data_url) = screenshot_result?;

            let a11y_text = match a11y_result {
                Ok(text) => Some(text),
                Err(e) => {
                    warn!("A11y tree extraction failed, falling back to screenshot-only: {e}");
                    None
                }
            };

            Ok(Observation {
                screenshot_path: Some(path),
                screenshot_data_url: Some(data_url),
                a11y_tree_text: a11y_text,
            })
        }
    }
}

/// Capture a screenshot with retry logic: 3 attempts, 5s interval between retries.
///
/// Returns the local file path and a base64 data URL.
pub async fn capture_screenshot_with_retry(
    session: &SessionKind,
    artifacts_dir: &Path,
    step_index: usize,
    screenshot_cmd: &[String],
) -> Result<(PathBuf, String), AppError> {
    let mut last_err = None;

    for attempt in 1..=SCREENSHOT_MAX_RETRIES {
        match capture_screenshot_once(session, artifacts_dir, step_index, screenshot_cmd).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                warn!(
                    "Screenshot capture attempt {}/{} failed: {}",
                    attempt, SCREENSHOT_MAX_RETRIES, e
                );
                last_err = Some(e);
                if attempt < SCREENSHOT_MAX_RETRIES {
                    tokio::time::sleep(SCREENSHOT_RETRY_INTERVAL).await;
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| AppError::Infra("Screenshot capture failed".into())))
}

/// Single attempt to capture a screenshot from the container.
async fn capture_screenshot_once(
    session: &SessionKind,
    artifacts_dir: &Path,
    step_index: usize,
    screenshot_cmd: &[String],
) -> Result<(PathBuf, String), AppError> {
    // Capture screenshot inside container
    let cmd_refs: Vec<&str> = screenshot_cmd.iter().map(|s| s.as_str()).collect();
    session.exec(&cmd_refs).await?;

    // Copy from container to host with step_NNN naming convention
    let local_path = artifacts_dir.join(format!("step_{:03}.png", step_index));
    session
        .copy_from("/tmp/screenshot.png", &local_path)
        .await?;

    // Read and encode as base64 data URL
    let bytes = std::fs::read(&local_path)
        .map_err(|e| AppError::Infra(format!("Cannot read screenshot: {e}")))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let data_url = format!("data:image/png;base64,{b64}");

    debug!("Screenshot captured: {}", local_path.display());
    Ok((local_path, data_url))
}

/// Extract the accessibility tree from the container via the helper script.
///
/// The a11y tree is trimmed to `max_tokens` approximate tokens (chars / 4).
/// Returns empty-string errors as failures (empty tree = no useful data).
async fn extract_a11y_tree(
    session: &SessionKind,
    max_tokens: usize,
    a11y_timeout: Duration,
    max_a11y_nodes: usize,
    a11y_cmd: &[String],
) -> Result<String, AppError> {
    let mut cmd: Vec<&str> = a11y_cmd.iter().map(|s| s.as_str()).collect();
    let max_nodes_str = max_a11y_nodes.to_string();
    if max_a11y_nodes > 0 {
        cmd.push("--max-nodes");
        cmd.push(&max_nodes_str);
    }
    let timeout_secs = a11y_timeout.as_secs();
    let output = tokio::time::timeout(a11y_timeout, session.exec(&cmd))
        .await
        .map_err(|_| {
            AppError::Infra(format!(
                "A11y tree extraction timed out after {timeout_secs}s"
            ))
        })?
        .map_err(|e| AppError::Infra(format!("A11y tree extraction failed: {e}")))?;

    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(AppError::Infra(
            "A11y tree extraction returned empty output".into(),
        ));
    }

    // Trim to max tokens (approximate: 1 token ~ 4 chars)
    let max_chars = max_tokens * 4;
    let result = if trimmed.len() > max_chars {
        debug!(
            "A11y tree trimmed from {} to {} chars (~{} tokens)",
            trimmed.len(),
            max_chars,
            max_tokens
        );
        let truncated = &trimmed[..max_chars];
        // Try to cut at a line boundary to avoid partial lines
        match truncated.rfind('\n') {
            Some(pos) if pos > max_chars / 2 => truncated[..pos].to_string(),
            _ => truncated.to_string(),
        }
    } else {
        trimmed.to_string()
    };

    debug!("A11y tree extracted: {} chars", result.len());
    Ok(result)
}

/// Trim an a11y tree string to a maximum number of approximate tokens.
///
/// Exposed for unit testing.
pub(crate) fn trim_a11y_tree(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        return text.to_string();
    }

    let truncated = &text[..max_chars];
    match truncated.rfind('\n') {
        Some(pos) if pos > max_chars / 2 => truncated[..pos].to_string(),
        _ => truncated.to_string(),
    }
}

/// Probe the a11y tree extraction to measure how long it takes.
///
/// Runs one extraction with a generous 60s timeout and returns the measured
/// wall-clock duration. This is used at startup to adaptively set the a11y
/// timeout for the agent loop.
pub async fn probe_a11y_timing(
    session: &SessionKind,
    max_a11y_nodes: usize,
    max_tokens: usize,
    a11y_cmd: &[String],
) -> Result<Duration, AppError> {
    let start = std::time::Instant::now();
    let probe_timeout = Duration::from_secs(60);
    let _ = extract_a11y_tree(session, max_tokens, probe_timeout, max_a11y_nodes, a11y_cmd).await?;
    Ok(start.elapsed())
}

/// Save an a11y tree to a file in the artifacts directory.
pub async fn save_a11y_tree(
    artifacts_dir: &Path,
    step_index: usize,
    a11y_text: &str,
) -> Result<PathBuf, AppError> {
    let path = artifacts_dir.join(format!("step_{:03}_a11y.txt", step_index));
    std::fs::write(&path, a11y_text)
        .map_err(|e| AppError::Infra(format!("Cannot write a11y tree: {e}")))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observation_type_default() {
        let obs_type = ObservationType::default();
        assert_eq!(obs_type, ObservationType::ScreenshotA11yTree);
    }

    #[test]
    fn test_observation_config_default() {
        let config = ObservationConfig::default();
        assert_eq!(config.observation_type, ObservationType::ScreenshotA11yTree);
        assert_eq!(config.max_a11y_tokens, 10_000);
        assert_eq!(config.sleep_after_action, 2.0);
        assert_eq!(config.a11y_timeout, Duration::from_secs(15));
        assert_eq!(config.max_a11y_nodes, 10_000);
    }

    #[test]
    fn test_observation_type_deserialize_screenshot() {
        let val: ObservationType = serde_json::from_str(r#""screenshot""#).unwrap();
        assert_eq!(val, ObservationType::Screenshot);
    }

    #[test]
    fn test_observation_type_deserialize_a11y_tree() {
        let val: ObservationType = serde_json::from_str(r#""a11y_tree""#).unwrap();
        assert_eq!(val, ObservationType::A11yTree);
    }

    #[test]
    fn test_observation_type_deserialize_screenshot_a11y_tree() {
        let val: ObservationType = serde_json::from_str(r#""screenshot_a11y_tree""#).unwrap();
        assert_eq!(val, ObservationType::ScreenshotA11yTree);
    }

    #[test]
    fn test_observation_type_serialize() {
        let json = serde_json::to_string(&ObservationType::Screenshot).unwrap();
        assert_eq!(json, r#""screenshot""#);

        let json = serde_json::to_string(&ObservationType::A11yTree).unwrap();
        assert_eq!(json, r#""a11y_tree""#);

        let json = serde_json::to_string(&ObservationType::ScreenshotA11yTree).unwrap();
        assert_eq!(json, r#""screenshot_a11y_tree""#);
    }

    #[test]
    fn test_trim_a11y_tree_no_trim_needed() {
        let text = "line1\nline2\nline3";
        let result = trim_a11y_tree(text, 10_000);
        assert_eq!(result, text);
    }

    #[test]
    fn test_trim_a11y_tree_trims_at_line_boundary() {
        // Create text that's over the limit
        // max_tokens=2 means max_chars=8
        let text = "aaa\nbbb\nccc\nddd";
        let result = trim_a11y_tree(text, 2);
        // max_chars=8, text[..8] = "aaa\nbbb\n", rfind('\n') at pos 7
        // pos 7 > 4 (half of 8), so we cut at pos 7
        assert_eq!(result, "aaa\nbbb");
    }

    #[test]
    fn test_trim_a11y_tree_no_good_line_boundary() {
        // max_tokens=1 means max_chars=4
        // text[..4] = "abcd", no newline, so we take the full truncated string
        let text = "abcdefghij";
        let result = trim_a11y_tree(text, 1);
        assert_eq!(result, "abcd");
    }

    #[test]
    fn test_trim_a11y_tree_empty() {
        let result = trim_a11y_tree("", 10_000);
        assert_eq!(result, "");
    }

    #[test]
    fn test_trim_a11y_tree_exact_boundary() {
        // Exactly at the limit — no trimming
        let text = "abcd"; // 4 chars = 1 token
        let result = trim_a11y_tree(text, 1);
        assert_eq!(result, "abcd");
    }

    #[test]
    fn test_trim_a11y_tree_newline_in_first_half_ignored() {
        // max_tokens=2 means max_chars=8
        // text[..8] = "ab\ncdefg", rfind('\n') at pos 2
        // pos 2 <= 4 (half of 8), so we DON'T cut at line boundary — take full truncated
        let text = "ab\ncdefghijk";
        let result = trim_a11y_tree(text, 2);
        assert_eq!(result, "ab\ncdefg");
    }

    #[test]
    fn test_observation_struct_fields() {
        let obs = Observation {
            screenshot_path: Some(PathBuf::from("/tmp/test.png")),
            screenshot_data_url: Some("data:image/png;base64,abc".into()),
            a11y_tree_text: Some("button\tOK\t\tGtkButton".into()),
        };
        assert!(obs.screenshot_path.is_some());
        assert!(obs.screenshot_data_url.is_some());
        assert!(obs.a11y_tree_text.is_some());
    }

    #[test]
    fn test_observation_struct_screenshot_only() {
        let obs = Observation {
            screenshot_path: Some(PathBuf::from("/tmp/test.png")),
            screenshot_data_url: Some("data:image/png;base64,abc".into()),
            a11y_tree_text: None,
        };
        assert!(obs.screenshot_path.is_some());
        assert!(obs.a11y_tree_text.is_none());
    }

    #[test]
    fn test_observation_struct_a11y_only() {
        let obs = Observation {
            screenshot_path: None,
            screenshot_data_url: None,
            a11y_tree_text: Some("panel\troot".into()),
        };
        assert!(obs.screenshot_path.is_none());
        assert!(obs.screenshot_data_url.is_none());
        assert!(obs.a11y_tree_text.is_some());
    }

    #[tokio::test]
    async fn test_save_a11y_tree() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_a11y_tree(dir.path(), 5, "button\tOK\t\tGtkButton")
            .await
            .unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "step_005_a11y.txt"
        );
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "button\tOK\t\tGtkButton");
    }

    #[tokio::test]
    async fn test_save_a11y_tree_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_a11y_tree(dir.path(), 0, "test data").await.unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_observation_config_custom() {
        let config = ObservationConfig {
            observation_type: ObservationType::Screenshot,
            max_a11y_tokens: 5_000,
            sleep_after_action: 1.0,
            a11y_timeout: Duration::from_secs(30),
            max_a11y_nodes: 5_000,
            ..ObservationConfig::default()
        };
        assert_eq!(config.observation_type, ObservationType::Screenshot);
        assert_eq!(config.max_a11y_tokens, 5_000);
        assert_eq!(config.sleep_after_action, 1.0);
        assert_eq!(config.a11y_timeout, Duration::from_secs(30));
        assert_eq!(config.max_a11y_nodes, 5_000);
    }

    #[test]
    fn test_observation_config_default_commands() {
        let config = ObservationConfig::default();
        assert_eq!(
            config.screenshot_cmd,
            vec!["scrot", "-o", "-p", "/tmp/screenshot.png"]
        );
        assert_eq!(config.a11y_cmd, vec!["/usr/local/bin/get-a11y-tree"]);
    }

    #[test]
    fn test_observation_config_macos_commands() {
        let macos_screenshot: Vec<String> = MACOS_SCREENSHOT_CMD
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let macos_a11y: Vec<String> = MACOS_A11Y_CMD.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(
            macos_screenshot,
            vec!["screencapture", "-x", "/tmp/screenshot.png"]
        );
        assert_eq!(macos_a11y, vec!["/usr/local/bin/a11y-helper"]);
    }

    #[test]
    fn test_screenshot_commands_write_to_same_path() {
        // Both Linux and macOS screenshot commands must write to /tmp/screenshot.png
        // so that copy_from picks up the file from the same location.
        let linux_last = LINUX_SCREENSHOT_CMD.last().unwrap();
        let macos_last = MACOS_SCREENSHOT_CMD.last().unwrap();
        assert_eq!(*linux_last, "/tmp/screenshot.png");
        assert_eq!(*macos_last, "/tmp/screenshot.png");
    }

    #[test]
    fn test_for_session_docker_returns_linux_defaults() {
        // Cannot construct a real DockerSession in a unit test, so verify
        // that Default (used for Docker) returns Linux commands.
        let config = ObservationConfig::default();
        assert_eq!(
            config.screenshot_cmd,
            LINUX_SCREENSHOT_CMD
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            config.a11y_cmd,
            LINUX_A11Y_CMD
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_macos_config_overrides_commands() {
        // Verify that constructing with macOS commands produces different
        // values from the Linux default — simulates what for_session(Tart) does.
        let macos_config = ObservationConfig {
            screenshot_cmd: MACOS_SCREENSHOT_CMD
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            a11y_cmd: MACOS_A11Y_CMD.iter().map(|s| (*s).to_string()).collect(),
            ..ObservationConfig::default()
        };
        let linux_config = ObservationConfig::default();

        // Commands differ
        assert_ne!(macos_config.screenshot_cmd, linux_config.screenshot_cmd);
        assert_ne!(macos_config.a11y_cmd, linux_config.a11y_cmd);

        // But non-command fields are identical
        assert_eq!(macos_config.observation_type, linux_config.observation_type);
        assert_eq!(macos_config.max_a11y_tokens, linux_config.max_a11y_tokens);
        assert_eq!(
            macos_config.sleep_after_action,
            linux_config.sleep_after_action
        );
        assert_eq!(macos_config.a11y_timeout, linux_config.a11y_timeout);
        assert_eq!(macos_config.max_a11y_nodes, linux_config.max_a11y_nodes);
    }

    #[test]
    fn test_trim_a11y_tree_large_text() {
        // Simulate a large a11y tree
        let lines: Vec<String> = (0..1000)
            .map(|i| {
                format!(
                    "button\tButton_{}\t\tGtkButton\tdescription\t100,{}\t50,20",
                    i,
                    i * 25
                )
            })
            .collect();
        let text = lines.join("\n");

        // Trim to 100 tokens (400 chars)
        let result = trim_a11y_tree(&text, 100);
        assert!(result.len() <= 400);
        // Should end at a line boundary
        assert!(!result.ends_with('\n'));
        assert!(result.contains("button\tButton_"));
    }
}
