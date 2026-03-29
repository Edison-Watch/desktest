use std::path::Path;
use std::process::Stdio;

use tracing::info;

use crate::error::AppError;

/// Run `desktest init-macos`: prepare a macOS golden image for Tart-based testing.
///
/// Steps:
/// 1. Verify Tart is installed
/// 2. Pull the base image (if not already local)
/// 3. Clone it to a temporary working name
/// 4. Boot the VM with a shared directory
/// 5. Install the desktest VM agent, a11y helper, Python, PyAutoGUI
/// 6. Optionally install Node.js (for Electron testing)
/// 7. Save the result as the output image name
pub async fn run_init_macos(
    base_image: &str,
    output_image: &str,
    with_electron: bool,
) -> Result<(), AppError> {
    // 1. Verify prerequisites
    crate::preflight::check_tart()?;
    check_apple_silicon()?;
    check_sshpass_installed()?;

    let work_vm = format!("desktest-init-{}", std::process::id());

    println!("Preparing macOS golden image...");
    println!("  Base image:   {base_image}");
    println!("  Output image: {output_image}");
    if with_electron {
        println!("  Extras:       Node.js (--with-electron)");
    }
    println!();

    // 2. Pull base image if needed
    info!("Pulling base image {base_image}...");
    println!("Pulling base image (this may take a while on first run)...");
    crate::tart::run_tart_command(["pull", base_image]).await?;

    // 3. Clone to working VM
    info!("Cloning {base_image} → {work_vm}...");
    println!("Cloning base image...");
    crate::tart::run_tart_command(["clone", base_image, &work_vm]).await?;

    // From here on, clean up the working VM on error
    let result = provision_vm(&work_vm, with_electron).await;

    if let Err(e) = &result {
        eprintln!("Provisioning failed: {e}");
        eprintln!("Cleaning up working VM '{work_vm}'...");
        let _ = crate::tart::run_tart_command(["delete", &work_vm]).await;
        return result;
    }

    // 4. Rename to output image
    // Tart doesn't have a rename command, so we clone then delete the work VM
    info!("Saving as {output_image}...");
    println!("Saving golden image as '{output_image}'...");

    // Delete existing output image if it exists (ignore errors)
    let _ = crate::tart::run_tart_command(["delete", output_image]).await;

    // Clone the working VM to the output name; clean up work VM on failure
    // to avoid leaking 10+ GB of disk space.
    if let Err(e) = crate::tart::run_tart_command(["clone", &work_vm, output_image]).await {
        eprintln!("Failed to save golden image: {e}");
        eprintln!("Cleaning up working VM '{work_vm}'...");
        let _ = crate::tart::run_tart_command(["delete", &work_vm]).await;
        return Err(e);
    }

    // Best-effort cleanup — the golden image is already saved, so a delete
    // failure here should not make the command report failure.
    if let Err(e) = crate::tart::run_tart_command(["delete", &work_vm]).await {
        tracing::warn!(
            "Could not delete working VM '{work_vm}': {e} (image saved successfully, manual cleanup may be needed)"
        );
    }

    println!();
    println!("Golden image '{output_image}' is ready!");
    println!();
    println!("Next steps:");
    println!(
        "  1. Create a task JSON with '\"type\": \"macos_tart\"' and '\"base_image\": \"{output_image}\"'"
    );
    println!("  2. Run: desktest run your-task.json");
    println!();
    println!("Important: The VM must have Accessibility permissions granted for the");
    println!("a11y-helper and PyAutoGUI to work. If tests fail with permission errors,");
    println!("boot the image manually and grant permissions in System Settings → Privacy");
    println!("& Security → Accessibility.");

    Ok(())
}

