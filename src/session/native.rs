use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tracing::debug;

use crate::error::AppError;
use crate::session::Session;

/// Default timeout for command execution in native sessions.
///
/// Commands like `osascript` can hang indefinitely if macOS shows a
/// permissions dialog (TCC), so we always apply a timeout.
const EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// A session that runs commands directly on the host macOS desktop.
///
/// Unlike `DockerSession` or `TartSession`, there is no isolation — commands
/// execute as the current user on the host machine. This is useful for CI on
/// bare-metal Macs or local development, but provides no protection against
/// side effects from the app under test.
pub struct NativeSession {
    /// PIDs of processes launched via `exec_detached`, tracked for cleanup.
    detached_pids: std::sync::Mutex<Vec<u32>>,
}

impl NativeSession {
    /// Create a new native session.
    pub fn create() -> Self {
        Self {
            detached_pids: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Deploy a macOS app for native session.
    ///
    /// For `MacosNative`, there is no deployment step — the app is expected to
    /// already be installed on the host. Returns the app_path or bundle_id for
    /// use by `launch_app`.
    pub async fn deploy_app(&self, app: &crate::task::AppConfig) -> Result<String, AppError> {
        match app {
            crate::task::AppConfig::MacosNative {
                app_path,
                bundle_id,
                ..
            } => {
                // No deployment needed — app is already on host.
                // Return whichever identifier is available for launch.
                if let Some(path) = app_path {
                    Ok(path.clone())
                } else if let Some(bid) = bundle_id {
                    Ok(bid.clone())
                } else {
                    Err(AppError::Config(
                        "MacosNative: at least one of 'bundle_id' or 'app_path' must be specified."
                            .into(),
                    ))
                }
            }
            _ => Err(AppError::Config(
                "NativeSession only supports MacosNative app config.".into(),
            )),
        }
    }

    /// Launch a macOS app natively on the host.
    pub async fn launch_app(
        &self,
        app: &crate::task::AppConfig,
        _deployed_path: &str,
    ) -> Result<(), AppError> {
        match app {
            crate::task::AppConfig::MacosNative {
                bundle_id,
                app_path,
            } => {
                if let Some(bid) = bundle_id {
                    self.exec_detached(&["open", "-b", bid]).await?;
                } else if let Some(path) = app_path {
                    if path.ends_with(".app") {
                        self.exec_detached(&["open", path]).await?;
                    } else {
                        self.exec_detached(&[path.as_str()]).await?;
                    }
                }
                Ok(())
            }
            _ => Err(AppError::Config(
                "NativeSession only supports MacosNative app config.".into(),
            )),
        }
    }
}

/// Spawn a command and wait for output with a timeout.
///
/// On timeout, kills the child process and returns an error mentioning
/// possible TCC permission issues (common on macOS when `osascript` or
/// similar tools trigger a permissions dialog).
async fn exec_with_timeout(
    cmd_name: &str,
    args: &[&str],
    full_cmd: &[&str],
    timeout: Duration,
) -> Result<std::process::Output, AppError> {
    let child = Command::new(cmd_name)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Infra(format!("Failed to spawn '{cmd_name}': {e}")))?;

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(result) => result
            .map_err(|e| AppError::Infra(format!("Failed to wait for '{cmd_name}': {e}"))),
        Err(_) => {
            // wait_with_output consumed child, but on timeout it hasn't completed.
            // The child is dropped here which will close its handles.
            // We need to kill via PID as a fallback.
            Err(AppError::Infra(format!(
                "Command '{}' timed out after {}s \
                 (if running on macOS, check Accessibility/Automation permissions \
                 in System Settings > Privacy & Security)",
                full_cmd.join(" "),
                timeout.as_secs()
            )))
        }
    }
}

