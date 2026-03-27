//! OpenAI Codex CLI provider — shells out to `codex exec` for LLM calls.
//!
//! Uses the locally-installed Codex CLI, leveraging the user's existing
//! ChatGPT login session or CODEX_API_KEY instead of requiring separate
//! API key configuration in desktest.
//!
//! Screenshots are passed directly as `-i` flags (Codex sees them natively),
//! while accessibility trees are embedded inline in the prompt text.
//! Each step spawns a fresh `codex exec` process — there is no persistent
//! conversation session between steps.

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

/// Provider that delegates to the Codex CLI (`codex exec`).
pub struct CodexCliProvider;

impl CodexCliProvider {
    /// Create a new CLI provider, verifying the `codex` binary is available.
    pub fn new() -> Result<Self, AppError> {
        let status = std::process::Command::new("codex")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        match status {
            Ok(s) if s.success() => {
                tracing::warn!(
                    "codex-cli provider uses --sandbox danger-full-access: \
                     the Codex model can execute arbitrary shell commands without approval"
                );
                Ok(Self)
            }
            _ => Err(AppError::Config(
                "Codex CLI not found. Install it with `npm install -g @openai/codex` \
                 or use a different provider."
                    .into(),
            )),
        }
    }
}

impl LlmProvider for CodexCliProvider {
    fn chat_completion<'a>(
        &'a self,
        messages: &'a [ChatMessage],
        _tools: &'a [serde_json::Value],
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ChatMessage, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let start = std::time::Instant::now();

            // 1. Create temp directory and build the prompt with screenshot files.
            // The PromptResult owns a TempDirGuard that ensures cleanup even if
            // the future is cancelled by an external timeout (e.g., step_timeout).
            let prompt_result = build_codex_prompt(messages)?;
            let screenshot_count = prompt_result.screenshot_paths.len();

            info!(
                "Codex CLI request: {} messages, {} screenshots",
                messages.len(),
                screenshot_count,
            );

            // Output file for the final response.
            let output_file = prompt_result.temp_dir.join("codex_response.txt");

            // 2. Build command — system prompt is embedded in the prompt text
            //    (not via a separate flag) to keep the prompt self-contained.
            let mut cmd = tokio::process::Command::new("codex");
            cmd.kill_on_drop(true);
            cmd.arg("exec");
            cmd.arg("-"); // Read prompt from stdin
            cmd.arg("--skip-git-repo-check");
            cmd.arg("--sandbox").arg("danger-full-access");
            cmd.arg("--color").arg("never");
            cmd.arg("-o").arg(&output_file);

            // Pass each screenshot as an image attachment.
            for path in &prompt_result.screenshot_paths {
                cmd.arg("-i").arg(path);
            }

            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            // 3. Spawn, write prompt, wait for output.
            let output = async {
                let mut child = cmd
                    .spawn()
                    .map_err(|e| AppError::Agent(format!("Failed to spawn codex CLI: {e}")))?;

                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = stdin.write_all(prompt_result.prompt_text.as_bytes()).await {
                        let _ = child.kill().await;
                        return Err(AppError::Agent(format!(
                            "Failed to write to codex stdin: {e}"
                        )));
                    }
                    // Close stdin to signal EOF
                }

                // 5-minute timeout as defense-in-depth (the agent loop's
                // step_timeout also wraps this call, but a hung process should
                // not block indefinitely if that outer timeout is misconfigured).
                // When the timeout fires, the future (and its `child`) are dropped.
                // `kill_on_drop(true)` on the Command ensures the child process
                // receives SIGKILL on drop rather than being left as an orphan.
                tokio::time::timeout(
                    std::time::Duration::from_secs(300),
                    child.wait_with_output(),
                )
                .await
                .map_err(|_| AppError::Agent("Codex CLI timed out after 300s".into()))?
                .map_err(|e| AppError::Agent(format!("Codex CLI process error: {e}")))
            }
            .await;

            let output = output?;
            let elapsed = start.elapsed();

            // 4. Check exit status
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                return Err(AppError::Agent(format!(
                    "Codex CLI failed (exit {}): {}{}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim(),
                    if stdout.is_empty() {
                        String::new()
                    } else {
                        format!("\nstdout: {}", stdout.chars().take(500).collect::<String>())
                    },
                )));
            }

            // 5. Read response from the output file written by `-o`.
            let result = tokio::fs::read_to_string(&output_file)
                .await
                .map_err(|e| AppError::Agent(format!("Failed to read codex output file: {e}")))?
                .trim()
                .to_string();

            if result.is_empty() {
                return Err(AppError::Agent("Codex CLI returned empty response".into()));
            }

            info!("Codex CLI response in {:.1}s", elapsed.as_secs_f64());

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

