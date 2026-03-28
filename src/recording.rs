use std::path::Path;

use tracing::{debug, info, warn};

use crate::session::{Session, SessionKind};
use crate::error::AppError;

/// Path inside the container where ffmpeg writes the recording.
const CONTAINER_RECORDING_PATH: &str = "/tmp/recording.mp4";

/// Path inside the container for ffmpeg's log output.
const CONTAINER_FFMPEG_LOG: &str = "/tmp/ffmpeg.log";

/// Path inside the container for the live caption text file.
const CONTAINER_CAPTION_PATH: &str = "/tmp/caption.txt";

/// Manages video recording of a test session via ffmpeg inside the container.
pub struct Recording {
    started: bool,
}

impl Recording {
    /// Start recording the virtual display via ffmpeg inside the container.
    ///
    /// Uses x11grab to capture the Xvfb display at `:99`.
    /// The recording runs as a detached process and is stopped with `stop()`.
    pub async fn start(
        session: &SessionKind,
        display_width: u32,
        display_height: u32,
    ) -> Result<Self, AppError> {
        let video_size = format!("{display_width}x{display_height}");

        // Create empty caption file for drawtext overlay
        let _ = session
            .exec(&[
                "bash",
                "-c",
                &format!("printf '' > {CONTAINER_CAPTION_PATH}"),
            ])
            .await;

        // drawtext filter: bottom-left, white text with black outline + dark box, auto-reloads file
        // fontsize=18 fits ~120 chars across 1920px; box gives contrast on any background
        let drawtext_filter = format!(
            "drawtext=textfile={}:reload=1:fontcolor=white:fontsize=18:borderw=1:bordercolor=black:box=1:boxcolor=black@0.5:boxborderw=6:x=10:y=h-th-10",
            CONTAINER_CAPTION_PATH
        );

        // Start ffmpeg as a detached background process
        session
            .exec_detached_with_log(
                &[
                    "ffmpeg",
                    "-f",
                    "x11grab",
                    "-video_size",
                    &video_size,
                    "-framerate",
                    "10",
                    "-i",
                    ":99",
                    "-vf",
                    &drawtext_filter,
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                    "-preset",
                    "ultrafast",
                    "-movflags",
                    "+faststart",
                    CONTAINER_RECORDING_PATH,
                ],
                CONTAINER_FFMPEG_LOG,
            )
            .await?;

        // Give ffmpeg a moment to initialize
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Verify ffmpeg is running
        let check = session
            .exec(&["bash", "-c", "pgrep -x ffmpeg || true"])
            .await
            .unwrap_or_default();
        if check.trim().is_empty() {
            // ffmpeg didn't start — log the error but don't fail the test
            let log = session
                .exec(&["cat", CONTAINER_FFMPEG_LOG])
                .await
                .unwrap_or_default();
            warn!("ffmpeg failed to start. Log: {log}");
            return Ok(Recording { started: false });
        }

        info!("Video recording started ({video_size} @ 10fps)");
        Ok(Recording { started: true })
    }

    /// Stop the ffmpeg recording by sending SIGINT.
    ///
    /// SIGINT causes ffmpeg to finalize the MP4 file properly (moov atom written).
    pub async fn stop(&self, session: &SessionKind) {
        if !self.started {
            debug!("Recording was not started, nothing to stop");
            return;
        }

        // Send SIGINT to ffmpeg for graceful shutdown
        match session
            .exec(&[
                "bash",
                "-c",
                "kill -INT $(pgrep -x ffmpeg) 2>/dev/null || true",
            ])
            .await
        {
            Ok(_) => debug!("Sent SIGINT to ffmpeg"),
            Err(e) => warn!("Failed to send SIGINT to ffmpeg: {e}"),
        }

        // Wait for ffmpeg to finalize the file
        for _ in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let check = session
                .exec(&["bash", "-c", "pgrep -x ffmpeg || true"])
                .await
                .unwrap_or_default();
            if check.trim().is_empty() {
                debug!("ffmpeg process exited");
                return;
            }
        }

