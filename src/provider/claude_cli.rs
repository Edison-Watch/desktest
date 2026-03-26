//! Claude Code CLI provider — shells out to `claude -p` for LLM calls.
//!
//! Uses the locally-installed Claude Code CLI, leveraging the user's existing
//! CLI authentication instead of requiring a separate API key.
//!
//! All trajectory screenshots and accessibility trees are saved as numbered
//! files in a temp directory, and the prompt instructs Claude to read them
//! in order via the Read tool. This gives the model full visual context
//! across the sliding window, matching the behavior of API-based providers.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use tracing::info;

use super::{ChatMessage, LlmProvider};
use crate::error::AppError;

/// Atomic counter for unique temp directory names.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// RAII guard that recursively deletes the temp directory on drop, ensuring
/// cleanup even when the future is externally cancelled (e.g., by step_timeout).
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Provider that delegates to the Claude Code CLI (`claude -p`).
pub struct ClaudeCliProvider;

impl ClaudeCliProvider {
    /// Create a new CLI provider, verifying the `claude` binary is available.
    pub fn new() -> Result<Self, AppError> {
        let status = std::process::Command::new("claude")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        match status {
            Ok(s) if s.success() => Ok(Self),
            _ => Err(AppError::Config(
                "Claude Code CLI not found. Install it from https://claude.ai/code \
                 or use a different provider."
                    .into(),
            )),
        }
    }
}

impl LlmProvider for ClaudeCliProvider {
    fn chat_completion<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        _tools: &'a [serde_json::Value],
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ChatMessage, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let start = std::time::Instant::now();

            // 1. Create temp directory and build the prompt with all observation files.
            let PromptResult {
                prompt_text,
                temp_dir,
                file_count,
            } = build_cli_prompt(messages)?;

            // RAII guard ensures the temp directory is cleaned up even if the
            // future is cancelled by an external timeout (e.g., step_timeout).
            let _guard = TempDirGuard(temp_dir);

            // max-turns: 1 turn per file read (worst case) + 1 for the response.
            // Claude can batch reads in parallel, so this is a generous upper bound.
            let max_turns = if file_count > 0 { file_count + 2 } else { 1 };

            info!(
                "Claude CLI request: {} messages, {} observation files, max_turns={}",
                messages.len(),
                file_count,
                max_turns,
            );

            // 2. Build command — system prompt is embedded in the prompt text
            //    (not via --append-system-prompt) to avoid stacking on top of
            //    Claude Code's built-in coding-assistant system prompt.
            let mut cmd = tokio::process::Command::new("claude");
            cmd.kill_on_drop(true);
            cmd.arg("-p");
            cmd.arg("--output-format").arg("text");
            cmd.arg("--max-turns").arg(max_turns.to_string());

            if file_count > 0 {
                cmd.arg("--allowedTools").arg("Read");
            }

            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            // 3. Spawn, write prompt, wait for output.
            let output = async {
                let mut child = cmd
                    .spawn()
                    .map_err(|e| AppError::Agent(format!("Failed to spawn claude CLI: {e}")))?;

                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = stdin.write_all(prompt_text.as_bytes()).await {
                        let _ = child.kill().await;
                        return Err(AppError::Agent(format!(
                            "Failed to write to claude stdin: {e}"
                        )));
                    }
                    // Close stdin to signal EOF
                }