/// Result of building the Codex CLI prompt.
struct PromptResult {
    /// The full prompt text to pipe to `codex exec -` via stdin.
    prompt_text: String,
    /// Temp directory containing screenshot files.
    temp_dir: PathBuf,
    /// Paths to screenshot files, passed as `-i` flags to codex.
    screenshot_paths: Vec<PathBuf>,
    /// RAII guard — drops (and deletes) the temp directory when PromptResult is dropped.
    _guard: TempDirGuard,
}

/// A single observation step extracted from the message array.
struct ObservationStep {
    /// 1-based step number.
    step: usize,
    /// Path to the screenshot file (if any).
    screenshot: Option<PathBuf>,
    /// Accessibility tree text (if any), embedded inline in the prompt.
    a11y_text: Option<String>,
    /// Whether this is the last (current) observation.
    is_current: bool,
}

/// Build the Codex CLI prompt from the message array.
///
/// Creates a temp directory and saves trajectory screenshots as files
/// (passed to codex via `-i` flags). Accessibility trees are embedded
/// inline in the prompt text rather than saved as separate files.
fn build_codex_prompt(messages: &[ChatMessage]) -> Result<PromptResult, AppError> {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let temp_dir = std::env::temp_dir().join(format!("desktest_codex_{pid}_{counter}"));
    // Remove any leftover directory from a crashed previous run with the same
    // PID + counter (possible if the OS recycles PIDs). Without this, stale
    // screenshot or response files could be silently reused.
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| AppError::Agent(format!("Failed to create temp dir: {e}")))?;
    // Early cleanup guard: if any operation below fails (e.g., save_screenshot
    // on bad base64), the guard's Drop cleans up the temp directory. On success,
    // we move the guard into PromptResult so it lives as long as the caller needs.
    let mut cleanup_guard = Some(TempDirGuard(temp_dir.clone()));

    let mut system_parts = Vec::new();
    let mut prompt_sections: Vec<String> = Vec::new();
    let mut observations: Vec<ObservationStep> = Vec::new();
    let mut obs_counter = 0usize;
    let mut screenshot_paths: Vec<PathBuf> = Vec::new();

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
                    // This is an observation message — save screenshot, embed a11y inline.
                    obs_counter += 1;
                    let is_current = Some(idx) == last_obs_idx;
                    let mut obs = ObservationStep {
                        step: obs_counter,
                        screenshot: None,
                        a11y_text: None,
                        is_current,
                    };

                    // Save screenshot. Observations have one screenshot each;
                    // if multiple images are present, save only the first.
                    if let Some(data_url) = images.first() {
                        let path = save_screenshot(&temp_dir, obs_counter, data_url)?;
                        screenshot_paths.push(path.clone());
                        obs.screenshot = Some(path);
                    }

                    // Embed accessibility tree text inline (not as a file).
                    if !text.is_empty() {
                        obs.a11y_text = Some(text);
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

    // Assemble the final prompt.
    let system_prompt = system_parts.join("\n");
    let prompt_text = assemble_prompt(&system_prompt, &prompt_sections, &observations);

    // Move the cleanup guard into PromptResult — the caller now owns cleanup.
    let guard = cleanup_guard.take().unwrap();
    Ok(PromptResult {
        prompt_text,
        screenshot_paths,
        _guard: guard,
        temp_dir,
    })
}

