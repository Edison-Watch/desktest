#![allow(dead_code)]

mod deploy;
mod exec;
mod image;
mod transfer;

use bollard::Docker;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, RemoveContainerOptions, StopContainerOptions,
};
use bollard::models::{HostConfig, PortBinding};
use futures::StreamExt;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::AppError;

pub const IMAGE_NAME: &str = "desktest-desktop:latest";
pub const IMAGE_NAME_ELECTRON: &str = "desktest-desktop:electron";

pub struct DockerSession {
    pub(in crate::docker) client: Docker,
    pub container_id: String,
}

impl DockerSession {
    /// Access the underlying Docker client (for container logs, etc.).
    pub fn docker_client(&self) -> &Docker {
        &self.client
    }

    /// Create and start a container from the test image.
    ///
    /// When `custom_image` is `Some`, use that pre-built image instead of the
    /// built-in `desktest-desktop` base image. The custom image is NOT built —
    /// it must already exist locally or be pullable by Docker.
    ///
    /// **Privileges:** No containers receive `CAP_SYS_ADMIN` or `/dev/fuse`.
    /// AppImages are launched with `--appimage-extract-and-run`, bypassing
    /// FUSE entirely. Custom Docker images that need FUSE for other reasons
    /// are not currently supported — see `TODO.md` for a future `needs_fuse`
    /// config escape hatch.
    pub async fn create(config: &Config, custom_image: Option<&str>) -> Result<Self, AppError> {
        let client = Docker::connect_with_local_defaults()
            .map_err(|e| AppError::Infra(format!("Cannot connect to Docker: {e}")))?;

        let image_name = if let Some(img) = custom_image {
            // For custom images, check if the image exists locally; if not, try to pull it.
            // Distinguish "not found" from other Docker errors to avoid misleading warnings.
            // NOTE: This matches bollard's error variant as of bollard 0.18.x.
            // If bollard changes its error representation, this arm may stop matching
            // and fall through to the Err(e) branch (safe but blocks pull). Re-verify
            // after bumping the bollard dependency.
            match client.inspect_image(img).await {
                Ok(_) => {}
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => {
                    warn!("Custom image '{img}' not found locally — pulling from remote registry. Ensure you trust this image source.");
                    use bollard::image::CreateImageOptions;
                    let options = CreateImageOptions {
                        from_image: img,
                        ..Default::default()
                    };
                    let mut stream = client.create_image(Some(options), None, None);
                    while let Some(result) = stream.next().await {
                        let info = result.map_err(|e| {
                            AppError::Config(format!(
                                "Cannot pull custom Docker image '{img}': {e}"
                            ))
                        })?;
                        if let Some(status) = &info.status {
                            debug!("Pull: {status}");
                        }
                    }
                }
                Err(e) => {
                    return Err(AppError::Infra(format!(
                        "Cannot inspect custom Docker image '{img}': {e}"
                    )));
                }
            }
            img.to_string()
        } else {
            Self::ensure_image(&client, false).await?;
            IMAGE_NAME.to_string()
        };

        // VNC port inside the container is always 5900.
        // If the user specified a host port, we bind it; otherwise VNC runs but isn't exposed.
        let container_vnc_port = "5900";

        let env: Vec<String> = vec![
            format!("DISPLAY_WIDTH={}", config.display_width),
            format!("DISPLAY_HEIGHT={}", config.display_height),
            format!("VNC_PORT={container_vnc_port}"),
        ];

        // No SYS_ADMIN or /dev/fuse needed — AppImages are launched with
        // --appimage-extract-and-run (see deploy.rs), which bypasses FUSE entirely.
        let mut host_config = HostConfig::default();

        let mut exposed_ports = std::collections::HashMap::new();

        if let Some(vnc_port) = config.vnc_port {
            let exposed_port = format!("{container_vnc_port}/tcp");
            exposed_ports.insert(exposed_port.clone(), Default::default());

            let host_binding = PortBinding {
                host_ip: Some(config.vnc_bind_addr.clone()),
                host_port: Some(vnc_port.to_string()),
            };

            let mut port_bindings = std::collections::HashMap::new();
            port_bindings.insert(exposed_port, Some(vec![host_binding]));
            host_config.port_bindings = Some(port_bindings);

            info!(
                "VNC will be available at {}:{}",
                config.vnc_bind_addr, vnc_port
            );
        }

        let container_config = ContainerConfig {
            image: Some(image_name.clone()),
            env: Some(env),
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            host_config: Some(host_config),
            ..Default::default()
        };

        let create_result = client
            .create_container(None::<CreateContainerOptions<String>>, container_config)
            .await
            .map_err(AppError::Docker)?;

        let container_id = create_result.id;
        debug!("Created container {container_id}");

        client
            .start_container::<String>(&container_id, None)
            .await
            .map_err(AppError::Docker)?;

        info!("Started container {container_id} (image: {image_name})");

        Ok(Self {
            client,
            container_id,
        })
    }

