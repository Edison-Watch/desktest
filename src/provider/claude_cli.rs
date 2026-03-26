//! Claude Code CLI provider — shells out to `claude -p` for LLM calls.
//!
//! Uses the locally-installed Claude Code CLI, leveraging the user's existing
//! CLI authentication instead of requiring a separate API key.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use tracing::info;

use super::{ChatMessage, LlmProvider};
use crate::error::AppError;

/// Atomic counter for unique temp file names.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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

            // 1. Parse messages into system prompt, user prompt, and temp image files
            let (system_prompt, prompt_text, temp_files) = build_cli_prompt(messages)?;
            let has_images = !temp_files.is_empty();

            info!(
                "Claude CLI request: {} messages, {} images",
                messages.len(),
                temp_files.len(),
            );

            // 2. Build command
            let mut cmd = tokio::process::Command::new("claude");
            cmd.arg("-p");
            cmd.arg("--output-format").arg("text");

            if has_images {
                // Need an extra turn for Claude to read the screenshot file
                cmd.arg("--max-turns").arg("2");
                cmd.arg("--allowedTools").arg("Read");
            } else {
                cmd.arg("--max-turns").arg("1");
            }

            if !system_prompt.is_empty() {
                cmd.arg("--append-system-prompt").arg(&system_prompt);
            }

            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            // 3. Spawn, write prompt, wait for output, then always clean up temp files.
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

            // Always clean up temp files, even on error paths
            cleanup_temp_files(&temp_files);

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

/// Build the CLI prompt from the message array.
///
/// Extracts the system prompt separately (for `--append-system-prompt`),
/// flattens user/assistant messages into a single prompt string, and saves
/// any embedded base64 images to temp files.
///
/// Returns `(system_prompt, prompt_text, temp_file_paths)`.
fn build_cli_prompt(messages: &[ChatMessage]) -> Result<(String, String, Vec<PathBuf>), AppError> {
    let mut system_parts = Vec::new();
    let mut sections = Vec::new();
    let mut temp_files = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "system" => {
                if let Some(text) = extract_text(msg) {
                    system_parts.push(text);
                }
            }
            "user" => {
                let (text, images) = extract_text_and_images(msg);
                let mut section = String::new();

                for data_url in &images {
                    let path = match save_image_to_temp(data_url) {
                        Ok(p) => p,
                        Err(e) => {
                            // Clean up any files already created before propagating
                            cleanup_temp_files(&temp_files);
                            return Err(e);
                        }
                    };
                    section.push_str(&format!(
                        "Screenshot saved at: {}\n\
                         Please read this image file to see the current desktop state.\n\n",
                        path.display()
                    ));
                    temp_files.push(path);
                }

                if !text.is_empty() {
                    section.push_str(&text);
                }

                if !section.is_empty() {
                    sections.push(section);
                }
            }
            "assistant" => {
                if let Some(text) = extract_text(msg) {
                    sections.push(format!("[Previous agent response]\n{text}"));
                }
            }
            _ => {} // skip tool messages
        }
    }

    let system_prompt = system_parts.join("\n").trim().to_string();
    let prompt_text = sections.join("\n\n---\n\n");

    Ok((system_prompt, prompt_text, temp_files))
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

/// Decode a base64 data URL and save to a temp image file.
///
/// The file extension is derived from the MIME type in the data URL
/// (e.g., `data:image/jpeg;base64,...` → `.jpeg`), falling back to `.png`.
fn save_image_to_temp(data_url: &str) -> Result<PathBuf, AppError> {
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

    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let path = std::env::temp_dir().join(format!("desktest_cli_{pid}_{counter}.{ext}"));

    std::fs::write(&path, &bytes)
        .map_err(|e| AppError::Agent(format!("Failed to write temp screenshot: {e}")))?;

    Ok(path)
}

/// Remove temp files, ignoring errors (best-effort cleanup).
fn cleanup_temp_files(paths: &[PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{system_message, user_image_message, user_message};

    #[test]
    fn test_build_cli_prompt_text_only() {
        let messages = vec![
            system_message("You are a tester."),
            user_message("Click the button."),
        ];
        let (sys, prompt, temps) = build_cli_prompt(&messages).unwrap();
        assert_eq!(sys, "You are a tester.");
        assert!(prompt.contains("Click the button."));
        assert!(temps.is_empty());
    }

    #[test]
    fn test_build_cli_prompt_with_image() {
        // Minimal valid base64
        let data_url = "data:image/png;base64,iVBORw0KGgo=";
        let messages = vec![system_message("Sys."), user_image_message(data_url)];
        let (sys, prompt, temps) = build_cli_prompt(&messages).unwrap();
        assert_eq!(sys, "Sys.");
        assert!(prompt.contains("Screenshot saved at:"));
        assert_eq!(temps.len(), 1);
        assert!(temps[0].exists());

        // Cleanup
        for f in &temps {
            let _ = std::fs::remove_file(f);
        }
    }

    #[test]
    fn test_build_cli_prompt_preserves_assistant_turns() {
        let messages = vec![
            system_message("Sys."),
            user_message("Task A"),
            ChatMessage {
                role: "assistant".into(),
                content: Some(serde_json::Value::String("I clicked the button.".into())),
                tool_calls: None,
                tool_call_id: None,
            },
            user_message("Current observation."),
        ];
        let (_sys, prompt, _temps) = build_cli_prompt(&messages).unwrap();
        assert!(prompt.contains("[Previous agent response]"));
        assert!(prompt.contains("I clicked the button."));
        assert!(prompt.contains("Current observation."));
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
    fn test_save_image_to_temp() {
        let data_url = "data:image/png;base64,iVBORw0KGgo=";
        let path = save_image_to_temp(data_url).unwrap();
        assert!(path.exists());
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("desktest_cli_")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_save_image_invalid_data_url() {
        let result = save_image_to_temp("not-a-data-url");
        assert!(result.is_err());
    }
}