        // If still running after 5s, force kill
        warn!("ffmpeg did not exit gracefully, force killing");
        let _ = session
            .exec(&[
                "bash",
                "-c",
                "kill -9 $(pgrep -x ffmpeg) 2>/dev/null || true",
            ])
            .await;
    }

    /// Update the caption overlay text displayed on the recording.
    ///
    /// Shows the agent's thought (up to 3 lines) and the action code on the
    /// recording. Failures are logged but never propagated — captions are
    /// best-effort.
    pub async fn update_caption(
        &self,
        session: &SessionKind,
        step: usize,
        thought: Option<&str>,
        action_code: &[String],
    ) {
        if !self.started {
            return;
        }

        // Escape for ffmpeg drawtext which interprets special sequences in textfile:
        // - % → %% (prevents %{expr} expansion)
        // - \ → \\ (prevents \n, \t, etc. being interpreted as escape sequences)
        let caption = format_caption(step, thought, action_code)
            .replace('\\', "\\\\")
            .replace('%', "%%");

        // Write via stdin to avoid shell escaping issues
        match session
            .exec_with_stdin(
                &["bash", "-c", &format!("cat > {CONTAINER_CAPTION_PATH}.tmp && mv {CONTAINER_CAPTION_PATH}.tmp {CONTAINER_CAPTION_PATH}")],
                caption.as_bytes(),
            )
            .await
        {
            Ok(_) => debug!("Caption updated: {caption}"),
            Err(e) => warn!("Failed to update caption: {e}"),
        }
    }

    /// Copy the recording from the container to the artifacts directory.
    ///
    /// Returns the local path to the recording file, or None if no recording was made.
    pub async fn collect(
        &self,
        session: &SessionKind,
        artifacts_dir: &Path,
    ) -> Option<std::path::PathBuf> {
        if !self.started {
            return None;
        }

        let dest = artifacts_dir.join("recording.mp4");
        match session.copy_from(CONTAINER_RECORDING_PATH, &dest).await {
            Ok(()) => {
                info!("Collected recording to {}", dest.display());
                Some(dest)
            }
            Err(e) => {
                warn!("Failed to collect recording: {e}");
                None
            }
        }
    }
}

/// Maximum characters per caption line (roughly fits 1920px at fontsize 18).
const CAPTION_LINE_WIDTH: usize = 120;

/// Maximum number of thought lines shown in caption.
const CAPTION_MAX_THOUGHT_LINES: usize = 3;

/// Maximum number of action-code lines shown in caption.
const CAPTION_MAX_ACTION_LINES: usize = 5;

/// Format a multi-line caption from the agent's thought and action code.
///
/// Layout (up to ~5 lines):
/// ```text
/// [Step 3] I can see the calculator app. I need to click on the
/// number 5 button first, then the plus button, then the number 3...
/// > pyautogui.click(512, 384)
/// > pyautogui.click(580, 384)
/// ```
fn format_caption(step: usize, thought: Option<&str>, action_code: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Thought lines: wrap to CAPTION_LINE_WIDTH, keep up to CAPTION_MAX_THOUGHT_LINES
    if let Some(thought) = thought {
        let clean = thought.trim().replace('\n', " ");
        if !clean.is_empty() {
            let thought_lines = wrap_text(&format!("[Step {step}] {clean}"), CAPTION_LINE_WIDTH);
            for (i, line) in thought_lines.into_iter().enumerate() {
                if i >= CAPTION_MAX_THOUGHT_LINES {
                    // Replace last line with truncation indicator
                    if let Some(last) = lines.last_mut() {
                        if !last.ends_with("...") {
                            last.push_str("...");
                        }
                    }
                    break;
                }
                lines.push(line);
            }
        }
    }

    if lines.is_empty() {
        lines.push(format!("[Step {step}]"));
    }

    // Action lines: show each code block as "> <code>" with inline comments for intent
    let mut action_line_count = 0;
    'outer: for block in action_code {
        for code_line in block.lines() {
            let trimmed = code_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Skip sleep/wait lines — they're noise in captions
            if trimmed.starts_with("time.sleep") || trimmed.starts_with("import time") {
                continue;
            }
            if action_line_count >= CAPTION_MAX_ACTION_LINES {
                if let Some(last) = lines.last_mut() {
                    if !last.ends_with("...") {
                        last.push_str("...");
                    }
                }
                break 'outer;
            }
            let prefix = if trimmed.starts_with('#') { "  " } else { "> " };
            let prefixed = format!("{prefix}{trimmed}");
            if prefixed.chars().count() > CAPTION_LINE_WIDTH {
                let truncated: String = prefixed.chars().take(CAPTION_LINE_WIDTH).collect();
                lines.push(format!("{truncated}..."));
            } else {
                lines.push(prefixed);
            }
            action_line_count += 1;
        }
    }

    lines.join("\n")
}