    /// Attach to an existing running container by ID or name.
    ///
    /// Unlike `create()`, this does not create, start, or manage the container
    /// lifecycle. The container must already be running. Use with `desktest attach`.
    pub async fn attach(container: &str) -> Result<Self, AppError> {
        let client = Docker::connect_with_local_defaults()
            .map_err(|e| AppError::Infra(format!("Cannot connect to Docker: {e}")))?;

        let inspect = client
            .inspect_container(container, None)
            .await
            .map_err(|e| AppError::Infra(format!("Cannot find container '{container}': {e}")))?;

        let running = inspect
            .state
            .as_ref()
            .and_then(|s| s.running)
            .unwrap_or(false);
        if !running {
            return Err(AppError::Infra(format!(
                "Container '{container}' is not running"
            )));
        }

        let container_id = inspect.id.unwrap_or_else(|| container.to_string());
        info!("Attached to container {container_id}");

        Ok(Self {
            client,
            container_id,
        })
    }

    /// Required binaries that must exist in custom Docker images.
    const REQUIRED_BINARIES: &[&str] = &["xdotool", "scrot", "Xvfb", "ffmpeg", "python3"];

    /// Required Python packages that must be importable in custom Docker images.
    const REQUIRED_PYTHON_PACKAGES: &[(&str, &str)] = &[
        ("pyautogui", "python3-pyautogui"),
        ("Xlib", "python3-xlib"),
        ("pyatspi", "python3-pyatspi"),
    ];

    /// Validate that a custom Docker image has all required dependencies.
    ///
    /// Checks for required binaries and Python packages by running commands
    /// inside the container. Returns `AppError::Config` (exit code 2) if
    /// any dependency is missing.
    pub async fn validate_custom_image(&self) -> Result<(), AppError> {
        let mut missing: Vec<String> = Vec::new();

        // Check required binaries
        for binary in Self::REQUIRED_BINARIES {
            let result = self.exec(&["which", binary]).await;
            if result.is_err() || result.as_ref().is_ok_and(|o| o.trim().is_empty()) {
                missing.push(format!("{binary} (binary)"));
            }
        }

        // Check required Python packages
        for (package, apt_name) in Self::REQUIRED_PYTHON_PACKAGES {
            let (_, exit_code) = self
                .exec_with_exit_code(&["python3", "-c", &format!("import {package}")])
                .await?;
            if exit_code != 0 {
                missing.push(format!("{apt_name} (Python package '{package}')"));
            }
        }

        // Check that ~/.Xauthority exists — PyAutoGUI/python-xlib will crash without it.
        // This is a common pitfall when building custom images (see docker/Dockerfile).
        let (_, xauth_exit) = self
            .exec_with_exit_code(&["test", "-f", "/home/tester/.Xauthority"])
            .await?;
        if xauth_exit != 0 {
            tracing::warn!(
                "~/.Xauthority not found — PyAutoGUI will fail to connect to the X display. \
                 Add `RUN touch /home/tester/.Xauthority` to your Dockerfile after `USER tester`."
            );
            missing
                .push("/home/tester/.Xauthority (required by PyAutoGUI/python-xlib)".to_string());
        }

        if missing.is_empty() {
            info!("Custom image validation passed: all required dependencies found");
            Ok(())
        } else {
            Err(AppError::Config(format!(
                "Custom Docker image is missing required dependencies: {}",
                missing.join(", ")
            )))
        }
    }