/// Boot the working VM, install dependencies, shut it down.
async fn provision_vm(vm_name: &str, with_electron: bool) -> Result<(), AppError> {
    // Locate the macos/ directory relative to the desktest binary
    let macos_dir = find_macos_dir()?;

    // Create a temporary shared directory for provisioning
    let shared_dir = std::env::temp_dir().join(format!("{vm_name}-provision"));
    std::fs::create_dir_all(&shared_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create shared dir: {e}")))?;

    // Copy provisioning files into the shared directory
    let provision_dir = shared_dir.join("provision");
    std::fs::create_dir_all(&provision_dir)
        .map_err(|e| AppError::Infra(format!("Cannot create provision dir: {e}")))?;

    // Copy vm-agent files
    std::fs::copy(
        macos_dir.join("vm-agent.py"),
        provision_dir.join("vm-agent.py"),
    )
    .map_err(|e| AppError::Infra(format!("Cannot copy vm-agent.py: {e}")))?;

    std::fs::copy(
        macos_dir.join("vm-agent-install.sh"),
        provision_dir.join("vm-agent-install.sh"),
    )
    .map_err(|e| AppError::Infra(format!("Cannot copy vm-agent-install.sh: {e}")))?;

    // Copy a11y-helper build script
    let a11y_dir = macos_dir.join("a11y-helper");
    if a11y_dir.exists() {
        copy_dir_recursive(&a11y_dir, &provision_dir.join("a11y-helper"))?;
    }

    // Write the provisioning script
    let provision_script = generate_provision_script(with_electron);
    std::fs::write(provision_dir.join("provision.sh"), &provision_script)
        .map_err(|e| AppError::Infra(format!("Cannot write provision.sh: {e}")))?;

    // Boot VM with shared directory
    println!("Booting VM (this takes ~30-60 seconds)...");
    info!("Booting VM {vm_name} with shared directory...");

    let shared_mount = format!("desktest:{}", shared_dir.display());
    let mut child = tokio::process::Command::new("tart")
        .arg("run")
        .arg(format!("--dir={shared_mount}"))
        .arg("--no-graphics")
        .arg(vm_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Infra(format!("Failed to spawn `tart run`: {e}")))?;

    // Wait for the VM to boot (check for SSH availability via tart ip).
    // On failure, clean up the child process and shared dir before returning.
    println!("Waiting for VM to boot...");
    let vm_ip = match wait_for_vm_ip(vm_name, &mut child).await {
        Ok(ip) => ip,
        Err(e) => {
            let _ = crate::tart::run_tart_command(["stop", vm_name]).await;
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = std::fs::remove_dir_all(&shared_dir);
            return Err(e);
        }
    };
    info!("VM booted with IP: {vm_ip}");

    // Run the provisioning script via SSH
    // Tart base images typically have user/password: admin/admin
    println!("Running provisioning script...");
    let provision_result = run_provision_via_ssh(&vm_ip, &provision_dir).await;

    // Stop the VM and clean up regardless of provisioning result
    println!("Stopping VM...");
    let _ = crate::tart::run_tart_command(["stop", vm_name]).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    let _ = std::fs::remove_dir_all(&shared_dir);

    provision_result
}

/// Generate the shell script that runs inside the VM during provisioning.
fn generate_provision_script(with_electron: bool) -> String {
    let mut script = String::from(
        r#"#!/bin/bash
set -euo pipefail

echo "=== desktest macOS golden image provisioning ==="

SHARED="/Volumes/My Shared Files/desktest/provision"

# Install Homebrew if not present
if ! command -v brew &>/dev/null; then
    echo "Installing Homebrew..."
    NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    eval "$(/opt/homebrew/bin/brew shellenv)"
    echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.zprofile
    echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.bash_profile
fi

# Ensure Python 3 and pip
echo "Installing Python 3..."
brew install python3 || true

# Install PyAutoGUI (Quartz backend for macOS)
echo "Installing PyAutoGUI..."
pip3 install pyautogui pyobjc-framework-Quartz pyobjc-framework-ApplicationServices --break-system-packages || \
    pip3 install pyautogui pyobjc-framework-Quartz pyobjc-framework-ApplicationServices

# Install the VM agent
echo "Installing desktest VM agent..."
if [ -f "$SHARED/vm-agent-install.sh" ]; then
    bash "$SHARED/vm-agent-install.sh"
else
    echo "WARNING: vm-agent-install.sh not found in shared directory"
fi

# Build and install a11y-helper
echo "Building a11y-helper..."
if [ -d "$SHARED/a11y-helper" ]; then
    cd "$SHARED/a11y-helper"
    swift build -c release 2>&1 || {
        echo "WARNING: a11y-helper build failed (Swift may not be installed)"
        echo "Install Xcode Command Line Tools: xcode-select --install"
    }
    if [ -f ".build/release/a11y-helper" ]; then
        sudo cp .build/release/a11y-helper /usr/local/bin/a11y-helper
        sudo chmod 755 /usr/local/bin/a11y-helper
        echo "Installed a11y-helper to /usr/local/bin/"
    fi
    cd -
fi

"#,
    );

    if with_electron {
        script.push_str(
            r#"# Install Node.js for Electron testing
echo "Installing Node.js..."
brew install node@20 || true
# Write PATH to both zsh and bash profiles — deploy_app uses `bash -lc`
# which sources ~/.bash_profile, not ~/.zprofile.
NODE_PATH_LINE='export PATH="/opt/homebrew/opt/node@20/bin:$PATH"'
echo "$NODE_PATH_LINE" >> ~/.zprofile
echo "$NODE_PATH_LINE" >> ~/.bash_profile

"#,
        );
    }

    script.push_str(
        r#"echo "=== Provisioning complete ==="
"#,
    );

    script
}

/// Wait for the VM to get an IP address (indicates it has booted).
///
/// Also checks if the `tart run` child process has exited early — if so,
/// surfaces the real error immediately instead of waiting for the full timeout.
async fn wait_for_vm_ip(
    vm_name: &str,
    child: &mut tokio::process::Child,
) -> Result<String, AppError> {
    let timeout = std::time::Duration::from_secs(180);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Infra(
                "Timeout waiting for VM to boot and get an IP address".into(),
            ));
        }

        // Detect early tart run exit before the timeout fires
        if let Some(status) = child
            .try_wait()
            .map_err(|e| AppError::Infra(format!("Failed to check tart run status: {e}")))?
        {
            return Err(AppError::Infra(format!(
                "`tart run` exited before the VM got an IP address: {status}"
            )));
        }

        let output = tokio::process::Command::new("tart")
            .args(["ip", vm_name])
            .output()
            .await
            .map_err(|e| AppError::Infra(format!("Failed to run `tart ip`: {e}")))?;

        if output.status.success() {
            let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ip.is_empty() {
                return Ok(ip);
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

/// Run the provisioning script inside the VM via SSH.
///
/// Tart base images use admin/admin credentials by default.
/// Requires `sshpass` to be installed on the host (checked at startup).
async fn run_provision_via_ssh(vm_ip: &str, provision_dir: &Path) -> Result<(), AppError> {
    let ssh_target = format!("admin@{vm_ip}");

    // First, ensure we can SSH in (the VM may take a moment after getting an IP)
    let ssh_ready_timeout = std::time::Duration::from_secs(60);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > ssh_ready_timeout {
            return Err(AppError::Infra(
                "Timeout waiting for SSH to become available in VM".into(),
            ));
        }

        let check = tokio::process::Command::new("sshpass")
            .args([
                "-p",
                "admin",
                "ssh",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "ConnectTimeout=5",
                &ssh_target,
                "echo",
                "ready",
            ])
            .output()
            .await;

        match check {
            Ok(output) if output.status.success() => break,
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        }
    }

    // Run the provisioning script by piping it to bash via stdin
    let script_path = provision_dir.join("provision.sh");
    let script_content = std::fs::read_to_string(&script_path)
        .map_err(|e| AppError::Infra(format!("Cannot read provision script: {e}")))?;

    let mut child = tokio::process::Command::new("sshpass")
        .args([
            "-p",
            "admin",
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            &ssh_target,
            "bash",
            "-s",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| AppError::Infra(format!("Failed to SSH into VM: {e}")))?;

    // Write the script to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(script_content.as_bytes())
            .await
            .map_err(|e| {
                AppError::Infra(format!(
                    "Failed to write provisioning script to SSH stdin: {e}"
                ))
            })?;
        // Drop stdin to close it and signal EOF
    }

    let status = child
        .wait()
        .await
        .map_err(|e| AppError::Infra(format!("SSH provisioning failed: {e}")))?;

    if !status.success() {
        return Err(AppError::Infra(format!(
            "Provisioning script failed with exit code: {}",
            status.code().unwrap_or(-1)
        )));
    }

    Ok(())
}

/// Verify that `sshpass` is installed (needed for VM provisioning).
///
/// Checks that the binary can be spawned rather than relying on a specific
/// flag's exit code (`sshpass -V` returns non-zero on some versions).
fn check_sshpass_installed() -> Result<(), AppError> {
    // `sshpass` with no args exits 1 but prints usage — a successful spawn
    // means the binary exists. We only care about spawn failure (not found).
    match std::process::Command::new("sshpass")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(_) => Ok(()), // Binary exists (any exit code is fine)
        Err(_) => Err(AppError::Config(
            "sshpass is not installed. It is required to provision the macOS VM.\n\
             Install it with: brew install hudochenkov/sshpass/sshpass"
                .into(),
        )),
    }
}

