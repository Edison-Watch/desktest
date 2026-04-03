use std::path::{Path, PathBuf};
use std::process::Stdio;

use tracing::info;

use crate::error::AppError;

/// Run `desktest init-windows`: prepare a Windows 11 golden image for QEMU/KVM testing.
///
/// Two-stage process:
/// 1. Boot QEMU with Windows ISO + VirtIO driver ISO + secondary provisioning ISO
///    containing Autounattend.xml. Windows installs unattended and shuts down.
/// 2. Boot the installed QCOW2 with SSH port forwarding, SCP agent scripts in,
///    and run provision.ps1 via SSH to install Python, PyAutoGUI, WinFsp, etc.
pub async fn run_init_windows(
    windows_iso: &Path,
    virtio_iso: &Path,
    output: &Path,
    ram: &str,
    cpus: u32,
    disk_size: &str,
) -> Result<(), AppError> {
    // 1. Verify prerequisites
    crate::preflight::check_windows_vm()?;
    let iso_tool = check_iso_builder()?;
    check_sshpass_installed()?;

    // Validate inputs exist
    if !windows_iso.exists() {
        return Err(AppError::Config(format!(
            "Windows ISO not found: {}",
            windows_iso.display()
        )));
    }
    if !virtio_iso.exists() {
        return Err(AppError::Config(format!(
            "VirtIO driver ISO not found: {}",
            virtio_iso.display()
        )));
    }
    if output.exists() {
        return Err(AppError::Config(format!(
            "Output file already exists: {}\nDelete it first or choose a different path.",
            output.display()
        )));
    }

    let windows_dir = find_windows_dir()?;

    println!("Preparing Windows 11 golden image...");
    println!("  Windows ISO:  {}", windows_iso.display());
    println!("  VirtIO ISO:   {}", virtio_iso.display());
    println!("  Output:       {}", output.display());
    println!("  Disk size:    {disk_size}");
    println!("  RAM:          {ram}");
    println!("  CPUs:         {cpus}");
    println!();

    // Create temporary working directory
    let work_dir = std::env::temp_dir().join(format!("desktest-init-windows-{}", std::process::id()));
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create work dir: {e}")))?;

    let result = run_both_stages(
        &windows_dir,
        windows_iso,
        virtio_iso,
        output,
        ram,
        cpus,
        disk_size,
        &iso_tool,
        &work_dir,
    )
    .await;

    // Clean up work directory
    let _ = std::fs::remove_dir_all(&work_dir);

    if let Err(e) = &result {
        eprintln!("init-windows failed: {e}");
        // Clean up partial output
        let _ = std::fs::remove_file(output);
    } else {
        println!();
        println!("Golden image '{}' is ready!", output.display());
        println!();
        println!("Next steps:");
        println!("  1. Create a task JSON with '\"type\": \"windows_vm\"' and '\"base_image\": \"{}\"'", output.display());
        println!("  2. Run: desktest run your-task.json");
        println!();
        println!("The image includes: Python 3, PyAutoGUI, uiautomation, WinFsp,");
        println!("vm-agent (scheduled task), auto-login, disabled UAC/Defender/Updates.");
    }

    result
}

async fn run_both_stages(
    windows_dir: &Path,
    windows_iso: &Path,
    virtio_iso: &Path,
    output: &Path,
    ram: &str,
    cpus: u32,
    disk_size: &str,
    iso_tool: &str,
    work_dir: &Path,
) -> Result<(), AppError> {
    // ── Stage 1: Windows installation from ISO ──────────────────────────
    println!("Stage 1: Installing Windows from ISO (this takes 15-30 minutes)...");
    stage1_install(
        windows_dir,
        windows_iso,
        virtio_iso,
        output,
        ram,
        cpus,
        disk_size,
        iso_tool,
        work_dir,
    )
    .await?;
    println!("Stage 1 complete: Windows installed.");
    println!();

    // ── Stage 2: Provisioning via SSH ───────────────────────────────────
    println!("Stage 2: Provisioning dependencies via SSH...");
    stage2_provision(windows_dir, output, ram, cpus, work_dir).await?;
    println!("Stage 2 complete: Golden image provisioned.");

    Ok(())
}