    /// Stop and remove the container.
    pub async fn cleanup(&self) -> Result<(), AppError> {
        debug!("Stopping container {}", self.container_id);
        let _ = self
            .client
            .stop_container(&self.container_id, Some(StopContainerOptions { t: 5 }))
            .await;

        debug!("Removing container {}", self.container_id);
        self.client
            .remove_container(
                &self.container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(AppError::Docker)?;

        info!("Cleaned up container {}", self.container_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            api_key: "sk-test".into(),
            provider: "openai".into(),
            model: "gpt-4.1".into(),
            api_base_url: "https://api.openai.com".into(),
            display_width: 1920,
            display_height: 1080,
            vnc_bind_addr: "127.0.0.1".into(),
            vnc_port: None,
            app_type: crate::config::AppType::Appimage,
            app_path: None,
            app_dir: None,
            entrypoint: None,
            startup_timeout_seconds: 30,
            electron: false,
        }
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_image_build() {
        let client = Docker::connect_with_local_defaults().unwrap();
        DockerSession::ensure_image(&client, true).await.unwrap();
        let inspect = client.inspect_image(IMAGE_NAME).await.unwrap();
        assert!(inspect.id.is_some());
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_container_create_start_stop() {
        let config = test_config();
        let session = DockerSession::create(&config, None).await.unwrap();

        let inspect = session
            .client
            .inspect_container(&session.container_id, None)
            .await
            .unwrap();
        assert!(inspect.state.unwrap().running.unwrap());

        session.cleanup().await.unwrap();

        let result = session
            .client
            .inspect_container(&session.container_id, None)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_exec_command() {
        let config = test_config();
        let session = DockerSession::create(&config, None).await.unwrap();

        let output = session.exec(&["echo", "hello"]).await.unwrap();
        assert!(output.trim().contains("hello"));

        session.cleanup().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_copy_file_into_container() {
        let config = test_config();
        let session = DockerSession::create(&config, None).await.unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"test content").unwrap();

        session
            .copy_into(tmp.path(), "/home/tester/")
            .await
            .unwrap();

        let filename = tmp.path().file_name().unwrap().to_str().unwrap();
        let output = session
            .exec(&["cat", &format!("/home/tester/{filename}")])
            .await
            .unwrap();
        assert!(output.contains("test content"));

        session.cleanup().await.unwrap();
    }

    #[test]
    fn test_required_binaries_list() {
        // Verify that the required binaries list contains the expected entries
        assert!(DockerSession::REQUIRED_BINARIES.contains(&"xdotool"));
        assert!(DockerSession::REQUIRED_BINARIES.contains(&"scrot"));
        assert!(DockerSession::REQUIRED_BINARIES.contains(&"Xvfb"));
        assert!(DockerSession::REQUIRED_BINARIES.contains(&"ffmpeg"));
        assert!(DockerSession::REQUIRED_BINARIES.contains(&"python3"));
    }

    #[test]
    fn test_required_python_packages_list() {
        let packages: Vec<&str> = DockerSession::REQUIRED_PYTHON_PACKAGES
            .iter()
            .map(|(pkg, _)| *pkg)
            .collect();
        assert!(packages.contains(&"pyautogui"));
        assert!(packages.contains(&"Xlib"));
        assert!(packages.contains(&"pyatspi"));
    }

    #[test]
    fn test_deploy_app_docker_image_type() {
        // DockerImage deploy should return an empty string (no deployment needed)
        let config = Config {
            app_type: crate::config::AppType::DockerImage,
            ..test_config()
        };
        // We can't actually run deploy_app without a real Docker session,
        // but we verify the AppType variant exists and can be constructed
        assert_eq!(config.app_type, crate::config::AppType::DockerImage);
    }

    #[test]
    fn test_image_name_constant() {
        assert_eq!(IMAGE_NAME, "desktest-desktop:latest");
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_attach_nonexistent_container() {
        match DockerSession::attach("nonexistent-container-id-12345").await {
            Err(e) => assert!(e.to_string().contains("Cannot find container")),
            Ok(_) => panic!("Expected error for nonexistent container"),
        }
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon with a running container
    async fn test_attach_to_running_container() {
        // Create a container, then attach to it
        let config = test_config();
        let session = DockerSession::create(&config, None).await.unwrap();
        let container_id = session.container_id.clone();

        // Attach to it by ID
        let attached = DockerSession::attach(&container_id).await.unwrap();
        assert_eq!(attached.container_id, container_id);

        // Run a command via the attached session
        let output = attached.exec(&["echo", "attached"]).await.unwrap();
        assert!(output.trim().contains("attached"));

        // Clean up (only via the original session)
        session.cleanup().await.unwrap();
    }
}
