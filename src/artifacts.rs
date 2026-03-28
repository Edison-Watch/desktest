#![allow(dead_code)]

use std::path::Path;

use tracing::{debug, warn};

use crate::docker::DockerSession;
use crate::error::AppError;
use crate::session::{Session, SessionKind};

/// Collect artifacts from the container into the host artifacts directory.
///
/// Collects:
/// - The tester user's home directory contents
/// - App stdout/stderr log
/// - Process list at time of collection
/// - Xvfb / x11vnc / xfce4 logs
/// - Docker container logs
pub async fn collect_artifacts(
    session: &SessionKind,
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

    // Collect app log (stdout/stderr from the launched app)
    let log_dest = artifacts_dir.join("app.log");
    match session.copy_from("/tmp/app.log", &log_dest).await {
        Ok(()) => debug!("Collected app log to {}", log_dest.display()),
        Err(e) => debug!("No app log to collect: {e}"),
    }

    // Capture process list
    match session.exec(&["ps", "auxf"]).await {
        Ok(output) => {
            let ps_path = artifacts_dir.join("processes.txt");
            if let Err(e) = std::fs::write(&ps_path, &output) {
                warn!("Failed to write process list: {e}");
            } else {
                debug!("Collected process list to {}", ps_path.display());
            }
        }
        Err(e) => debug!("Failed to capture process list: {e}"),
    }

    // Capture system logs from /tmp (Xvfb, x11vnc, etc.)
    match session
        .exec(&["bash", "-c", "ls /tmp/*.log 2>/dev/null || true"])
        .await
    {
        Ok(output) => {
            for log_file in output.lines().filter(|l| !l.trim().is_empty()) {
                let log_file = log_file.trim();
                let filename = std::path::Path::new(log_file)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown.log".to_string());
                // Skip app.log since we already collected it
                if filename == "app.log" {
                    continue;
                }
                let dest = artifacts_dir.join(&filename);
                match session.copy_from(log_file, &dest).await {
                    Ok(()) => debug!("Collected {filename}"),
                    Err(e) => debug!("Failed to collect {filename}: {e}"),
                }
            }
        }
        Err(e) => debug!("Failed to list log files: {e}"),
    }

    // Capture dmesg output (useful for FUSE/AppImage issues)
    match session.exec(&["dmesg"]).await {
        Ok(output) if !output.trim().is_empty() => {
            let dmesg_path = artifacts_dir.join("dmesg.txt");
            if let Err(e) = std::fs::write(&dmesg_path, &output) {
                warn!("Failed to write dmesg: {e}");
            } else {
                debug!("Collected dmesg output");
            }
        }
        _ => debug!("No dmesg output to collect"),
    }

    // Capture container logs via Docker API (Docker-specific)
    if let Some(docker) = session.as_docker() {
        collect_docker_logs(docker, artifacts_dir).await;
    }

    Ok(())
}

/// Collect Docker container logs (stdout/stderr from entrypoint.sh).
async fn collect_docker_logs(session: &DockerSession, artifacts_dir: &Path) {
    use bollard::container::LogsOptions;

    let options = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        timestamps: true,
        ..Default::default()
    };

    let mut output = String::new();
    let stream = session
        .docker_client()
        .logs(&session.container_id, Some(options));
    futures::pin_mut!(stream);

    while let Some(chunk) = futures::StreamExt::next(&mut stream).await {
        match chunk {
            Ok(log) => output.push_str(&log.to_string()),
            Err(e) => {
                debug!("Error reading container logs: {e}");
                break;
            }
        }
    }

    if !output.is_empty() {
        let log_path = artifacts_dir.join("container.log");
        if let Err(e) = std::fs::write(&log_path, &output) {
            warn!("Failed to write container logs: {e}");
        } else {
            debug!("Collected container logs to {}", log_path.display());
        }
    }
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
