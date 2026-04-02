#![allow(dead_code)]

use std::path::Path;

use tracing::{debug, warn};

use crate::docker::DockerSession;
use crate::error::AppError;
use crate::session::{Session, SessionKind};

/// Collect artifacts from the container into the host artifacts directory.
///
/// Collects:
/// - The tester user's home directory contents (filtered by exclude patterns)
/// - App stdout/stderr log
/// - Process list at time of collection
/// - Xvfb / x11vnc / xfce4 logs
/// - Docker container logs
pub async fn collect_artifacts(
    session: &SessionKind,
    artifacts_dir: &Path,
    excludes: &[String],
) -> Result<(), AppError> {
    std::fs::create_dir_all(artifacts_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create artifacts dir: {e}")))?;

    // Collect home directory (with exclude filtering)
    // Skip for native sessions — we don't want to copy the host user's entire home
    if !matches!(session, SessionKind::Native(_)) {
        let home_dest = artifacts_dir.join("home");
        match collect_home_filtered(session, &home_dest, excludes).await {
            Ok(()) => debug!("Collected home directory to {}", home_dest.display()),
            Err(e) => warn!("Failed to collect home directory: {e}"),
        }
    }

    // Collect app log (stdout/stderr from the launched app)
    let app_log_path = match session {
        SessionKind::WindowsVm(_) => "C:\\Temp\\app.log",
        _ => "/tmp/app.log",
    };
    let log_dest = artifacts_dir.join("app.log");
    match session.copy_from(app_log_path, &log_dest).await {
        Ok(()) => debug!("Collected app log to {}", log_dest.display()),
        Err(e) => debug!("No app log to collect: {e}"),
    }

    // Capture process list (macOS `ps` doesn't support `f` flag)
    let ps_cmd: &[&str] = match session {
        SessionKind::Tart(_) | SessionKind::Native(_) => &["ps", "aux"],
        SessionKind::Docker(_) => &["ps", "auxf"],
        SessionKind::WindowsVm(_) => &["tasklist"],
    };
    match session.exec(ps_cmd).await {
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

/// Collect the home directory using `tar --exclude` inside the container to skip
/// large/irrelevant directories (node_modules, caches, etc.) at the source,
/// avoiding slow multi-hundred-MB transfers over the Docker socket.
///
/// Falls back to a plain `copy_from` if no excludes are configured or if the
/// filtered tar approach fails.
async fn collect_home_filtered(
    session: &SessionKind,
    home_dest: &Path,
    excludes: &[String],
) -> Result<(), AppError> {
    if excludes.is_empty() {
        // No excludes — use the direct Docker copy path
        return session.copy_from("/home/tester", home_dest).await;
    }

    // Build: tar cf /tmp/_desktest_home.tar --exclude=X --exclude=Y -C /home tester
    let tmp_tar = "/tmp/_desktest_home.tar";
    let mut cmd_parts = vec!["tar".to_string(), "cf".to_string(), tmp_tar.to_string()];
    for pattern in excludes {
        cmd_parts.push(format!("--exclude={pattern}"));
    }
    cmd_parts.push("-C".to_string());
    cmd_parts.push("/home".to_string());
    cmd_parts.push("tester".to_string());

    let cmd_refs: Vec<&str> = cmd_parts.iter().map(|s| s.as_str()).collect();
    let (output, exit_code) = session.exec_with_exit_code(&cmd_refs).await?;
    if exit_code != 0 {
        debug!(
            "Filtered tar failed (exit {}): {}; falling back to unfiltered copy",
            exit_code, output
        );
        return session.copy_from("/home/tester", home_dest).await;
    }

    // Download the filtered tar and extract locally
    let tar_dest = home_dest.with_extension("tar");
    let result = session.copy_from(tmp_tar, &tar_dest).await;

    // Clean up the temp tar inside the container (best-effort)
    let _ = session.exec(&["rm", "-f", tmp_tar]).await;

    result?;

    // Extract the tar locally — it contains a `tester/` root directory
    std::fs::create_dir_all(home_dest)
        .map_err(|e| AppError::Infra(format!("Cannot create dir: {e}")))?;

    let tar_file = std::fs::File::open(&tar_dest)
        .map_err(|e| AppError::Infra(format!("Cannot open tar: {e}")))?;
    let mut archive = tar::Archive::new(tar_file);
    for entry in archive
        .entries()
        .map_err(|e| AppError::Infra(format!("Tar read error: {e}")))?
    {
        let mut entry = entry.map_err(|e| AppError::Infra(format!("Tar entry error: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| AppError::Infra(format!("Tar path error: {e}")))?
            .to_path_buf();

        // Skip symlinks and hard links to prevent directory escape attacks
        if matches!(
            entry.header().entry_type(),
            tar::EntryType::Symlink | tar::EntryType::Link
        ) {
            debug!("Skipping symlink/link entry: {}", path.display());
            continue;
        }

        // Strip the first component ("tester/")
        let components: Vec<_> = path.components().collect();
        if components.len() <= 1 {
            continue; // root dir entry
        }
        let relative: std::path::PathBuf = components[1..].iter().collect();

        // Path traversal check
        for comp in relative.components() {
            match comp {
                std::path::Component::ParentDir | std::path::Component::RootDir => {
                    return Err(AppError::Infra(format!(
                        "Tar entry contains unsafe path: {}",
                        path.display()
                    )));
                }
                _ => {}
            }
        }

        let dest = home_dest.join(&relative);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&dest)
                .map_err(|e| AppError::Infra(format!("Cannot create dir: {e}")))?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| AppError::Infra(format!("Cannot create dir: {e}")))?;
            }
            entry
                .unpack(&dest)
                .map_err(|e| AppError::Infra(format!("Unpack error: {e}")))?;
        }
    }

    // Remove the intermediate tar file
    let _ = std::fs::remove_file(&tar_dest);

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
