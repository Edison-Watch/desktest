use tracing::info;

use crate::config::Config;
use crate::error::AppError;
use super::DockerSession;

impl DockerSession {
    /// Deploy the app under test into the container. Returns the path to run inside the container.
    ///
    /// For `DockerImage` app type, nothing is deployed — the app is already in the custom image.
    /// The returned path is empty string (caller should use `entrypoint_cmd` to launch).
    pub async fn deploy_app(&self, config: &Config) -> Result<String, AppError> {
        match config.app_type {
            crate::config::AppType::Appimage => {
                let app_path = config
                    .app_path
                    .as_ref()
                    .ok_or_else(|| AppError::Config("app_path required for appimage".into()))?;

                let filename = app_path
                    .file_name()
                    .ok_or_else(|| AppError::Infra("No filename in app_path".into()))?
                    .to_string_lossy();

                let container_path = format!("/home/tester/{filename}");

                self.copy_into(app_path, "/home/tester/").await?;
                self.exec(&["chmod", "+x", &container_path]).await?;

                info!("Deployed AppImage to {container_path}");
                Ok(container_path)
            }
            crate::config::AppType::Folder => {
                let app_dir = config
                    .app_dir
                    .as_ref()
                    .ok_or_else(|| AppError::Config("app_dir required for folder app".into()))?;

                let entrypoint = config
                    .entrypoint
                    .as_ref()
                    .ok_or_else(|| AppError::Config("entrypoint required for folder app".into()))?;

                self.copy_into(app_dir, "/home/tester/").await?;

                let dir_name = app_dir
                    .file_name()
                    .ok_or_else(|| AppError::Infra("No dirname in app_dir".into()))?
                    .to_string_lossy();

                let entrypoint_path = format!("/home/tester/{dir_name}/{entrypoint}");
                self.exec(&["chmod", "+x", &entrypoint_path]).await?;

                info!("Deployed folder app, entrypoint: {entrypoint_path}");
                Ok(entrypoint_path)
            }
            crate::config::AppType::DockerImage | crate::config::AppType::VncAttach => {
                // Nothing to deploy — the app is already part of the custom Docker image
                // or managed externally (attach mode).
                info!("No deployment needed for this app type");
                Ok(String::new())
            }
        }
    }

    /// Launch the app inside the container (non-blocking).
    /// App stdout/stderr is captured to /tmp/app.log for debugging.
    /// AppImages are launched with --appimage-extract-and-run to avoid FUSE issues in containers.
    /// AppImages and Electron apps get --no-sandbox (Chromium's sandbox doesn't work in containers).
    /// Electron apps additionally get --disable-gpu and --force-renderer-accessibility.
    /// For folder deploys, these flags are passed as positional args — scripts that forward
    /// "$@" to the binary will receive them; others can safely ignore them.
    pub async fn launch_app(&self, app_path: &str, is_appimage: bool, is_electron: bool) -> Result<(), AppError> {
        let mut args: Vec<&str> = vec![app_path];
        if is_appimage {
            args.push("--appimage-extract-and-run");
        }
        // Chromium/CEF sandbox doesn't work in containers
        if is_appimage || is_electron {
            args.push("--no-sandbox");
        }
        if is_electron {
            args.push("--disable-gpu");
            args.push("--force-renderer-accessibility");
        }

        self.exec_detached_with_log(&args, "/tmp/app.log").await?;
        info!("Launched app: {app_path}");
        Ok(())
    }
}
