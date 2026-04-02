#![allow(dead_code)]

pub mod deploy;
pub mod readiness;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::error::AppError;
use crate::session::Session;
use crate::vm_protocol::{
    ProtocolClient, Request, RequestType, next_request_id, relative_transfer_path,
};

/// Guest-side shared directory mount point (VirtIO-FS via WinFsp).
const WINDOWS_GUEST_SHARED_DIR: &str = "Z:\\";

const DEFAULT_AGENT_READY_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub struct WindowsVmSession {
    vm_name: String,
    shared_dir: PathBuf,
    overlay_path: PathBuf,
    ovmf_vars_path: PathBuf,
    tpm_state_dir: PathBuf,
    qmp_sock: PathBuf,
    protocol: ProtocolClient,
    qemu_child: Arc<Mutex<Option<Child>>>,
    virtiofsd_child: Arc<Mutex<Option<Child>>>,
    swtpm_child: Arc<Mutex<Option<Child>>>,
}

/// Clean up stale resources from previously crashed Windows VM sessions.
///
/// Scans the system temp directory for `desktest-windows-*-shared` directories
/// that are no longer associated with a running QEMU process.
pub fn cleanup_stale_windows_vms() {
    let tmp = std::env::temp_dir();
    let entries = match std::fs::read_dir(&tmp) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("desktest-windows-") || !name.ends_with("-shared") {
            continue;
        }

        let shared_path = entry.path();
        let pid_file = shared_path.join(".pid");

        // Check if the QEMU process is still running
        let still_running = if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                // kill(pid, 0) tests process existence without sending a signal
                unsafe { libc::kill(pid, 0) == 0 }
            } else {
                false
            }
        } else {
            false
        };

        if !still_running {
            debug!("Cleaning up stale Windows VM shared dir: {name}");

            // Try to kill orphaned virtiofsd and swtpm daemons
            kill_daemon_from_pidfile(&shared_path.join(".virtiofsd.pid"));
            kill_daemon_from_pidfile(&shared_path.join(".swtpm.pid"));

            // Clean up overlay QCOW2 and OVMF vars (paths stored in metadata)
            if let Ok(overlay) = std::fs::read_to_string(shared_path.join(".overlay_path")) {
                let _ = std::fs::remove_file(overlay.trim());
            }
            if let Ok(vars) = std::fs::read_to_string(shared_path.join(".ovmf_vars_path")) {
                let _ = std::fs::remove_file(vars.trim());
            }

            // Clean up TPM state directory
            if let Ok(tpm_dir) = std::fs::read_to_string(shared_path.join(".tpm_state_dir")) {
                let _ = std::fs::remove_dir_all(tpm_dir.trim());
            }

            let _ = std::fs::remove_dir_all(&shared_path);
        }
    }
}

fn kill_daemon_from_pidfile(pid_file: &Path) {
    if let Ok(pid_str) = std::fs::read_to_string(pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
    }
}