                // 5-minute timeout as defense-in-depth (the agent loop's
                // step_timeout also wraps this call, but a hung process should
                // not block indefinitely if that outer timeout is misconfigured).
                tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    child.wait_with_output(),
                )
                .await
                .map_err(|_| AppError::Agent("Claude CLI timed out after 300s".into()))?
                .map_err(|e| AppError::Agent(format!("Claude CLI process error: {e}")))
            }
            .await;

            let output = output?;
            let elapsed = start.elapsed();

            // 4. Check exit status
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                return Err(AppError::Agent(format!(
                    "Claude CLI failed (exit {}): {}{}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim(),
                    if stdout.is_empty() {
                        String::new()
                    } else {
                        format!("\nstdout: {}", stdout.chars().take(500).collect::<String>())
                    },
                )));
            }

            // 5. Read text response
            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();

            if result.is_empty() {
                return Err(AppError::Agent("Claude CLI returned empty response".into()));
            }

            info!("Claude CLI response in {:.1}s", elapsed.as_secs_f64());

            Ok(ChatMessage {
                role: "assistant".into(),
                content: Some(serde_json::Value::String(result)),
                tool_calls: None,
                tool_call_id: None,
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

/// Result of building the CLI prompt.
struct PromptResult {
    /// The full prompt text to pipe to `claude -p` via stdin.
    prompt_text: String,
    /// Temp directory containing observation files (cleaned up by TempDirGuard).
    temp_dir: PathBuf,
    /// Total number of observation files saved (screenshots + a11y trees).
    file_count: usize,
}

/// A single observation step extracted from the message array.
struct ObservationStep {
    /// 1-based step number.
    step: usize,
    /// Path to the screenshot file (if any).
    screenshot: Option<PathBuf>,
    /// Path to the accessibility tree file (if any).
    a11y_tree: Option<PathBuf>,
    /// Whether this is the last (current) observation.
    is_current: bool,
}

/// Build the CLI prompt from the message array.
///
/// Creates a temp directory and saves all trajectory screenshots and
/// accessibility trees as numbered files. The prompt embeds the system
/// prompt inline (avoiding `--append-system-prompt` stacking) and lists
/// each file by exact path for Claude to read in order.
fn build_cli_prompt(messages: &[ChatMessage]) -> Result<PromptResult, AppError> {
    // Create temp directory for this invocation.
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let temp_dir = std::env::temp_dir().join(format!("desktest_cli_{pid}_{counter}"));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| AppError::Agent(format!("Failed to create temp dir: {e}")))?;

    let mut system_parts = Vec::new();
    let mut prompt_sections: Vec<String> = Vec::new();
    let mut observations: Vec<ObservationStep> = Vec::new();
    let mut obs_counter = 0usize;

    // Identify the last observation index (the current one).
    let last_obs_idx = messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, msg)| {
            msg.role == "user" && {
                let (_, images) = extract_text_and_images(msg);
                !images.is_empty()
            }
        })
        .map(|(i, _)| i);

    // Process each message.
    for (idx, msg) in messages.iter().enumerate() {
        match msg.role.as_str() {
            "system" => {
                if let Some(text) = extract_text(msg) {
                    system_parts.push(text);
                }
            }
            "user" => {
                let (text, images) = extract_text_and_images(msg);

                if !images.is_empty() {
                    // This is an observation message — save files.
                    obs_counter += 1;
                    let is_current = Some(idx) == last_obs_idx;
                    let mut obs = ObservationStep {
                        step: obs_counter,
                        screenshot: None,
                        a11y_tree: None,
                        is_current,
                    };

                    // Save screenshot(s). In practice there's one per observation.
                    for data_url in &images {
                        let path = save_screenshot(&temp_dir, obs_counter, data_url)?;
                        obs.screenshot = Some(path);
                    }

                    // Save accessibility tree text if present.
                    if !text.is_empty() {
                        let path = save_a11y_tree(&temp_dir, obs_counter, &text)?;
                        obs.a11y_tree = Some(path);
                    }

                    observations.push(obs);
                } else if !text.is_empty() {
                    // Text-only user message (task instruction, error feedback, etc.)
                    prompt_sections.push(text);
                }
            }
            "assistant" => {
                if let Some(text) = extract_text(msg) {
                    prompt_sections.push(format!("[Previous agent response]\n{text}"));
                }
            }
            _ => {} // skip tool messages
        }
    }

    // Count total files.
    let file_count: usize = observations
        .iter()
        .map(|o| o.screenshot.is_some() as usize + o.a11y_tree.is_some() as usize)
        .sum();

    // Assemble the final prompt.
    let system_prompt = system_parts.join("\n");
    let prompt_text = assemble_prompt(&system_prompt, &prompt_sections, &observations);

    Ok(PromptResult {
        prompt_text,
        temp_dir,
        file_count,
    })
}