/// Verify we're running on Apple Silicon.
fn check_apple_silicon() -> Result<(), AppError> {
    let arch = std::env::consts::ARCH;
    if arch != "aarch64" {
        return Err(AppError::Config(format!(
            "Tart requires Apple Silicon (aarch64), but this machine is {arch}.\n\
             macOS VM testing is only supported on Apple Silicon Macs (M1+)."
        )));
    }
    Ok(())
}

/// Find the `macos/` directory containing VM agent and a11y helper sources.
///
/// Searches relative to the current executable, then relative to CWD.
fn find_macos_dir() -> Result<std::path::PathBuf, AppError> {
    // Try relative to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            // In dev: target/debug/desktest → ../../macos
            let dev_path = parent.join("../../macos");
            if dev_path.join("vm-agent.py").exists() {
                return Ok(std::fs::canonicalize(&dev_path).unwrap_or(dev_path));
            }
            // In release: alongside the binary
            let release_path = parent.join("macos");
            if release_path.join("vm-agent.py").exists() {
                return Ok(release_path);
            }
        }
    }

    // Try relative to CWD
    let cwd_path = std::path::PathBuf::from("macos");
    if cwd_path.join("vm-agent.py").exists() {
        return Ok(std::fs::canonicalize(&cwd_path).unwrap_or(cwd_path));
    }

    Err(AppError::Config(
        "Cannot find the 'macos/' directory containing VM agent and a11y helper.\n\
         Run this command from the desktest repository root, or ensure the 'macos/' \
         directory is alongside the desktest binary."
            .into(),
    ))
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dest)
        .map_err(|e| AppError::Infra(format!("Cannot create {}: {e}", dest.display())))?;

    for entry in std::fs::read_dir(src)
        .map_err(|e| AppError::Infra(format!("Cannot read {}: {e}", src.display())))?
    {
        let entry = entry.map_err(|e| AppError::Infra(format!("Cannot read dir entry: {e}")))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        // Skip .build directories (Swift build artifacts)
        if src_path.is_dir() {
            let name = entry.file_name();
            if name == ".build" || name == ".swiftpm" {
                continue;
            }
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path).map_err(|e| {
                AppError::Infra(format!(
                    "Cannot copy {} → {}: {e}",
                    src_path.display(),
                    dest_path.display()
                ))
            })?;
        }
    }
    Ok(())
}