/// Wrap text into lines of at most `width` characters, breaking on word boundaries.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for raw_word in s.split_whitespace() {
        // Truncate words longer than width to prevent line overflow
        let word: &str = if raw_word.chars().count() > width {
            let end = raw_word
                .char_indices()
                .nth(width)
                .map(|(i, _)| i)
                .unwrap_or(raw_word.len());
            &raw_word[..end]
        } else {
            raw_word
        };

        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_recording_path() {
        assert_eq!(CONTAINER_RECORDING_PATH, "/tmp/recording.mp4");
    }

    #[test]
    fn test_container_ffmpeg_log_path() {
        assert_eq!(CONTAINER_FFMPEG_LOG, "/tmp/ffmpeg.log");
    }

    #[test]
    fn test_recording_not_started() {
        let recording = Recording { started: false };
        assert!(!recording.started);
    }

    #[test]
    fn test_recording_started() {
        let recording = Recording { started: true };
        assert!(recording.started);
    }

    #[test]
    fn test_container_caption_path() {
        assert_eq!(CONTAINER_CAPTION_PATH, "/tmp/caption.txt");
    }

    #[test]
    fn test_format_caption_thought_and_action() {
        let caption = format_caption(
            3,
            Some("I see the calculator. Clicking the 5 button."),
            &["pyautogui.click(512, 384)".to_string()],
        );
        assert!(caption.contains("[Step 3]"));
        assert!(caption.contains("I see the calculator"));
        assert!(caption.contains("> pyautogui.click(512, 384)"));
    }

    #[test]
    fn test_format_caption_no_thought() {
        let caption = format_caption(1, None, &["pyautogui.click(100, 200)".to_string()]);
        assert!(caption.starts_with("[Step 1]"));
        assert!(caption.contains("> pyautogui.click(100, 200)"));
    }

    #[test]
    fn test_format_caption_no_action() {
        let caption = format_caption(5, Some("Waiting for the app to load."), &[]);
        assert!(caption.contains("[Step 5]"));
        assert!(caption.contains("Waiting for the app"));
        assert!(!caption.contains(">"));
    }

    #[test]
    fn test_format_caption_long_thought_wraps() {
        let long_thought = "word ".repeat(100); // 500 chars
        let caption = format_caption(1, Some(&long_thought), &[]);
        let lines: Vec<&str> = caption.lines().collect();
        // Should be capped at CAPTION_MAX_THOUGHT_LINES
        assert!(lines.len() <= CAPTION_MAX_THOUGHT_LINES);
        // Last thought line should end with "..."
        assert!(lines.last().unwrap().ends_with("..."));
    }

    #[test]
    fn test_format_caption_multiline_code_block() {
        let code =
            "# Focus the window\npyautogui.click(100, 200)\npyautogui.press('enter')".to_string();
        let caption = format_caption(2, Some("Clicking"), &[code]);
        assert!(caption.contains("  # Focus the window"));
        assert!(caption.contains("> pyautogui.click(100, 200)"));
        assert!(caption.contains("> pyautogui.press('enter')"));
    }

    #[test]
    fn test_format_caption_shows_comments_and_skips_blanks() {
        let code = "# click the button\n\npyautogui.click(100, 200)".to_string();
        let caption = format_caption(1, None, &[code]);
        assert!(caption.contains("  # click the button"));
        assert!(caption.contains("> pyautogui.click(100, 200)"));
    }

    #[test]
    fn test_format_caption_skips_sleep_lines() {
        let code = "pyautogui.click(100, 200)\ntime.sleep(0.3)\nimport time".to_string();
        let caption = format_caption(1, None, &[code]);
        assert!(caption.contains("> pyautogui.click(100, 200)"));
        assert!(!caption.contains("time.sleep"));
        assert!(!caption.contains("import time"));
    }

    #[test]
    fn test_wrap_text_short() {
        let lines = wrap_text("hello world", 120);
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn test_wrap_text_wraps_at_boundary() {
        let lines = wrap_text("aaa bbb ccc ddd", 8);
        assert_eq!(lines, vec!["aaa bbb", "ccc ddd"]);
    }
}
