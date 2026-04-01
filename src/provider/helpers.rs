//! Shared helper functions used by CLI-based providers (claude_cli, codex_cli).

use std::path::PathBuf;

use base64::Engine;

use super::ChatMessage;
use crate::error::AppError;

/// Save a base64-encoded screenshot to the temp directory.
pub(crate) fn save_screenshot(
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

/// Save accessibility tree text to the temp directory.
pub(crate) fn save_a11y_tree(
    temp_dir: &std::path::Path,
    step: usize,
    text: &str,
) -> Result<PathBuf, AppError> {
    let path = temp_dir.join(format!("step_{step:03}_a11y.txt"));
    std::fs::write(&path, text.as_bytes())
        .map_err(|e| AppError::Agent(format!("Failed to write a11y tree: {e}")))?;

    Ok(path)
}

/// Extract plain text content from a ChatMessage.
pub(crate) fn extract_text(msg: &ChatMessage) -> Option<String> {
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
pub(crate) fn extract_text_and_images(msg: &ChatMessage) -> (String, Vec<String>) {
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
    use crate::provider::{user_image_message, user_message};

    #[test]
    fn test_save_screenshot_jpeg_extension() {
        let temp_dir = std::env::temp_dir().join("desktest_test_helpers_jpeg");
        let _ = std::fs::create_dir_all(&temp_dir);

        let data_url = "data:image/jpeg;base64,/9j/4AAQ";
        let path = save_screenshot(&temp_dir, 1, data_url).unwrap();
        assert!(path.to_str().unwrap().ends_with(".jpeg"));
        assert_eq!(path.file_name().unwrap(), "step_001_screenshot.jpeg");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_save_a11y_tree() {
        let temp_dir = std::env::temp_dir().join("desktest_test_helpers_a11y");
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
        let temp_dir = std::env::temp_dir().join("desktest_test_helpers_invalid");
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
}
