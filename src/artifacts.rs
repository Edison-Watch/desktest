#![allow(dead_code)]

use std::path::Path;

use tracing::{debug, warn};

use crate::docker::DockerSession;
use crate::error::AppError;

/// Collect artifacts from the container into the host artifacts directory.
///
/// Collects:
/// - The tester user's home directory contents
/// - App logs (best-effort)
pub async fn collect_artifacts(
    session: &DockerSession,
    artifacts_dir: &Path,
) -> Result<(), AppError> {
    std::fs::create_dir_all(artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    // Collect home directory
    let home_dest = artifacts_dir.join("home");
    match session.copy_from("/home/tester", &home_dest).await {
        Ok(()) => debug!("Collected home directory to {}", home_dest.display()),
        Err(e) => warn!("Failed to collect home directory: {e}"),
    }

    // Collect app logs (best-effort)
    let log_dest = artifacts_dir.join("app.log");
    match session.copy_from("/tmp/app.log", &log_dest).await {
        Ok(()) => debug!("Collected app log to {}", log_dest.display()),
        Err(e) => debug!("No app log to collect: {e}"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_artifacts_dir_created() {
        let tmp = tempfile::TempDir::new().unwrap();
        let artifacts = tmp.path().join("test-artifacts");
        assert!(!artifacts.exists());

        std::fs::create_dir_all(&artifacts).unwrap();
        assert!(artifacts.exists());
    }
}