/// Assemble the final prompt text with inline system prompt and observation sections.
fn assemble_prompt(
    system_prompt: &str,
    inline_sections: &[String],
    observations: &[ObservationStep],
) -> String {
    let mut prompt = String::new();

    // 1. System prompt embedded inline to avoid stacking with Codex CLI's
    //    built-in system prompt.
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

    // 3. Observation sections — a11y trees inline, screenshots via `-i` flags.
    if !observations.is_empty() {
        prompt.push_str("## Observations\n\n");
        prompt.push_str(
            "The following observations show the desktop state at each step. \
             Screenshots are attached as images (passed via -i flags). \
             Accessibility trees are shown inline below.\n\n",
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

            if obs.screenshot.is_some() {
                prompt.push_str(&format!(
                    "- Screenshot: attached as image (step_{:03}_screenshot)\n",
                    obs.step
                ));
            }

            if let Some(ref a11y) = obs.a11y_text {
                prompt.push_str("- Accessibility tree:\n```\n");
                prompt.push_str(a11y);
                prompt.push_str("\n```\n");
            }

            prompt.push('\n');
        }

        prompt.push_str(
            "**Important:** Review ALL observations before responding. \
             The CURRENT step is the one you should act on.\n",
        );
    }

    prompt
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

/// Save a base64-encoded screenshot to the temp directory.
fn save_screenshot(
    temp_dir: &std::path::Path,
    step: usize,
    data_url: &str,
) -> Result<PathBuf, AppError> {
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
        let result = build_codex_prompt(&messages).unwrap();
        assert!(result.prompt_text.contains("<system-instructions>"));
        assert!(result.prompt_text.contains("You are a tester."));
        assert!(result.prompt_text.contains("Click the button."));
        assert!(result.screenshot_paths.is_empty());

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_build_prompt_single_observation() {
        let data_url = "data:image/png;base64,iVBORw0KGgo=";
        let messages = vec![
            system_message("Sys."),
            user_message("## Task\n\nDo something."),
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

        let result = build_codex_prompt(&messages).unwrap();
        assert_eq!(result.screenshot_paths.len(), 1);
        assert!(result.prompt_text.contains("Step 1 (CURRENT"));
        assert!(result.prompt_text.contains("A11y tree content here"));
        // A11y tree is inline, not a file reference
        assert!(result.prompt_text.contains("Accessibility tree:"));
        assert!(result.prompt_text.contains("```"));

        // Verify screenshot file exists
        assert!(result.screenshot_paths[0].exists());
        assert!(
            result.screenshot_paths[0]
                .to_str()
                .unwrap()
                .ends_with("step_001_screenshot.png")
        );

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

        let result = build_codex_prompt(&messages).unwrap();
        assert_eq!(result.screenshot_paths.len(), 2);
        assert!(result.prompt_text.contains("Step 1 (previous)"));
        assert!(result.prompt_text.contains("Step 2 (CURRENT"));
        assert!(result.prompt_text.contains("A11y tree step 1"));
        assert!(result.prompt_text.contains("A11y tree step 2"));
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

        let result = build_codex_prompt(&messages).unwrap();
        assert_eq!(result.screenshot_paths.len(), 1);
        // No a11y tree section
        assert!(!result.prompt_text.contains("Accessibility tree:"));

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
        let result = build_codex_prompt(&messages).unwrap();
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
        let result = build_codex_prompt(&messages).unwrap();
        assert!(result.prompt_text.contains("<system-instructions>"));
        assert!(result.prompt_text.contains("You are a desktop tester."));
        assert!(result.prompt_text.contains("</system-instructions>"));

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }

    #[test]
    fn test_save_screenshot_jpeg_extension() {
        let temp_dir = std::env::temp_dir().join("desktest_codex_test_jpeg");
        let _ = std::fs::create_dir_all(&temp_dir);

        let data_url = "data:image/jpeg;base64,/9j/4AAQ";
        let path = save_screenshot(&temp_dir, 1, data_url).unwrap();
        assert!(path.to_str().unwrap().ends_with(".jpeg"));
        assert_eq!(path.file_name().unwrap(), "step_001_screenshot.jpeg");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_save_screenshot_invalid_data_url() {
        let temp_dir = std::env::temp_dir().join("desktest_codex_test_invalid");
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
        let dir = std::env::temp_dir().join("desktest_codex_guard_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("test.txt"), "hello").unwrap();
        assert!(dir.exists());

        {
            let _guard = TempDirGuard(dir.clone());
        }
        assert!(!dir.exists());
    }

    #[test]
    fn test_a11y_tree_inline_not_saved_as_file() {
        let img = "data:image/png;base64,iVBORw0KGgo=";
        let messages = vec![
            system_message("Sys."),
            ChatMessage {
                role: "user".into(),
                content: Some(serde_json::json!([
                    {"type": "image_url", "image_url": {"url": img}},
                    {"type": "text", "text": "A11y tree content"},
                ])),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let result = build_codex_prompt(&messages).unwrap();

        // Only screenshot file should exist, no a11y .txt file
        let entries: Vec<_> = std::fs::read_dir(&result.temp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].file_name().to_str().unwrap().ends_with(".png"));

        let _ = std::fs::remove_dir_all(&result.temp_dir);
    }
}