impl Session for NativeSession {
    async fn exec(&self, cmd: &[&str]) -> Result<String, AppError> {
        let (cmd_name, args) = cmd
            .split_first()
            .ok_or_else(|| AppError::Infra("Empty command".into()))?;

        debug!("native exec: {}", cmd.join(" "));

        let output = exec_with_timeout(cmd_name, args, cmd, EXEC_TIMEOUT).await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Infra(format!(
                "Command '{}' failed (exit {}): {}",
                cmd.join(" "),
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn exec_with_exit_code(&self, cmd: &[&str]) -> Result<(String, i64), AppError> {
        let (cmd_name, args) = cmd
            .split_first()
            .ok_or_else(|| AppError::Infra("Empty command".into()))?;

        debug!("native exec_with_exit_code: {}", cmd.join(" "));

        let output = exec_with_timeout(cmd_name, args, cmd, EXEC_TIMEOUT).await?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let exit_code = output.status.code().unwrap_or(-1) as i64;

        Ok((stdout, exit_code))
    }

    async fn exec_with_stdin(&self, cmd: &[&str], stdin_data: &[u8]) -> Result<String, AppError> {
        let (cmd_name, args) = cmd
            .split_first()
            .ok_or_else(|| AppError::Infra("Empty command".into()))?;

        debug!("native exec_with_stdin: {}", cmd.join(" "));

        let mut child = Command::new(cmd_name)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::Infra(format!("Failed to spawn '{cmd_name}': {e}")))?;

        // Write stdin data
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(stdin_data)
                .await
                .map_err(|e| AppError::Infra(format!("Failed to write stdin: {e}")))?;
            // Drop stdin to close the pipe
        }

        let output = match tokio::time::timeout(EXEC_TIMEOUT, child.wait_with_output()).await {
            Ok(result) => result
                .map_err(|e| AppError::Infra(format!("Failed to wait for '{cmd_name}': {e}")))?,
            Err(_) => {
                return Err(AppError::Infra(format!(
                    "Command '{}' timed out after {}s",
                    cmd.join(" "),
                    EXEC_TIMEOUT.as_secs()
                )));
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Infra(format!(
                "Command '{}' failed (exit {}): {}",
                cmd.join(" "),
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn exec_detached(&self, cmd: &[&str]) -> Result<(), AppError> {
        let (cmd_name, args) = cmd
            .split_first()
            .ok_or_else(|| AppError::Infra("Empty command".into()))?;

        debug!("native exec_detached: {}", cmd.join(" "));

        let child = Command::new(cmd_name)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::Infra(format!("Failed to spawn detached '{cmd_name}': {e}")))?;

        if let Some(pid) = child.id() {
            if let Ok(mut pids) = self.detached_pids.lock() {
                pids.push(pid);
            }
        }

        Ok(())
    }

    async fn exec_detached_with_log(&self, cmd: &[&str], log_path: &str) -> Result<(), AppError> {
        let (cmd_name, args) = cmd
            .split_first()
            .ok_or_else(|| AppError::Infra("Empty command".into()))?;

        debug!("native exec_detached_with_log: {} > {log_path}", cmd.join(" "));

        let log_file = std::fs::File::create(log_path)
            .map_err(|e| AppError::Infra(format!("Failed to create log file '{log_path}': {e}")))?;
        let log_stderr = log_file
            .try_clone()
            .map_err(|e| AppError::Infra(format!("Failed to clone log file handle: {e}")))?;

        let child = Command::new(cmd_name)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_stderr))
            .spawn()
            .map_err(|e| AppError::Infra(format!("Failed to spawn detached '{cmd_name}': {e}")))?;

        if let Some(pid) = child.id() {
            if let Ok(mut pids) = self.detached_pids.lock() {
                pids.push(pid);
            }
        }

        Ok(())
    }

    async fn copy_into(&self, src: &Path, dest_path: &str) -> Result<(), AppError> {
        debug!("native copy_into: {} -> {dest_path}", src.display());

        let dest = Path::new(dest_path);

        if src.is_dir() {
            copy_dir_recursive(src, dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::Infra(format!(
                        "Failed to create parent directory '{}': {e}",
                        parent.display()
                    ))
                })?;
            }
            std::fs::copy(src, dest).map_err(|e| {
                AppError::Infra(format!(
                    "Failed to copy '{}' to '{dest_path}': {e}",
                    src.display()
                ))
            })?;
        }

        Ok(())
    }

    async fn copy_from(&self, container_path: &str, local_path: &Path) -> Result<(), AppError> {
        debug!(
            "native copy_from: {container_path} -> {}",
            local_path.display()
        );

        let src = Path::new(container_path);

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Infra(format!(
                    "Failed to create parent directory '{}': {e}",
                    parent.display()
                ))
            })?;
        }

        if src.is_dir() {
            copy_dir_recursive(src, local_path)?;
        } else {
            std::fs::copy(src, local_path).map_err(|e| {
                AppError::Infra(format!(
                    "Failed to copy '{container_path}' to '{}': {e}",
                    local_path.display()
                ))
            })?;
        }

        Ok(())
    }

    async fn cleanup(&self) -> Result<(), AppError> {
        debug!("native cleanup: terminating detached processes");

        // Best-effort: kill any processes we spawned via exec_detached
        if let Ok(pids) = self.detached_pids.lock() {
            for &pid in pids.iter() {
                // Send SIGTERM; ignore errors (process may have already exited)
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }

        Ok(())
    }
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dest).map_err(|e| {
        AppError::Infra(format!(
            "Failed to create directory '{}': {e}",
            dest.display()
        ))
    })?;

    for entry in std::fs::read_dir(src).map_err(|e| {
        AppError::Infra(format!(
            "Failed to read directory '{}': {e}",
            src.display()
        ))
    })? {
        let entry = entry.map_err(|e| AppError::Infra(format!("Directory entry error: {e}")))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).map_err(|e| {
                AppError::Infra(format!(
                    "Failed to copy '{}' to '{}': {e}",
                    src_path.display(),
                    dest_path.display()
                ))
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_native_exec_echo() {
        let session = NativeSession::create();
        let result = session.exec(&["echo", "hello"]).await.unwrap();
        assert_eq!(result.trim(), "hello");
    }

    #[tokio::test]
    async fn test_native_exec_with_exit_code_success() {
        let session = NativeSession::create();
        let (stdout, code) = session
            .exec_with_exit_code(&["echo", "test"])
            .await
            .unwrap();
        assert_eq!(stdout.trim(), "test");
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn test_native_exec_with_exit_code_failure() {
        let session = NativeSession::create();
        let (_, code) = session
            .exec_with_exit_code(&["false"])
            .await
            .unwrap();
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn test_native_exec_with_stdin() {
        let session = NativeSession::create();
        let result = session
            .exec_with_stdin(&["cat"], b"hello from stdin")
            .await
            .unwrap();
        assert_eq!(result, "hello from stdin");
    }

    #[tokio::test]
    async fn test_native_exec_empty_command() {
        let session = NativeSession::create();
        let result = session.exec(&[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_native_copy_into_and_from() {
        let session = NativeSession::create();
        let tmp = std::env::temp_dir().join("desktest-native-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Create a source file
        let src_file = tmp.join("source.txt");
        std::fs::write(&src_file, "test content").unwrap();

        // Copy into a destination
        let dest_path = tmp.join("dest.txt");
        session
            .copy_into(&src_file, dest_path.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(&dest_path).unwrap(), "test content");

        // Copy back from
        let round_trip = tmp.join("round_trip.txt");
        session
            .copy_from(dest_path.to_str().unwrap(), &round_trip)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&round_trip).unwrap(),
            "test content"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_native_cleanup_noop() {
        let session = NativeSession::create();
        // Cleanup should succeed even with no detached processes
        session.cleanup().await.unwrap();
    }
}