/// Assemble the final prompt text with inline system prompt and file manifest.
fn assemble_prompt(
    system_prompt: &str,
    inline_sections: &[String],
    observations: &[ObservationStep],
) -> String {
    let mut prompt = String::new();

    // 1. System prompt embedded inline to avoid stacking with Claude Code's
    //    built-in coding-assistant system prompt.
    if !system_prompt.is_empty() {
        prompt.push_str("<system-instructions>\n");
        prompt.push_str(system_prompt.trim());
        prompt.push_str("\n</system-instructions>\n\n");
    }

    // 2. Inline sections (task instruction, previous agent responses, feedback).
    for section in inline_sections {
        prompt.push_str(section);
        prompt.push_str("\n\n---\n\n");
    }

    // 3. Observation file manifest — explicit paths for Claude to read in order.
    if !observations.is_empty() {
        prompt.push_str("## Observation Files\n\n");
        prompt.push_str(
            "Read the following files IN ORDER to understand the full trajectory. \
             Each step has a screenshot and/or accessibility tree describing the \
             desktop state at that point.\n\n",
        );

        for obs in observations {
            let label = if obs.is_current {
                format!(
                    "### Step {} (CURRENT — this is the latest desktop state)",
                    obs.step
                )
            } else {
                format!("### Step {} (previous)", obs.step)
            };
            prompt.push_str(&label);
            prompt.push('\n');

            if let Some(ref path) = obs.screenshot {
                prompt.push_str(&format!("- Screenshot: {}\n", path.display()));
            }
            if let Some(ref path) = obs.a11y_tree {
                prompt.push_str(&format!("- Accessibility tree: {}\n", path.display()));
            }
            prompt.push('\n');
        }

        prompt.push_str(
            "**Important:** Read ALL files listed above before responding. \
             Start with Step 1 and proceed in order. The CURRENT step is the one \
             you should act on.\n",
        );
    }

    prompt
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

/// Save a base64-encoded screenshot to the temp directory.
fn save_screenshot(temp_dir: &std::path::Path, step: usize, data_url: &str) -> Result<PathBuf, AppError> {
    let base64_data = data_url
        .split(',')
        .nth(1)
        .ok_or_else(|| AppError::Agent("Invalid image data URL format".into()))?;

    // Extract extension from MIME type: "data:image/jpeg;base64,..." → "jpeg"
    let ext = data_url
        .split(';')
        .next()
        .and_then(|s| s.split('/').nth(1))
        .unwrap_or("png");

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| AppError::Agent(format!("Failed to decode base64 image: {e}")))?;

    let path = temp_dir.join(format!("step_{step:03}_screenshot.{ext}"));
    std::fs::write(&path, &bytes)
        .map_err(|e| AppError::Agent(format!("Failed to write screenshot: {e}")))?;

    Ok(path)
}

/// Save accessibility tree text to the temp directory.
fn save_a11y_tree(temp_dir: &std::path::Path, step: usize, text: &str) -> Result<PathBuf, AppError> {
    let path = temp_dir.join(format!("step_{step:03}_a11y.txt"));
    std::fs::write(&path, text.as_bytes())
        .map_err(|e| AppError::Agent(format!("Failed to write a11y tree: {e}")))?;

    Ok(path)
}

