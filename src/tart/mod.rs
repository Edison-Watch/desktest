#![allow(dead_code)]

pub mod deploy;
pub mod protocol;
pub mod readiness;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::session::Session;

use self::protocol::{ProtocolClient, Request, RequestType, relative_transfer_path};

const TART_SHARED_DIR_NAME: &str = "desktest";
const DEFAULT_AGENT_READY_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub struct TartSession {
    vm_name: String,
    shared_dir: PathBuf,
    guest_shared_dir: String,
    protocol: ProtocolClient,
    run_child: Arc<Mutex<Option<Child>>>,
}

/// Clean up stale shared directories from previously crashed Tart sessions.
///
/// Scans the system temp directory for `desktest-tart-*-shared` directories
/// that are no longer associated with a running VM. This handles the case
/// where desktest was killed before `TartSession::cleanup()` could run.
pub fn cleanup_stale_shared_dirs() {
    let tmp = std::env::temp_dir();
    let entries = match std::fs::read_dir(&tmp) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("desktest-tart-") && name.ends_with("-shared") {
            // Extract the VM name from the dir name (strip "-shared" suffix)
            let vm_name = &name[..name.len() - 7];

            // Check if this VM is still running via `tart list`
            let still_running = std::process::Command::new("tart")
                .args(["get", vm_name])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if !still_running {
                tracing::debug!("Cleaning up stale shared dir: {name}");
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

impl TartSession {
    pub async fn create(base_image: &str) -> Result<Self, AppError> {
        // Opportunistically clean up shared dirs from crashed sessions.
        // Run on a blocking thread to avoid stalling the Tokio executor
        // (cleanup does synchronous I/O and spawns `tart get` subprocesses).
        let _ = tokio::task::spawn_blocking(cleanup_stale_shared_dirs).await;

        let vm_name = format!("desktest-tart-{}", protocol::next_request_id());
        let shared_dir = std::env::temp_dir().join(format!("{vm_name}-shared"));
        let guest_shared_dir = format!("/Volumes/My Shared Files/{TART_SHARED_DIR_NAME}");

        let session = Self::new(vm_name, shared_dir, guest_shared_dir);
        session.protocol.ensure_layout().await?;

        run_tart_command(["clone", base_image, &session.vm_name]).await?;

        // From this point, the cloned VM exists on disk.  If anything below
        // fails we must delete it, otherwise the clone leaks indefinitely.
        let result: Result<Child, AppError> = async {
            let mut child = Command::new("tart")
                .arg("run")
                .arg(format!(
                    "--dir={TART_SHARED_DIR_NAME}:{}",
                    session.shared_dir.display()
                ))
                .arg(&session.vm_name)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| AppError::Infra(format!("Failed to spawn `tart run`: {e}")))?;

            wait_for_agent_or_exit(&session.protocol, &mut child, DEFAULT_AGENT_READY_TIMEOUT)
                .await?;

            Ok(child)
        }
        .await;

        match result {
            Ok(child) => {
                *session.run_child.lock().await = Some(child);
                Ok(session)
            }
            Err(e) => {
                // Clean up the leaked clone before propagating the error
                let _ = run_tart_command(["delete", &session.vm_name]).await;
                let _ = tokio::fs::remove_dir_all(&session.shared_dir).await;
                Err(e)
            }
        }
    }

    fn new(vm_name: String, shared_dir: PathBuf, guest_shared_dir: String) -> Self {
        Self {
            vm_name,
            shared_dir: shared_dir.clone(),
            guest_shared_dir,
            protocol: ProtocolClient::with_timeouts(
                shared_dir,
                DEFAULT_REQUEST_TIMEOUT,
                DEFAULT_POLL_INTERVAL,
            ),
            run_child: Arc::new(Mutex::new(None)),
        }
    }

    pub fn vm_name(&self) -> &str {
        &self.vm_name
    }

    pub fn shared_dir(&self) -> &Path {
        &self.shared_dir
    }

    pub fn guest_shared_dir(&self) -> &str {
        &self.guest_shared_dir
    }

    async fn send(&self, request: Request) -> Result<protocol::Response, AppError> {
        self.protocol.send_request(&request).await
    }

    async fn prepare_transfer_in(&self, src: &Path) -> Result<(PathBuf, String), AppError> {
        let request_id = protocol::next_request_id();
        let stage_dir = self.protocol.transfer_stage(&request_id);
        tokio::fs::create_dir_all(&stage_dir).await?;

        let name = src.file_name().ok_or_else(|| {
            AppError::Infra(format!("Source path has no basename: {}", src.display()))
        })?;
        let staged_path = stage_dir.join(name);
        copy_path(src, &staged_path)?;
        let relative = relative_transfer_path(&self.shared_dir, &staged_path)?;
        Ok((stage_dir, relative))
    }

    async fn prepare_transfer_out(&self) -> Result<(PathBuf, String), AppError> {
        let request_id = protocol::next_request_id();
        let stage_dir = self.protocol.transfer_stage(&request_id);
        tokio::fs::create_dir_all(&stage_dir).await?;
        let relative = relative_transfer_path(&self.shared_dir, &stage_dir)?;
        Ok((stage_dir, relative))
    }
}

impl Session for TartSession {
    async fn exec(&self, cmd: &[&str]) -> Result<String, AppError> {
        let response = self
            .send(Request {
                kind: RequestType::Exec,
                cmd: Some(cmd.iter().map(|s| (*s).to_string()).collect()),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await?;
        Ok(response.stdout)
    }

    async fn exec_with_exit_code(&self, cmd: &[&str]) -> Result<(String, i64), AppError> {
        let response = self
            .send(Request {
                kind: RequestType::ExecExitCode,
                cmd: Some(cmd.iter().map(|s| (*s).to_string()).collect()),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await?;
        Ok((response.stdout, response.exit_code))
    }

    async fn exec_with_stdin(&self, cmd: &[&str], stdin_data: &[u8]) -> Result<String, AppError> {
        let response = self
            .send(Request {
                kind: RequestType::ExecStdin,
                cmd: Some(cmd.iter().map(|s| (*s).to_string()).collect()),
                stdin_b64: Some(base64::engine::general_purpose::STANDARD.encode(stdin_data)),
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await?;
        Ok(response.stdout)
    }

    async fn exec_detached(&self, cmd: &[&str]) -> Result<(), AppError> {
        self.send(Request {
            kind: RequestType::ExecDetached,
            cmd: Some(cmd.iter().map(|s| (*s).to_string()).collect()),
            stdin_b64: None,
            src_path: None,
            dest_path: None,
            transfer_path: None,
        })
        .await?;
        Ok(())
    }

    async fn exec_detached_with_log(&self, cmd: &[&str], log_path: &str) -> Result<(), AppError> {
        let escaped_cmd = cmd
            .iter()
            .map(|s| shell_escape::escape((*s).into()))
            .collect::<Vec<_>>()
            .join(" ");
        self.exec_detached(&[
            "bash",
            "-lc",
            &format!(
                "nohup {escaped_cmd} > {} 2>&1 &",
                shell_escape::escape(log_path.into())
            ),
        ])
        .await
    }

    async fn copy_into(&self, src: &Path, dest_path: &str) -> Result<(), AppError> {
        let (stage_dir, transfer_path) = self.prepare_transfer_in(src).await?;
        let result = self
            .send(Request {
                kind: RequestType::CopyToVm,
                cmd: None,
                stdin_b64: None,
                src_path: None,
                dest_path: Some(dest_path.to_string()),
                transfer_path: Some(transfer_path),
            })
            .await;
        let _ = tokio::fs::remove_dir_all(stage_dir).await;
        result.map(|_| ())
    }

    async fn copy_from(&self, container_path: &str, local_path: &Path) -> Result<(), AppError> {
        let (stage_dir, transfer_path) = self.prepare_transfer_out().await?;
        let result = self
            .send(Request {
                kind: RequestType::CopyFromVm,
                cmd: None,
                stdin_b64: None,
                src_path: Some(container_path.to_string()),
                dest_path: None,
                transfer_path: Some(transfer_path),
            })
            .await;

        if let Err(err) = result {
            let _ = tokio::fs::remove_dir_all(stage_dir).await;
            return Err(err);
        }

        let mut entries = std::fs::read_dir(&stage_dir).map_err(|e| {
            AppError::Infra(format!(
                "Cannot read transfer stage {}: {e}",
                stage_dir.display()
            ))
        })?;
        let first_entry = entries
            .next()
            .transpose()
            .map_err(|e| {
                AppError::Infra(format!(
                    "Cannot inspect transfer stage {}: {e}",
                    stage_dir.display()
                ))
            })?
            .ok_or_else(|| {
                AppError::Infra(format!(
                    "Tart transfer stage {} is empty",
                    stage_dir.display()
                ))
            })?;

        let staged_root = first_entry.path();
        if staged_root.is_dir() {
            copy_dir_contents(&staged_root, local_path)?;
        } else {
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&staged_root, local_path).map_err(|e| {
                AppError::Infra(format!(
                    "Cannot copy {} to {}: {e}",
                    staged_root.display(),
                    local_path.display()
                ))
            })?;
        }

        let _ = tokio::fs::remove_dir_all(stage_dir).await;
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), AppError> {
        let mut child_guard = self.run_child.lock().await;
        if let Some(child) = child_guard.as_mut() {
            if child.try_wait()?.is_none() {
                let _ = run_tart_command(["stop", &self.vm_name]).await;
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        *child_guard = None;

        let _ = run_tart_command(["delete", &self.vm_name]).await;
        let _ = tokio::fs::remove_dir_all(&self.shared_dir).await;
        Ok(())
    }
}

pub async fn run_tart_command<I, S>(args: I) -> Result<(), AppError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args_vec: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    let output = Command::new("tart")
        .args(&args_vec)
        .output()
        .await
        .map_err(|e| {
            AppError::Infra(format!("Failed to run `tart {}`: {e}", args_vec.join(" ")))
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(AppError::Infra(format!(
        "`tart {}` failed with status {}{}",
        args_vec.join(" "),
        output.status,
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    )))
}

async fn wait_for_agent_or_exit(
    protocol: &ProtocolClient,
    child: &mut Child,
    timeout: Duration,
) -> Result<(), AppError> {
    let deadline = tokio::time::Instant::now() + timeout;
    let sentinel = protocol.paths().agent_ready_path.clone();
    loop {
        if tokio::fs::try_exists(&sentinel).await.unwrap_or(false) {
            return Ok(());
        }

        if let Some(status) = child.try_wait()? {
            return Err(AppError::Infra(format!(
                "`tart run` exited before the VM agent became ready: {status}"
            )));
        }

        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill().await;
            return Err(AppError::Infra(
                "Timed out waiting for Tart VM agent to become ready".into(),
            ));
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn copy_path(src: &Path, dest: &Path) -> Result<(), AppError> {
    if src.is_file() {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dest).map_err(|e| {
            AppError::Infra(format!(
                "Cannot copy {} to {}: {e}",
                src.display(),
                dest.display()
            ))
        })?;
        return Ok(());
    }

    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_path(&child_src, &child_dest)?;
        }
        return Ok(());
    }

    Err(AppError::Infra(format!(
        "Source path does not exist: {}",
        src.display()
    )))
}

fn copy_dir_contents(src_dir: &Path, dest_dir: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dest_dir)?;
    for entry in std::fs::read_dir(src_dir)? {
        let entry = entry?;
        let child_src = entry.path();
        let child_dest = dest_dir.join(entry.file_name());
        copy_path(&child_src, &child_dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_path_copies_nested_directories() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src");
        let nested = src.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("hello.txt"), "hello").unwrap();

        let dest = temp.path().join("dest");
        copy_path(&src, &dest).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("nested").join("hello.txt")).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    #[ignore = "requires Tart and a golden image with the desktest VM agent installed"]
    async fn tart_session_exec_round_trip() {
        let Some(base_image) = std::env::var("DESKTEST_TART_BASE_IMAGE").ok() else {
            return;
        };

        let session = TartSession::create(&base_image).await.unwrap();
        let output = session.exec(&["echo", "hello"]).await.unwrap();
        assert_eq!(output.trim(), "hello");
        session.cleanup().await.unwrap();
    }

    // --- Rust ↔ Python contract tests ---
    //
    // These spawn the real vm-agent.py against a temp shared directory and
    // drive it through ProtocolClient, validating that both sides agree on
    // the request/response format.

    /// Locate the vm-agent.py script relative to the project root.
    fn vm_agent_path() -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest.join("macos").join("vm-agent.py")
    }

    /// Helper: spawn vm-agent.py against `shared_dir`, wait for agent_ready.
    async fn spawn_agent(shared_dir: &Path) -> tokio::process::Child {
        let child = Command::new("python3")
            .arg(vm_agent_path())
            .arg(shared_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn vm-agent.py — is python3 installed?");

        // Wait for agent_ready sentinel
        let sentinel = shared_dir.join("agent_ready");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !sentinel.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "vm-agent.py did not write agent_ready within 5s"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        child
    }

    /// Helper: stop the agent process.
    async fn stop_agent(mut child: tokio::process::Child) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn contract_exec() {
        let temp = tempfile::tempdir().unwrap();
        let child = spawn_agent(temp.path()).await;
        let client = protocol::ProtocolClient::with_timeouts(
            temp.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );

        let response = client
            .send_request(&protocol::Request {
                kind: protocol::RequestType::Exec,
                cmd: Some(vec!["echo".into(), "contract-test".into()]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap();

        assert_eq!(response.exit_code, 0);
        assert!(
            response.stdout.contains("contract-test"),
            "stdout={:?}",
            response.stdout
        );
        assert!(response.error.is_none());
        stop_agent(child).await;
    }

    #[tokio::test]
    async fn contract_exec_exit_code() {
        let temp = tempfile::tempdir().unwrap();
        let child = spawn_agent(temp.path()).await;
        let client = protocol::ProtocolClient::with_timeouts(
            temp.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );

        let response = client
            .send_request(&protocol::Request {
                kind: protocol::RequestType::ExecExitCode,
                cmd: Some(vec!["sh".into(), "-c".into(), "exit 7".into()]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap();

        assert_eq!(response.exit_code, 7);
        stop_agent(child).await;
    }

    #[tokio::test]
    async fn contract_exec_stdin() {
        let temp = tempfile::tempdir().unwrap();
        let child = spawn_agent(temp.path()).await;
        let client = protocol::ProtocolClient::with_timeouts(
            temp.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );

        let input = b"hello from rust";
        let encoded = base64::engine::general_purpose::STANDARD.encode(input);

        let response = client
            .send_request(&protocol::Request {
                kind: protocol::RequestType::ExecStdin,
                cmd: Some(vec!["cat".into()]),
                stdin_b64: Some(encoded),
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap();

        assert_eq!(response.exit_code, 0);
        assert!(
            response.stdout.contains("hello from rust"),
            "stdout={:?}",
            response.stdout
        );
        stop_agent(child).await;
    }

    #[tokio::test]
    async fn contract_exec_detached() {
        let temp = tempfile::tempdir().unwrap();
        let child = spawn_agent(temp.path()).await;
        let client = protocol::ProtocolClient::with_timeouts(
            temp.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );

        let marker = temp.path().join("detached_done");
        let response = client
            .send_request(&protocol::Request {
                kind: protocol::RequestType::ExecDetached,
                cmd: Some(vec![
                    "sh".into(),
                    "-c".into(),
                    format!("echo ok > {}", marker.display()),
                ]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap();

        assert_eq!(response.exit_code, 0);

        // Wait for the background process to finish
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !marker.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "detached process did not finish"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "ok");
        stop_agent(child).await;
    }

    #[tokio::test]
    async fn contract_copy_to_vm() {
        let temp = tempfile::tempdir().unwrap();
        let child = spawn_agent(temp.path()).await;
        let client = protocol::ProtocolClient::with_timeouts(
            temp.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );

        // Stage a file in transfers/
        let stage_dir = temp.path().join("transfers").join("rust_copy_in");
        tokio::fs::create_dir_all(&stage_dir).await.unwrap();
        tokio::fs::write(stage_dir.join("payload.txt"), "from rust")
            .await
            .unwrap();

        let dest_dir = temp.path().join("vm_dest");
        let response = client
            .send_request(&protocol::Request {
                kind: protocol::RequestType::CopyToVm,
                cmd: None,
                stdin_b64: None,
                src_path: None,
                dest_path: Some(dest_dir.to_string_lossy().to_string()),
                transfer_path: Some("transfers/rust_copy_in/payload.txt".into()),
            })
            .await
            .unwrap();

        assert_eq!(response.exit_code, 0);
        assert!(response.error.is_none());
        let copied = dest_dir.join("payload.txt");
        assert!(copied.exists(), "file was not copied to dest");
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "from rust");
        stop_agent(child).await;
    }

    #[tokio::test]
    async fn contract_copy_from_vm() {
        let temp = tempfile::tempdir().unwrap();
        let child = spawn_agent(temp.path()).await;
        let client = protocol::ProtocolClient::with_timeouts(
            temp.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );

        // Create a source file the agent can read
        let src_file = temp.path().join("vm_file.txt");
        tokio::fs::write(&src_file, "from python").await.unwrap();

        let stage_dir = temp.path().join("transfers").join("rust_copy_out");
        tokio::fs::create_dir_all(&stage_dir).await.unwrap();

        let response = client
            .send_request(&protocol::Request {
                kind: protocol::RequestType::CopyFromVm,
                cmd: None,
                stdin_b64: None,
                src_path: Some(src_file.to_string_lossy().to_string()),
                dest_path: None,
                transfer_path: Some("transfers/rust_copy_out".into()),
            })
            .await
            .unwrap();

        assert_eq!(response.exit_code, 0);
        assert!(response.error.is_none());
        let copied = stage_dir.join("vm_file.txt");
        assert!(copied.exists(), "file was not copied to stage dir");
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "from python");
        stop_agent(child).await;
    }

    #[tokio::test]
    async fn contract_agent_error_propagates() {
        let temp = tempfile::tempdir().unwrap();
        let child = spawn_agent(temp.path()).await;
        let client = protocol::ProtocolClient::with_timeouts(
            temp.path(),
            Duration::from_secs(5),
            Duration::from_millis(50),
        );

        // copy_from_vm with nonexistent source should return an error
        let stage_dir = temp.path().join("transfers").join("err_test");
        tokio::fs::create_dir_all(&stage_dir).await.unwrap();

        let err = client
            .send_request(&protocol::Request {
                kind: protocol::RequestType::CopyFromVm,
                cmd: None,
                stdin_b64: None,
                src_path: Some("/no/such/path.txt".into()),
                dest_path: None,
                transfer_path: Some("transfers/err_test".into()),
            })
            .await
            .unwrap_err();

        assert!(
            err.to_string().contains("Tart VM agent error"),
            "expected agent error, got: {err}"
        );
        stop_agent(child).await;
    }
}