impl WindowsVmSession {
    pub async fn create(base_image: &str) -> Result<Self, AppError> {
        // Opportunistically clean up shared dirs from crashed sessions.
        let _ = tokio::task::spawn_blocking(cleanup_stale_windows_vms).await;

        let request_id = next_request_id();
        let vm_name = format!("desktest-windows-{request_id}");
        let tmp = std::env::temp_dir();
        let shared_dir = tmp.join(format!("{vm_name}-shared"));
        let overlay_path = tmp.join(format!("{vm_name}.qcow2"));
        let ovmf_vars_path = tmp.join(format!("{vm_name}-OVMF_VARS.fd"));
        let tpm_state_dir = tmp.join(format!("{vm_name}-tpm"));
        let qmp_sock = shared_dir.join("qmp.sock");

        let protocol = ProtocolClient::with_timeouts(
            &shared_dir,
            "Windows VM",
            DEFAULT_REQUEST_TIMEOUT,
            DEFAULT_POLL_INTERVAL,
        );
        protocol.ensure_layout().await?;

        // Write metadata files for stale cleanup
        tokio::fs::write(
            shared_dir.join(".overlay_path"),
            overlay_path.to_string_lossy().as_ref(),
        )
        .await?;
        tokio::fs::write(
            shared_dir.join(".ovmf_vars_path"),
            ovmf_vars_path.to_string_lossy().as_ref(),
        )
        .await?;
        tokio::fs::write(
            shared_dir.join(".tpm_state_dir"),
            tpm_state_dir.to_string_lossy().as_ref(),
        )
        .await?;

        // 1. Create QCOW2 overlay from base image
        info!("Creating QCOW2 overlay from '{base_image}'...");
        run_command(
            "qemu-img",
            &[
                "create",
                "-b",
                base_image,
                "-F",
                "qcow2",
                "-f",
                "qcow2",
                &overlay_path.to_string_lossy(),
            ],
        )
        .await?;

        // 2. Copy OVMF_VARS template for this VM
        let ovmf_vars_template = "/usr/share/OVMF/OVMF_VARS.fd";
        tokio::fs::copy(ovmf_vars_template, &ovmf_vars_path)
            .await
            .map_err(|e| {
                AppError::Infra(format!(
                    "Cannot copy OVMF_VARS template from {ovmf_vars_template}: {e}\n\
                     Install with: sudo apt install ovmf"
                ))
            })?;

        // 3. Create TPM state directory and start swtpm
        tokio::fs::create_dir_all(&tpm_state_dir).await?;
        let swtpm_sock = shared_dir.join("swtpm.sock");
        info!("Starting swtpm...");
        let swtpm_child = tokio::process::Command::new("swtpm")
            .args([
                "socket",
                "--tpmstate",
                &format!("dir={}", tpm_state_dir.display()),
                "--ctrl",
                &format!("type=unixio,path={}", swtpm_sock.display()),
                "--tpm2",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::Infra(format!(
                    "Failed to spawn swtpm: {e}\n\
                     Install with: sudo apt install swtpm"
                ))
            })?;

        // Write swtpm PID for stale cleanup
        if let Some(pid) = swtpm_child.id() {
            let _ = tokio::fs::write(shared_dir.join(".swtpm.pid"), pid.to_string()).await;
        }

        // Wait for swtpm socket to appear (up to 10s)
        let swtpm_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if swtpm_sock.exists() {
                break;
            }
            if tokio::time::Instant::now() >= swtpm_deadline {
                return Err(AppError::Infra(
                    "Timed out waiting for swtpm socket to appear".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // 4. Start virtiofsd for shared directory
        info!("Starting virtiofsd...");
        let virtiofsd_sock = shared_dir.join("virtiofsd.sock");
        let virtiofsd_child = tokio::process::Command::new("virtiofsd")
            .args([
                &format!("--socket-path={}", virtiofsd_sock.display()),
                &format!("--shared-dir={}", shared_dir.display()),
                "--sandbox=chroot",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::Infra(format!(
                    "Failed to spawn virtiofsd: {e}\n\
                     Install with: sudo apt install virtiofsd"
                ))
            })?;

        // Write virtiofsd PID for stale cleanup
        if let Some(pid) = virtiofsd_child.id() {
            let _ = tokio::fs::write(shared_dir.join(".virtiofsd.pid"), pid.to_string()).await;
        }

        // Wait for virtiofsd socket to appear (up to 10s)
        let vfs_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if virtiofsd_sock.exists() {
                break;
            }
            if tokio::time::Instant::now() >= vfs_deadline {
                return Err(AppError::Infra(
                    "Timed out waiting for virtiofsd socket to appear".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // 5. Spawn QEMU
        info!("Starting QEMU...");
        let qemu_child = tokio::process::Command::new("qemu-system-x86_64")
            .args([
                "-enable-kvm",
                "-m",
                "4G",
                "-smp",
                "4",
                "-object",
                "memory-backend-memfd,id=mem,size=4G,share=on",
                "-numa",
                "node,memdev=mem",
                // UEFI firmware (pflash pair)
                "-drive",
                "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd",
                "-drive",
                &format!("if=pflash,format=raw,file={}", ovmf_vars_path.display()),
                // TPM 2.0
                "-chardev",
                &format!("socket,id=chrtpm,path={}", swtpm_sock.display()),
                "-tpmdev",
                "emulator,id=tpm0,chardev=chrtpm",
                "-device",
                "tpm-tis,tpmdev=tpm0",
                // Disk
                "-drive",
                &format!("file={},if=virtio", overlay_path.display()),
                // VirtIO-FS shared directory
                "-chardev",
                &format!("socket,id=char0,path={}", virtiofsd_sock.display()),
                "-device",
                "vhost-user-fs-pci,chardev=char0,tag=desktest",
                // QMP monitor socket
                "-qmp",
                &format!("unix:{},server,wait=off", qmp_sock.display()),
                // Display
                "-display",
                "none",
                "-vnc",
                "none",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::Infra(format!(
                    "Failed to spawn qemu-system-x86_64: {e}\n\
                     Install with: sudo apt install qemu-system-x86"
                ))
            })?;

        // Write QEMU PID for stale cleanup
        if let Some(pid) = qemu_child.id() {
            let _ = tokio::fs::write(shared_dir.join(".pid"), pid.to_string()).await;
        }

        let session = Self {
            vm_name,
            shared_dir,
            overlay_path,
            ovmf_vars_path,
            tpm_state_dir,
            qmp_sock,
            protocol,
            qemu_child: Arc::new(Mutex::new(Some(qemu_child))),
            virtiofsd_child: Arc::new(Mutex::new(Some(virtiofsd_child))),
            swtpm_child: Arc::new(Mutex::new(Some(swtpm_child))),
        };

        // 6. Wait for agent_ready sentinel
        info!("Waiting for Windows VM agent to become ready...");
        if let Err(e) = session
            .protocol
            .wait_for_agent_ready(DEFAULT_AGENT_READY_TIMEOUT)
            .await
        {
            // Tear down all running children before propagating the error
            // to avoid orphaned QEMU/virtiofsd/swtpm processes.
            Self::kill_child(&session.qemu_child).await;
            Self::kill_child(&session.virtiofsd_child).await;
            Self::kill_child(&session.swtpm_child).await;
            let _ = tokio::fs::remove_file(&session.overlay_path).await;
            let _ = tokio::fs::remove_file(&session.ovmf_vars_path).await;
            let _ = tokio::fs::remove_dir_all(&session.tpm_state_dir).await;
            let _ = tokio::fs::remove_dir_all(&session.shared_dir).await;
            return Err(e);
        }

        info!("Windows VM '{}' is ready", session.vm_name);
        Ok(session)
    }

    pub fn vm_name(&self) -> &str {
        &self.vm_name
    }

    pub fn shared_dir(&self) -> &Path {
        &self.shared_dir
    }

    pub fn guest_shared_dir(&self) -> &str {
        WINDOWS_GUEST_SHARED_DIR
    }

    pub fn qmp_sock(&self) -> &Path {
        &self.qmp_sock
    }

    async fn send(&self, request: Request) -> Result<crate::vm_protocol::Response, AppError> {
        self.protocol.send_request(&request).await
    }

    async fn prepare_transfer_in(&self, src: &Path) -> Result<(PathBuf, String), AppError> {
        let request_id = next_request_id();
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
        let request_id = next_request_id();
        let stage_dir = self.protocol.transfer_stage(&request_id);
        tokio::fs::create_dir_all(&stage_dir).await?;
        let relative = relative_transfer_path(&self.shared_dir, &stage_dir)?;
        Ok((stage_dir, relative))
    }

    async fn kill_child(child_mutex: &Arc<Mutex<Option<Child>>>) {
        let mut guard = child_mutex.lock().await;
        if let Some(child) = guard.as_mut() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        *guard = None;
    }
}

impl Session for WindowsVmSession {
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
        // Use PowerShell to launch the process detached with output redirection.
        // `cmd /c start /b ... > logfile` does NOT work: it redirects start's own
        // stdout (empty), not the child's. PowerShell's *> merges all output streams.
        let inner_cmd = cmd
            .iter()
            .map(|s| {
                if s.contains(' ') {
                    // Escape embedded single quotes by doubling them for PowerShell
                    format!("'{}'", s.replace('\'', "''"))
                } else {
                    (*s).to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let escaped_log = log_path.replace('\'', "''");
        // Use `& cmd args` (not `& { cmd args }`) so that a single-quoted path
        // like `'C:\Temp\My App.exe'` is invoked as a command, not evaluated as
        // a string literal expression.
        self.exec_detached(&[
            "powershell",
            "-NonInteractive",
            "-Command",
            &format!("& {inner_cmd} *> '{escaped_log}'"),
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
                    "Windows VM transfer stage {} is empty",
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
        info!("Cleaning up Windows VM '{}'...", self.vm_name);

        // 1. Send ACPI shutdown via QMP (graceful)
        if self.qmp_sock.exists() {
            debug!("Sending ACPI shutdown via QMP...");
            // Best-effort: if QMP fails, we'll force-kill
            let _ = send_qmp_command(&self.qmp_sock, r#"{"execute": "system_powerdown"}"#).await;
        }

        // 2. Wait briefly, then force-kill QEMU
        tokio::time::sleep(Duration::from_secs(5)).await;
        Self::kill_child(&self.qemu_child).await;

        // 3. Stop virtiofsd and swtpm
        Self::kill_child(&self.virtiofsd_child).await;
        Self::kill_child(&self.swtpm_child).await;

        // 4. Delete overlay QCOW2 and OVMF_VARS copy
        let _ = tokio::fs::remove_file(&self.overlay_path).await;
        let _ = tokio::fs::remove_file(&self.ovmf_vars_path).await;

        // 5. Remove TPM state directory
        let _ = tokio::fs::remove_dir_all(&self.tpm_state_dir).await;

        // 6. Remove shared directory
        let _ = tokio::fs::remove_dir_all(&self.shared_dir).await;

        Ok(())
    }
}

/// Send a command to the QEMU Machine Protocol (QMP) socket.
///
/// All I/O operations are wrapped with timeouts to prevent cleanup() from
/// hanging indefinitely if QEMU is in a bad state or the socket is stale.
async fn send_qmp_command(qmp_sock: &Path, command: &str) -> Result<(), AppError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let connect_timeout = Duration::from_secs(3);
    let io_timeout = Duration::from_secs(5);

    let mut stream = tokio::time::timeout(connect_timeout, UnixStream::connect(qmp_sock))
        .await
        .map_err(|_| AppError::Infra("QMP connect timed out".into()))?
        .map_err(|e| AppError::Infra(format!("Cannot connect to QMP socket: {e}")))?;

    // Read QMP greeting
    let mut buf = vec![0u8; 4096];
    let _ = tokio::time::timeout(io_timeout, stream.read(&mut buf)).await;

    // Send qmp_capabilities to enter command mode
    stream
        .write_all(b"{\"execute\": \"qmp_capabilities\"}\n")
        .await
        .map_err(|e| AppError::Infra(format!("QMP capabilities handshake failed: {e}")))?;
    let _ = tokio::time::timeout(io_timeout, stream.read(&mut buf)).await;

    // Send the actual command
    stream
        .write_all(format!("{command}\n").as_bytes())
        .await
        .map_err(|e| AppError::Infra(format!("QMP command failed: {e}")))?;
    let _ = tokio::time::timeout(io_timeout, stream.read(&mut buf)).await;

    Ok(())
}

async fn run_command(program: &str, args: &[&str]) -> Result<(), AppError> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| AppError::Infra(format!("Failed to run `{program}`: {e}")))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(AppError::Infra(format!(
        "`{program} {}` failed with status {}{}",
        args.join(" "),
        output.status,
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    )))
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