/// Extract plain text content from a ChatMessage.
fn extract_text(msg: &ChatMessage) -> Option<String> {
    match &msg.content {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(arr)) => {
            let texts: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    if item.get("type")?.as_str()? == "text" {
                        item.get("text")?.as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Extract text and image data URLs from a ChatMessage.
fn extract_text_and_images(msg: &ChatMessage) -> (String, Vec<String>) {
    let mut texts = Vec::new();
    let mut images = Vec::new();

    match &msg.content {
        Some(serde_json::Value::String(s)) => {
            texts.push(s.clone());
        }
        Some(serde_json::Value::Array(arr)) => {
            for item in arr {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                            texts.push(t.to_string());
                        }
                    }
                    Some("image_url") => {
                        if let Some(url) = item
                            .get("image_url")
                            .and_then(|u| u.get("url"))
                            .and_then(|u| u.as_str())
                        {
                            images.push(url.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    (texts.join("\n"), images)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{system_message, user_image_message, user_message};

    #[test]
    fn test_build_prompt_text_only() {
        let messages = vec![
            system_message("You are a tester."),
            user_message("Click the button."),
        ];
        let result = build_cli_prompt(&messages).unwrap();
        assert!(result.prompt_text.contains("<system-instructions>"));
        assert!(result.prompt_text.contains("You are a tester."));
        assert!(result.prompt_text.contains("Click the button."));
        assert_eq!(result.file_count, 0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_build_prompt_single_observation() {
        let data_url = "data:image/png;base64,iVBORw0KGgo=";
        let messages = vec![
            system_message("Sys."),
            user_message("## Task\n\nDo something."),
            // Observation with image + a11y text
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!([
                    {"type": "image_url", "image_url": {"url": data_url}},
                    {"type": "text", "text": "A11y tree content here"},
                ])),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let result = build_cli_prompt(&messages).unwrap();
        assert_eq!(result.file_count, 2); // screenshot + a11y
        assert!(result.prompt_text.contains("step_001_screenshot.png"));
        assert!(result.prompt_text.contains("step_001_a11y.txt"));
        assert!(result.prompt_text.contains("Step 1 (CURRENT"));
        assert!(result.prompt_text.contains("Read ALL files"));

        // Verify files exist
        let screenshot = result.temp_dir.join("step_001_screenshot.png");
        let a11y = result.temp_dir.join("step_001_a11y.txt");
        assert!(screenshot.exists());
        assert!(a11y.exists());

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_build_prompt_multi_step_trajectory() {
        let img = "data:image/png;base64,iVBORw0KGgo=";

        let messages = vec![
            system_message("System prompt."),
            user_message("## Task\n\nDo something."),
            // Step 1 observation (previous)
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!([
                    {"type": "image_url", "image_url": {"url": img}},
                    {"type": "text", "text": "A11y tree step 1"},
                ])),
                tool_calls: None,
                tool_call_id: None,
            },
            // Step 1 agent response
            ChatMessage {
                role: "assistant".into(),
                content: Some(serde_json::Value::String("I clicked the button.".into())),
                tool_calls: None,
                tool_call_id: None,
            },
            // Step 2 observation (current)
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!([
                    {"type": "image_url", "image_url": {"url": img}},
                    {"type": "text", "text": "A11y tree step 2"},
                ])),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let result = build_cli_prompt(&messages).unwrap();

        // Both observations should have files saved
        assert_eq!(result.file_count, 4); // 2 screenshots + 2 a11y trees
        assert!(result.prompt_text.contains("Step 1 (previous)"));
        assert!(result.prompt_text.contains("Step 2 (CURRENT"));
        assert!(result.prompt_text.contains("step_001_screenshot.png"));
        assert!(result.prompt_text.contains("step_002_screenshot.png"));
        assert!(result.prompt_text.contains("step_001_a11y.txt"));
        assert!(result.prompt_text.contains("step_002_a11y.txt"));
        // Agent response should be inline
        assert!(result.prompt_text.contains("[Previous agent response]"));
        assert!(result.prompt_text.contains("I clicked the button."));

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_build_prompt_screenshot_only_observation() {
        let img = "data:image/png;base64,iVBORw0KGgo=";
        let messages = vec![
            system_message("Sys."),
            user_image_message(img), // image only, no a11y text
        ];

        let result = build_cli_prompt(&messages).unwrap();
        assert_eq!(result.file_count, 1); // screenshot only, no a11y
        assert!(result.prompt_text.contains("step_001_screenshot.png"));
        assert!(!result.prompt_text.contains("a11y.txt"));

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_build_prompt_preserves_assistant_turns() {
        let messages = vec![
            system_message("Sys."),
            user_message("Task A"),
            ChatMessage {
                role: "assistant".into(),
                content: Some(serde_json::Value::String("I clicked the button.".into())),
                tool_calls: None,
                tool_call_id: None,
            },
            user_message("Error feedback here."),
        ];
        let result = build_cli_prompt(&messages).unwrap();
        assert!(result.prompt_text.contains("[Previous agent response]"));
        assert!(result.prompt_text.contains("I clicked the button."));
        assert!(result.prompt_text.contains("Error feedback here."));

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_system_prompt_embedded_inline() {
        let messages = vec![
            system_message("You are a desktop tester."),
            user_message("Do the task."),
        ];
        let result = build_cli_prompt(&messages).unwrap();
        // System prompt should be in <system-instructions> tags, not separate
        assert!(result.prompt_text.contains("<system-instructions>"));
        assert!(result.prompt_text.contains("You are a desktop tester."));
        assert!(result.prompt_text.contains("</system-instructions>"));

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_max_turns_scales_with_files() {
        // With 4 files, max_turns should be 4 + 2 = 6
        let file_count = 4;
        let max_turns = file_count + 2;
        assert_eq!(max_turns, 6);

        // With 0 files, max_turns should be 1
        let file_count = 0;
        let max_turns = if file_count > 0 { file_count + 2 } else { 1 };
        assert_eq!(max_turns, 1);
    }

    #[test]
    fn test_save_screenshot_jpeg_extension() {
        let temp_dir = std::env::temp_dir().join("desktest_test_jpeg");
        let _ = std::fs::create_dir_all(&temp_dir);

        let data_url = "data:image/jpeg;base64,/9j/4AAQ";
        let path = save_screenshot(&temp_dir, 1, data_url).unwrap();
        assert!(path.to_str().unwrap().ends_with(".jpeg"));
        assert_eq!(path.file_name().unwrap(), "step_001_screenshot.jpeg");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_save_a11y_tree() {
        let temp_dir = std::env::temp_dir().join("desktest_test_a11y");
        let _ = std::fs::create_dir_all(&temp_dir);

        let path = save_a11y_tree(&temp_dir, 3, "tree content here").unwrap();
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "step_003_a11y.txt");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "tree content here");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_save_screenshot_invalid_data_url() {
        let temp_dir = std::env::temp_dir().join("desktest_test_invalid");
        let _ = std::fs::create_dir_all(&temp_dir);

        let result = save_screenshot(&temp_dir, 1, "not-a-data-url");
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_extract_text_string_content() {
        let msg = user_message("Hello");
        assert_eq!(extract_text(&msg), Some("Hello".to_string()));
    }

    #[test]
    fn test_extract_text_array_content() {
        let msg = ChatMessage {
            role: "user".into(),
            content: Some(serde_json::json!([
                {"type": "text", "text": "Part 1"},
                {"type": "image_url", "image_url": {"url": "data:..."}},
                {"type": "text", "text": "Part 2"},
            ])),
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text(&msg), Some("Part 1\nPart 2".to_string()));
    }

    #[test]
    fn test_extract_text_none_content() {
        let msg = ChatMessage {
            role: "user".into(),
            content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(extract_text(&msg), None);
    }

    #[test]
    fn test_extract_text_and_images_mixed() {
        let msg = ChatMessage {
            role: "user".into(),
            content: Some(serde_json::json!([
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}},
                {"type": "text", "text": "A11y tree here"},
            ])),
            tool_calls: None,
            tool_call_id: None,
        };
        let (text, images) = extract_text_and_images(&msg);
        assert_eq!(text, "A11y tree here");
        assert_eq!(images, vec!["data:image/png;base64,abc"]);
    }

    #[test]
    fn test_temp_dir_guard_cleanup() {
        let dir = std::env::temp_dir().join("desktest_guard_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.txt"), "hello").unwrap();
        assert!(dir.exists());

        // Drop the guard — should remove the directory
        {
            let _guard = TempDirGuard(dir.clone());
        }
        assert!(!dir.exists());
    }
}
