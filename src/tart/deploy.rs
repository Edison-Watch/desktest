use std::path::Path;

use tracing::info;

use super::TartSession;
use crate::error::AppError;
use crate::session::Session;
use crate::task::AppConfig;

impl TartSession {
    /// Deploy a macOS app into the Tart VM.
    ///
    /// - If `app_path` points to a host-side `.app` bundle or directory, it is
    ///   copied into the VM via the shared directory.
    /// - Built-in macOS apps (identified by bundle_id only) don't need deployment.
    /// - Returns a display string describing what was deployed (for logging).
    pub async fn deploy_app(&self, app: &AppConfig) -> Result<String, AppError> {
        let (app_path, is_electron) = match app {
            AppConfig::MacosTart {
                app_path, electron, ..
            } => (app_path.as_deref(), *electron),
            _ => return Ok(String::new()),
        };

        // If there's a host-side app_path, copy it into the VM
        if let Some(host_path) = app_path {
            let src = Path::new(host_path);
            if src.exists() {
                let dest = "/Users/admin/Desktop/";
                info!("Deploying app from {host_path} into VM...");
                self.copy_into(src, dest).await?;

                let filename = src
                    .file_name()
                    .ok_or_else(|| AppError::Infra("No filename in app_path".into()))?
                    .to_string_lossy();
                let vm_path = format!("{dest}{filename}");

                // Make non-.app entries executable (scripts, binaries)
                if !host_path.ends_with(".app") {
                    self.exec(&["chmod", "+x", &vm_path]).await?;
                }

                // For Electron apps, install npm dependencies if a package.json was deployed
                if is_electron && src.is_dir() && src.join("package.json").exists() {
                    info!("Installing npm dependencies in {vm_path}...");
                    self.exec(&[
                        "bash",
                        "-lc",
                        &format!(
                            "cd {} && npm install",
                            shell_escape::escape(vm_path.as_str().into())
                        ),
                    ])
                    .await?;
                }

                info!("Deployed to {vm_path}");
                return Ok(vm_path);
            }
            // If the path doesn't exist on the host, assume it's a VM-local path
            info!("app_path '{host_path}' not found on host — assuming VM-local path");
        }

        Ok(String::new())
    }

    /// Launch the macOS app inside the Tart VM.
    ///
    /// Priority: `launch_cmd` > `bundle_id` > `deployed_path` (VM-local) > `app_path` (config).
    /// Electron apps get `--force-renderer-accessibility`.
    ///
    /// `deployed_path` is the VM-local path returned by `deploy_app()`. When an app
    /// was copied from the host into the VM, this differs from the config's `app_path`
    /// (which is the host-side path). If non-empty, it takes precedence.
    pub async fn launch_app(&self, app: &AppConfig, deployed_path: &str) -> Result<(), AppError> {
        let (bundle_id, config_app_path, launch_cmd, electron) = match app {
            AppConfig::MacosTart {
                bundle_id,
                app_path,
                launch_cmd,
                electron,
                ..
            } => (
                bundle_id.as_deref(),
                app_path.as_deref(),
                launch_cmd.as_deref(),
                *electron,
            ),
            _ => return Ok(()),
        };

        // Prefer the deployed VM-local path over the config's host-side path
        let app_path = if deployed_path.is_empty() {
            config_app_path
        } else {
            Some(deployed_path)
        };

        // Electron apps in Tart VMs need --in-process-gpu to work around a
        // Chromium compositor bug where the window is logically created but not
        // painted to the screen buffer. Running the GPU compositor in-process
        // avoids the IPC path that breaks under Apple Virtualization.framework.
        if let Some(cmd) = launch_cmd {
            // Arbitrary launch command — run directly
            info!("Launching app via launch_cmd: {cmd}");
            self.exec_detached_with_log(&["bash", "-lc", cmd], "/tmp/app.log")
                .await?;
        } else if let Some(bid) = bundle_id {
            // Launch by bundle identifier
            let mut cmd = format!("open -b {}", shell_escape::escape(bid.into()));
            if electron {
                cmd.push_str(
                    " --args --no-sandbox --in-process-gpu --force-renderer-accessibility",
                );
            }
            info!("Launching app via bundle_id: {bid}");
            self.exec_detached_with_log(&["bash", "-lc", &cmd], "/tmp/app.log")
                .await?;
        } else if let Some(path) = app_path {
            // Launch by path — determine if it's a .app bundle or an executable
            if path.ends_with(".app") {
                let mut cmd = format!("open {}", shell_escape::escape(path.into()));
                if electron {
                    cmd.push_str(
                        " --args --no-sandbox --in-process-gpu --force-renderer-accessibility",
                    );
                }
                info!("Launching app bundle: {path}");
                self.exec_detached_with_log(&["bash", "-lc", &cmd], "/tmp/app.log")
                    .await?;
            } else if electron && std::path::Path::new(path).extension().is_none() {
                // Electron directory deploy — launch via npx inside the directory
                let escaped = shell_escape::escape(path.into());
                let cmd = format!(
                    "cd {escaped} && npx electron . \
                     --no-sandbox --in-process-gpu --force-renderer-accessibility"
                );
                info!("Launching Electron app from directory: {path}");
                self.exec_detached_with_log(&["bash", "-lc", &cmd], "/tmp/app.log")
                    .await?;
            } else {
                // Direct executable
                let mut args: Vec<&str> = vec![path];
                if electron {
                    args.push("--no-sandbox");
                    args.push("--in-process-gpu");
                    args.push("--force-renderer-accessibility");
                }
                info!("Launching executable: {path}");
                self.exec_detached_with_log(&args, "/tmp/app.log").await?;
            }
        }

        Ok(())
    }
}