/// Stage 1: Create a secondary ISO with Autounattend.xml, boot QEMU with
/// Windows ISO + VirtIO ISO + secondary ISO, wait for unattended install
/// to complete (QEMU exits when guest shuts down).
async fn stage1_install(
    windows_dir: &Path,
    windows_iso: &Path,
    virtio_iso: &Path,
    output: &Path,
    ram: &str,
    cpus: u32,
    disk_size: &str,
    iso_tool: &str,
    work_dir: &Path,
) -> Result<(), AppError> {
    // 1. Create secondary ISO containing Autounattend.xml
    let autounattend_iso = work_dir.join("autounattend.iso");
    let iso_staging = work_dir.join("iso-staging");
    std::fs::create_dir_all(&iso_staging)
        .map_err(|e| AppError::Infra(format!("Cannot create ISO staging dir: {e}")))?;

    let autounattend_src = windows_dir.join("Autounattend.xml");
    if !autounattend_src.exists() {
        return Err(AppError::Config(format!(
            "Autounattend.xml not found at {}",
            autounattend_src.display()
        )));
    }
    std::fs::copy(&autounattend_src, iso_staging.join("Autounattend.xml"))
        .map_err(|e| AppError::Infra(format!("Cannot copy Autounattend.xml: {e}")))?;

    info!("Creating secondary ISO with {iso_tool}...");
    let iso_output_str = autounattend_iso.to_string_lossy().to_string();
    let iso_staging_str = iso_staging.to_string_lossy().to_string();
    run_command(
        iso_tool,
        &["-o", &iso_output_str, "-J", "-R", "-quiet", &iso_staging_str],
    )
    .await?;

    // 2. Create target QCOW2 disk
    info!("Creating QCOW2 disk ({disk_size})...");
    run_command(
        "qemu-img",
        &["create", "-f", "qcow2", &output.to_string_lossy(), disk_size],
    )
    .await?;

    // 3. Set up UEFI firmware and TPM
    let ovmf_vars_path = work_dir.join("OVMF_VARS.fd");
    tokio::fs::copy("/usr/share/OVMF/OVMF_VARS.fd", &ovmf_vars_path)
        .await
        .map_err(|e| AppError::Infra(format!("Cannot copy OVMF_VARS template: {e}")))?;

    let tpm_state_dir = work_dir.join("tpm");
    tokio::fs::create_dir_all(&tpm_state_dir).await?;
    let swtpm_sock = work_dir.join("swtpm.sock");

    let mut swtpm_child = tokio::process::Command::new("swtpm")
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
        .map_err(|e| AppError::Infra(format!("Failed to spawn swtpm: {e}")))?;

    // Wait for swtpm socket
    wait_for_socket(&swtpm_sock, 10).await?;

    // 4. Spawn QEMU for Windows installation
    //    - Windows ISO as primary CD-ROM
    //    - VirtIO driver ISO as secondary CD-ROM
    //    - Autounattend ISO as tertiary CD-ROM
    //    - No VirtIO-FS (WinFsp not installed yet)
    //    - No VNC (headless install)
    info!("Starting QEMU for Windows installation...");
    let cpus_str = cpus.to_string();
    let mem_backend = format!("memory-backend-memfd,id=mem,size={ram},share=on");
    let ovmf_vars_arg = format!("if=pflash,format=raw,file={}", ovmf_vars_path.display());
    let disk_arg = format!("file={},if=virtio", output.display());
    let tpm_chardev = format!("socket,id=chrtpm,path={}", swtpm_sock.display());
    let windows_iso_arg = format!("file={},media=cdrom,index=0", windows_iso.display());
    let virtio_iso_arg = format!("file={},media=cdrom,index=1", virtio_iso.display());
    let autounattend_iso_arg =
        format!("file={},media=cdrom,index=2", autounattend_iso.display());

    let mut qemu_child = tokio::process::Command::new("qemu-system-x86_64")
        .args([
            "-enable-kvm",
            "-m",
            ram,
            "-smp",
            &cpus_str,
            "-object",
            &mem_backend,
            "-numa",
            "node,memdev=mem",
            // UEFI firmware
            "-drive",
            "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd",
            "-drive",
            &ovmf_vars_arg,
            // TPM 2.0
            "-chardev",
            &tpm_chardev,
            "-tpmdev",
            "emulator,id=tpm0,chardev=chrtpm",
            "-device",
            "tpm-tis,tpmdev=tpm0",
            // Disk
            "-drive",
            &disk_arg,
            // CD-ROMs: Windows ISO, VirtIO drivers, Autounattend
            "-drive",
            &windows_iso_arg,
            "-drive",
            &virtio_iso_arg,
            "-drive",
            &autounattend_iso_arg,
            // Network (needed for driver install, not for internet)
            "-nic",
            "user,model=virtio-net-pci",
            // Display
            "-display",
            "none",
            "-vnc",
            "none",
            // Boot from CD-ROM first
            "-boot",
            "d",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit()) // Show any serial output
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| AppError::Infra(format!("Failed to spawn QEMU: {e}")))?;

    println!("QEMU started (PID: {:?}). Waiting for Windows installation to complete...", qemu_child.id());
    println!("(The VM will shut down automatically when installation finishes)");

    // Wait for QEMU to exit (Windows installs and then shuts down via Autounattend)
    let install_timeout = std::time::Duration::from_secs(45 * 60); // 45 minutes max
    match tokio::time::timeout(install_timeout, qemu_child.wait()).await {
        Ok(Ok(status)) => {
            if !status.success() {
                // QEMU exits with non-zero when guest initiates shutdown — this is normal
                info!("QEMU exited with status {status} (expected for guest shutdown)");
            }
        }
        Ok(Err(e)) => {
            let _ = swtpm_child.kill().await;
            return Err(AppError::Infra(format!("QEMU process error: {e}")));
        }
        Err(_) => {
            let _ = qemu_child.kill().await;
            let _ = swtpm_child.kill().await;
            return Err(AppError::Infra(
                "Windows installation timed out after 45 minutes".into(),
            ));
        }
    }

    // Clean up swtpm
    let _ = swtpm_child.kill().await;
    let _ = swtpm_child.wait().await;

    Ok(())
}

