#![allow(dead_code)]

use std::path::Path;

use tracing::info;

use crate::error::AppError;
use crate::session::Session;
use crate::task::AppConfig;

use super::WindowsVmSession;

/// Deploy an application into the Windows VM and return the guest-side path.
///
/// If `app_path` is set and points to a host file, copies it into `C:\Temp\`
/// in the guest. If `launch_cmd` is set, no deployment is needed — the command
/// will be executed directly.
pub async fn deploy_app(session: &WindowsVmSession, app: &AppConfig) -> Result<String, AppError> {
    match app {
        AppConfig::WindowsVm {
            app_path,
            installer_cmd,
            ..
        } => {
            let deployed_path = if let Some(host_path) = app_path {
                let src = Path::new(host_path);
                if src.exists() {
                    info!("Deploying '{}' into Windows VM...", host_path);
                    session.copy_into(src, "C:\\Temp").await?;

                    let filename = src
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let guest_path = format!("C:\\Temp\\{filename}");
                    info!("Deployed to {guest_path}");
                    guest_path
                } else {
                    // Assume it's already a path inside the VM
                    host_path.clone()
                }
            } else {
                String::new()
            };

            // Run installer if specified
            if let Some(installer) = installer_cmd {
                info!("Running installer: {installer}");
                session
                    .exec(&["powershell", "-Command", installer])
                    .await?;
            }

            Ok(deployed_path)
        }
        _ => Err(AppError::Config(
            "deploy_app called with non-WindowsVm app config".into(),
        )),
    }
}

/// Launch the application inside the Windows VM.
///
/// Priority: `launch_cmd` > `deployed_path`.
pub async fn launch_app(
    session: &WindowsVmSession,
    app: &AppConfig,
    deployed_path: &str,
) -> Result<(), AppError> {
    match app {
        AppConfig::WindowsVm { launch_cmd, .. } => {
            if let Some(cmd) = launch_cmd {
                info!("Launching via launch_cmd: {cmd}");
                session
                    .exec_detached_with_log(
                        &["powershell", "-Command", cmd],
                        "C:\\Temp\\app.log",
                    )
                    .await?;
            } else if !deployed_path.is_empty() {
                info!("Launching: {deployed_path}");
                session
                    .exec_detached_with_log(&[deployed_path], "C:\\Temp\\app.log")
                    .await?;
            } else {
                return Err(AppError::Config(
                    "WindowsVm task has no launch_cmd and no app_path to launch".into(),
                ));
            }

            Ok(())
        }
        _ => Err(AppError::Config(
            "launch_app called with non-WindowsVm app config".into(),
        )),
    }
}
