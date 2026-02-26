use std::path::Path;

use tracing::{debug, info, warn};

use crate::docker::DockerSession;
use crate::error::AppError;

/// Path inside the container where ffmpeg writes the recording.
const CONTAINER_RECORDING_PATH: &str = "/tmp/recording.mp4";

/// Path inside the container for ffmpeg's log output.
const CONTAINER_FFMPEG_LOG: &str = "/tmp/ffmpeg.log";

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
        session: &DockerSession,
        display_width: u32,
        display_height: u32,
    ) -> Result<Self, AppError> {
        let video_size = format!("{display_width}x{display_height}");

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
                    "-c:v",
                    "libx264",
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
    pub async fn stop(&self, session: &DockerSession) {
        if !self.started {
            debug!("Recording was not started, nothing to stop");
            return;
        }

        // Send SIGINT to ffmpeg for graceful shutdown
        match session.exec(&["bash", "-c", "kill -INT $(pgrep -x ffmpeg) 2>/dev/null || true"]).await {
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
            .exec(&["bash", "-c", "kill -9 $(pgrep -x ffmpeg) 2>/dev/null || true"])
            .await;
    }

    /// Copy the recording from the container to the artifacts directory.
    ///
    /// Returns the local path to the recording file, or None if no recording was made.
    pub async fn collect(
        &self,
        session: &DockerSession,
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
}