/// Stage 2: Boot the installed QCOW2, SSH in, copy agent scripts, run provision.ps1.
async fn stage2_provision(
    windows_dir: &Path,
    output: &Path,
    ram: &str,
    cpus: u32,
    work_dir: &Path,
) -> Result<(), AppError> {
    // Set up UEFI and TPM again (fresh instances for Stage 2 boot)
    let ovmf_vars_path = work_dir.join("OVMF_VARS_s2.fd");
    tokio::fs::copy("/usr/share/OVMF/OVMF_VARS.fd", &ovmf_vars_path)
        .await
        .map_err(|e| AppError::Infra(format!("Cannot copy OVMF_VARS template: {e}")))?;

    let tpm_state_dir = work_dir.join("tpm-s2");
    tokio::fs::create_dir_all(&tpm_state_dir).await?;
    let swtpm_sock = work_dir.join("swtpm-s2.sock");

    let mut swtpm_child = tokio::process::Command::new("swtpm")
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
        .map_err(|e| AppError::Infra(format!("Failed to spawn swtpm (stage 2): {e}")))?;

    wait_for_socket(&swtpm_sock, 10).await?;

    // Boot QEMU with SSH port forwarding (host:2222 → guest:22)
    info!("Starting QEMU for provisioning (SSH on localhost:2222)...");
    let cpus_str = cpus.to_string();
    let mem_backend = format!("memory-backend-memfd,id=mem,size={ram},share=on");
    let ovmf_vars_arg = format!("if=pflash,format=raw,file={}", ovmf_vars_path.display());
    let disk_arg = format!("file={},if=virtio", output.display());
    let tpm_chardev = format!("socket,id=chrtpm,path={}", swtpm_sock.display());

    let mut qemu_child = tokio::process::Command::new("qemu-system-x86_64")
        .args([
            "-enable-kvm",
            "-m",
            ram,
            "-smp",
            &cpus_str,
            "-object",
            &mem_backend,
            "-numa",
            "node,memdev=mem",
            // UEFI firmware
            "-drive",
            "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd",
            "-drive",
            &ovmf_vars_arg,
            // TPM 2.0
            "-chardev",
            &tpm_chardev,
            "-tpmdev",
            "emulator,id=tpm0,chardev=chrtpm",
            "-device",
            "tpm-tis,tpmdev=tpm0",
            // Disk (the installed Windows image)
            "-drive",
            &disk_arg,
            // Network with SSH port forwarding
            "-nic",
            "user,model=virtio-net-pci,hostfwd=tcp::2222-:22",
            // Display
            "-display",
            "none",
            "-vnc",
            "none",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| AppError::Infra(format!("Failed to spawn QEMU (stage 2): {e}")))?;

    println!("QEMU started (PID: {:?}). Waiting for SSH...", qemu_child.id());

    // Wait for SSH to become available
    match wait_for_ssh().await {
        Ok(()) => info!("SSH is available"),
        Err(e) => {
            let _ = qemu_child.kill().await;
            let _ = swtpm_child.kill().await;
            return Err(e);
        }
    }

    // Copy agent scripts into VM via SCP
    println!("Copying agent scripts to VM...");
    let provision_result = copy_and_provision(windows_dir, &mut qemu_child).await;

    // Wait for QEMU to exit (provision.ps1 shuts down the guest)
    println!("Waiting for VM to shut down...");
    let shutdown_timeout = std::time::Duration::from_secs(120);
    match tokio::time::timeout(shutdown_timeout, qemu_child.wait()).await {
        Ok(_) => info!("VM shut down after provisioning"),
        Err(_) => {
            tracing::warn!("VM did not shut down within 120s, force-killing");
            let _ = qemu_child.kill().await;
            let _ = qemu_child.wait().await;
        }
    }

    // Clean up swtpm
    let _ = swtpm_child.kill().await;
    let _ = swtpm_child.wait().await;

    provision_result
}

/// Copy agent scripts via SCP and run provision.ps1 via SSH.
async fn copy_and_provision(
    windows_dir: &Path,
    qemu_child: &mut tokio::process::Child,
) -> Result<(), AppError> {
    let ssh_opts = [
        "-o", "StrictHostKeyChecking=no",
        "-o", "UserKnownHostsFile=/dev/null",
        "-o", "PreferredAuthentications=password",
        "-o", "PubkeyAuthentication=no",
        "-p", "2222",
    ];

    // Create the provisioning directory on the guest
    run_sshpass(&ssh_opts, &["tester@localhost", "mkdir", "-p", "C:\\Temp\\desktest-provision"]).await
        .map_err(|e| AppError::Infra(format!("Failed to create provision dir on guest: {e}")))?;

    // SCP agent scripts
    let scripts = ["vm-agent.py", "execute-action.py", "get-a11y-tree.py", "win-screenshot.py"];
    for script in &scripts {
        let src = windows_dir.join(script);
        if src.exists() {
            println!("  Copying {script}...");
            run_scp(&ssh_opts, &src, "tester@localhost:C:\\Temp\\desktest-provision\\").await?;
        } else {
            tracing::warn!("{script} not found at {}", src.display());
        }
    }

    // SCP provision.ps1
    let provision_script = windows_dir.join("provision.ps1");
    if !provision_script.exists() {
        return Err(AppError::Config(format!(
            "provision.ps1 not found at {}",
            provision_script.display()
        )));
    }
    println!("  Copying provision.ps1...");
    run_scp(&ssh_opts, &provision_script, "tester@localhost:C:\\Temp\\").await?;

    // Check if QEMU died during file copy
    if let Ok(Some(status)) = qemu_child.try_wait() {
        return Err(AppError::Infra(format!(
            "QEMU exited unexpectedly during file copy: {status}"
        )));
    }

    // Run provision.ps1 via SSH
    println!("Running provision.ps1 (this takes 5-15 minutes)...");
    let status = tokio::process::Command::new("sshpass")
        .args(["-p", "desktest"])
        .args(["ssh"])
        .args(&ssh_opts)
        .args([
            "tester@localhost",
            "powershell",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            "C:\\Temp\\provision.ps1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| AppError::Infra(format!("SSH provisioning failed: {e}")))?;

    // Exit code 255 is expected: provision.ps1 ends with `shutdown /s`,
    // which kills the SSH connection.
    let code = status.code().unwrap_or(-1);
    if !status.success() && code != 255 {
        return Err(AppError::Infra(format!(
            "Provisioning script failed with exit code: {code}",
        )));
    }

    Ok(())
}

// ─── Helper functions ────────────────────────────────────────────────────────

/// Wait for SSH to become available on localhost:2222.
async fn wait_for_ssh() -> Result<(), AppError> {
    let timeout = std::time::Duration::from_secs(180);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Infra(
                "Timeout waiting for SSH to become available on the Windows VM.\n\
                 The VM may have failed to boot or OpenSSH Server may not be running."
                    .into(),
            ));
        }

        let check = tokio::process::Command::new("sshpass")
            .args(["-p", "desktest"])
            .args([
                "ssh",
                "-o", "StrictHostKeyChecking=no",
                "-o", "UserKnownHostsFile=/dev/null",
                "-o", "PreferredAuthentications=password",
                "-o", "PubkeyAuthentication=no",
                "-o", "ConnectTimeout=5",
                "-p", "2222",
                "tester@localhost",
                "echo",
                "ready",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        match check {
            Ok(status) if status.success() => return Ok(()),
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        }
    }
}

/// Run sshpass with SSH command.
async fn run_sshpass(ssh_opts: &[&str], cmd: &[&str]) -> Result<(), AppError> {
    let mut args = vec!["-p", "desktest", "ssh"];
    args.extend_from_slice(ssh_opts);
    args.extend_from_slice(cmd);

    let status = tokio::process::Command::new("sshpass")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| AppError::Infra(format!("sshpass/ssh failed: {e}")))?;

    if !status.success() {
        return Err(AppError::Infra(format!(
            "SSH command failed with exit code {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

/// Run sshpass + scp to copy a file to the guest.
async fn run_scp(ssh_opts: &[&str], src: &Path, dest: &str) -> Result<(), AppError> {
    let mut args: Vec<&str> = vec!["-p", "desktest", "scp"];
    // Convert SSH opts to SCP opts (replace -p with -P for port)
    for chunk in ssh_opts.chunks(2) {
        if chunk.len() == 2 {
            if chunk[0] == "-p" {
                args.push("-P");
                args.push(chunk[1]);
            } else {
                args.push(chunk[0]);
                args.push(chunk[1]);
            }
        }
    }
    let src_str = src.to_string_lossy();
    args.push(&src_str);
    args.push(dest);

    let output = tokio::process::Command::new("sshpass")
        .args(&args)
        .output()
        .await
        .map_err(|e| AppError::Infra(format!("sshpass/scp failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Infra(format!(
            "SCP failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        )));
    }
    Ok(())
}

/// Wait for a Unix socket to appear on disk.
async fn wait_for_socket(path: &Path, timeout_secs: u64) -> Result<(), AppError> {
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if path.exists() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Infra(format!(
                "Timed out waiting for socket: {}",
                path.display()
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Run a command and return an error if it fails.
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

/// Check that genisoimage or mkisofs is available.
fn check_iso_builder() -> Result<String, AppError> {
    for tool in &["genisoimage", "mkisofs"] {
        if std::process::Command::new(tool)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Ok((*tool).to_string());
        }
    }
    Err(AppError::Config(
        "Neither genisoimage nor mkisofs is installed.\n\
         Install with: sudo apt install genisoimage"
            .into(),
    ))
}

/// Verify that `sshpass` is installed.
fn check_sshpass_installed() -> Result<(), AppError> {
    match std::process::Command::new("sshpass")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(_) => Ok(()),
        Err(_) => Err(AppError::Config(
            "sshpass is not installed. It is required to provision the Windows VM.\n\
             Install with: sudo apt install sshpass"
                .into(),
        )),
    }
}

/// Find the `windows/` directory containing agent scripts and provisioning files.
fn find_windows_dir() -> Result<PathBuf, AppError> {
    // Try relative to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            // In dev: target/debug/desktest → ../../windows
            let dev_path = parent.join("../../windows");
            if dev_path.join("vm-agent.py").exists() {
                return Ok(std::fs::canonicalize(&dev_path).unwrap_or(dev_path));
            }
            // In release: alongside the binary
            let release_path = parent.join("windows");
            if release_path.join("vm-agent.py").exists() {
                return Ok(release_path);
            }
        }
    }

    // Try relative to CWD
    let cwd_path = PathBuf::from("windows");
    if cwd_path.join("vm-agent.py").exists() {
        return Ok(std::fs::canonicalize(&cwd_path).unwrap_or(cwd_path));
    }

    Err(AppError::Config(
        "Cannot find the 'windows/' directory containing agent scripts.\n\
         Run this command from the desktest repository root, or ensure the 'windows/' \
         directory is alongside the desktest binary."
            .into(),
    ))
}
