#![allow(dead_code)]

use std::path::{Path, PathBuf};

use base64::Engine;

use crate::docker::DockerSession;
use crate::error::AppError;

/// Capture a screenshot from the container's virtual display, save it to the
/// artifacts directory, and return both the local path and a base64 data URL.
pub async fn capture_screenshot(
    session: &DockerSession,
    artifacts_dir: &Path,
    index: usize,
) -> Result<(PathBuf, String), AppError> {
    // Capture screenshot inside container
    session
        .exec(&["scrot", "-o", "-p", "/tmp/screenshot.png"])
        .await?;

    // Copy from container to host
    let local_path = artifacts_dir.join(format!("screenshot_{:04}.png", index));
    session
        .copy_from("/tmp/screenshot.png", &local_path)
        .await?;

    // Read and encode as base64 data URL
    let bytes = std::fs::read(&local_path)
        .map_err(|e| AppError::Infra(format!("Cannot read screenshot: {e}")))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let data_url = format!("data:image/png;base64,{b64}");

    Ok((local_path, data_url))
}

/// Encode raw bytes as a PNG base64 data URL string.
pub fn bytes_to_data_url(bytes: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:image/png;base64,{b64}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_round_trip() {
        let original = b"fake png data here";
        let b64 = base64::engine::general_purpose::STANDARD.encode(original);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        assert_eq!(original.as_slice(), decoded.as_slice());
    }

    #[test]
    fn test_data_url_format() {
        let data_url = bytes_to_data_url(b"test");
        assert!(data_url.starts_with("data:image/png;base64,"));
        let b64_part = data_url.strip_prefix("data:image/png;base64,").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64_part)
            .unwrap();
        assert_eq!(decoded, b"test");
    }
}
