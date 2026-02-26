#![allow(dead_code)]

use std::path::Path;

use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, RemoveContainerOptions,
    StopContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::BuildImageOptions;
use bollard::models::{HostConfig, PortBinding};
use bollard::Docker;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use crate::config::Config;
use crate::error::AppError;

pub const IMAGE_NAME: &str = "llm-desktop-tester:latest";

pub struct DockerSession {
    client: Docker,
    pub container_id: String,
}

impl DockerSession {
    /// Access the underlying Docker client (for container logs, etc.).
    pub fn docker_client(&self) -> &Docker {
        &self.client
    }

    /// Build the Docker image from the docker/ directory if it doesn't already exist.
    pub async fn ensure_image(client: &Docker, force_rebuild: bool) -> Result<(), AppError> {
        if !force_rebuild {
            if client.inspect_image(IMAGE_NAME).await.is_ok() {
                debug!("Image {IMAGE_NAME} already exists, skipping build");
                return Ok(());
            }
        }

        info!("Building Docker image {IMAGE_NAME}...");

        let docker_dir = Self::find_docker_context()?;
        let tar_bytes = Self::create_tar_context(&docker_dir)?;

        let options = BuildImageOptions {
            t: IMAGE_NAME.to_string(),
            rm: true,
            ..Default::default()
        };

        let mut stream = client.build_image(options, None, Some(tar_bytes.into()));

        while let Some(result) = stream.next().await {
            let info = result.map_err(AppError::Docker)?;
            if let Some(stream_text) = &info.stream {
                debug!("{}", stream_text.trim_end());
            }
            if let Some(err) = &info.error {
                return Err(AppError::Infra(format!("Docker build error: {err}")));
            }
        }

        info!("Docker image {IMAGE_NAME} built successfully");
        Ok(())
    }

    fn find_docker_context() -> Result<std::path::PathBuf, AppError> {
        let candidates = [
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("docker"))),
            Some(std::path::PathBuf::from("docker")),
            Some(std::path::PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/docker"
            ))),
        ];

        for candidate in candidates.into_iter().flatten() {
            if candidate.join("Dockerfile").exists() {
                return Ok(candidate);
            }
        }

        Err(AppError::Infra(
            "Cannot find docker/ directory with Dockerfile".into(),
        ))
    }

    fn create_tar_context(docker_dir: &Path) -> Result<Vec<u8>, AppError> {
        let mut archive = tar::Builder::new(Vec::new());

        for entry in std::fs::read_dir(docker_dir)
            .map_err(|e| AppError::Infra(format!("Cannot read docker dir: {e}")))?
        {
            let entry = entry.map_err(|e| AppError::Infra(format!("Dir entry error: {e}")))?;
            let path = entry.path();
            let name = entry.file_name();

            if path.is_file() {
                archive
                    .append_path_with_name(&path, &name)
                    .map_err(|e| AppError::Infra(format!("Tar error: {e}")))?;
            }
        }

        archive
            .into_inner()
            .map_err(|e| AppError::Infra(format!("Tar finalize error: {e}")))
    }

    /// Create and start a container from the test image.
    pub async fn create(config: &Config) -> Result<Self, AppError> {
        let client =
            Docker::connect_with_local_defaults().map_err(|e| AppError::Infra(format!("Cannot connect to Docker: {e}")))?;

        Self::ensure_image(&client, false).await?;

        // VNC port inside the container is always 5900.
        // If the user specified a host port, we bind it; otherwise VNC runs but isn't exposed.
        let container_vnc_port = "5900";

        let env: Vec<String> = vec![
            format!("DISPLAY_WIDTH={}", config.display_width),
            format!("DISPLAY_HEIGHT={}", config.display_height),
            format!("VNC_PORT={container_vnc_port}"),
        ];

        let mut host_config = HostConfig {
            cap_add: Some(vec!["SYS_ADMIN".into()]),
            devices: Some(vec![bollard::models::DeviceMapping {
                path_on_host: Some("/dev/fuse".into()),
                path_in_container: Some("/dev/fuse".into()),
                cgroup_permissions: Some("rwm".into()),
            }]),
            ..Default::default()
        };

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

            info!("VNC will be available at {}:{}", config.vnc_bind_addr, vnc_port);
        }

        let container_config = ContainerConfig {
            image: Some(IMAGE_NAME.to_string()),
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

        info!("Started container {container_id}");

        Ok(Self {
            client,
            container_id,
        })
    }

    /// Execute a command inside the container and return stdout.
    pub async fn exec(&self, cmd: &[&str]) -> Result<String, AppError> {
        let exec = self
            .client
            .create_exec(
                &self.container_id,
                CreateExecOptions {
                    cmd: Some(cmd.to_vec()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    env: Some(vec!["DISPLAY=:99"]),
                    ..Default::default()
                },
            )
            .await
            .map_err(AppError::Docker)?;

        let start_result = self
            .client
            .start_exec(&exec.id, None)
            .await
            .map_err(AppError::Docker)?;

        let mut output = String::new();
        if let StartExecResults::Attached { output: mut stream, .. } = start_result {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(AppError::Docker)?;
                output.push_str(&chunk.to_string());
            }
        }

        Ok(output)
    }

    /// Execute a command inside the container with data piped to stdin,
    /// and return stdout.
    pub async fn exec_with_stdin(&self, cmd: &[&str], stdin_data: &[u8]) -> Result<String, AppError> {
        let exec = self
            .client
            .create_exec(
                &self.container_id,
                CreateExecOptions {
                    cmd: Some(cmd.to_vec()),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    env: Some(vec!["DISPLAY=:99"]),
                    ..Default::default()
                },
            )
            .await
            .map_err(AppError::Docker)?;

        let start_result = self
            .client
            .start_exec(&exec.id, None)
            .await
            .map_err(AppError::Docker)?;

        let mut output = String::new();
        if let StartExecResults::Attached { output: mut stream, input: mut writer } = start_result {
            // Write stdin data and close the writer
            writer
                .write_all(stdin_data)
                .await
                .map_err(|e| AppError::Infra(format!("Failed to write stdin: {e}")))?;
            writer
                .shutdown()
                .await
                .map_err(|e| AppError::Infra(format!("Failed to close stdin: {e}")))?;
            drop(writer);

            // Read all output
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(AppError::Docker)?;
                output.push_str(&chunk.to_string());
            }
        }

        Ok(output)
    }

    /// Execute a command in the background (detached) inside the container.
    /// Output is redirected to the specified log file (default: /dev/null).
    pub async fn exec_detached(&self, cmd: &[&str]) -> Result<(), AppError> {
        self.exec_detached_with_log(cmd, "/dev/null").await
    }

    /// Execute a command in the background, redirecting stdout/stderr to a log file.
    pub async fn exec_detached_with_log(&self, cmd: &[&str], log_path: &str) -> Result<(), AppError> {
        // bollard doesn't have a `detach` option on CreateExecOptions,
        // so we launch a background process via bash.
        let escaped_cmd = cmd
            .iter()
            .map(|s| shell_escape::escape((*s).into()))
            .collect::<Vec<_>>()
            .join(" ");

        self.exec(&[
            "bash",
            "-c",
            &format!("nohup {escaped_cmd} > {log_path} 2>&1 &"),
        ])
        .await?;

        Ok(())
    }

    /// Copy a file or directory from the host into the container.
    pub async fn copy_into(&self, src: &Path, dest_path: &str) -> Result<(), AppError> {
        let mut archive = tar::Builder::new(Vec::new());

        if src.is_file() {
            let name = src
                .file_name()
                .ok_or_else(|| AppError::Infra("No filename".into()))?;
            archive
                .append_path_with_name(src, name)
                .map_err(|e| AppError::Infra(format!("Tar error: {e}")))?;
        } else if src.is_dir() {
            archive
                .append_dir_all(
                    src.file_name()
                        .ok_or_else(|| AppError::Infra("No dirname".into()))?,
                    src,
                )
                .map_err(|e| AppError::Infra(format!("Tar error: {e}")))?;
        } else {
            return Err(AppError::Infra(format!(
                "Source path does not exist: {}",
                src.display()
            )));
        }

        let tar_bytes = archive
            .into_inner()
            .map_err(|e| AppError::Infra(format!("Tar finalize error: {e}")))?;

        self.client
            .upload_to_container(
                &self.container_id,
                Some(bollard::container::UploadToContainerOptions {
                    path: dest_path.to_string(),
                    ..Default::default()
                }),
                tar_bytes.into(),
            )
            .await
            .map_err(AppError::Docker)?;

        Ok(())
    }

    /// Copy a file or directory from the container to a local path.
    ///
    /// When copying a single file, `local_path` is the destination file.
    /// When copying a directory, `local_path` is the destination directory
    /// (the container directory's contents are extracted into it).
    pub async fn copy_from(&self, container_path: &str, local_path: &Path) -> Result<(), AppError> {
        let stream = self
            .client
            .download_from_container(
                &self.container_id,
                Some(bollard::container::DownloadFromContainerOptions {
                    path: container_path.to_string(),
                }),
            );

        let mut tar_bytes: Vec<u8> = Vec::new();
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(AppError::Docker)?;
            tar_bytes.extend_from_slice(&chunk);
        }

        let mut archive = tar::Archive::new(&tar_bytes[..]);
        let entries = archive
            .entries()
            .map_err(|e| AppError::Infra(format!("Tar read error: {e}")))?;

        // Docker wraps the path in a tar with the basename as root.
        // e.g. downloading /home/tester gives entries like tester/, tester/.bashrc, etc.
        // We strip the first path component and extract relative to local_path.
        let mut entry_count = 0;
        for entry in entries {
            let mut entry = entry.map_err(|e| AppError::Infra(format!("Tar entry error: {e}")))?;
            let entry_path = entry
                .path()
                .map_err(|e| AppError::Infra(format!("Tar path error: {e}")))?
                .to_path_buf();

            entry_count += 1;

            // Strip the first component (Docker's wrapper directory)
            let components: Vec<_> = entry_path.components().collect();
            if components.len() <= 1 {
                // This is the root directory entry itself; if it's a dir, ensure local_path exists
                if entry.header().entry_type().is_dir() {
                    std::fs::create_dir_all(local_path)
                        .map_err(|e| AppError::Infra(format!("Cannot create dir: {e}")))?;
                } else {
                    // Single file download - write directly to local_path
                    if let Some(parent) = local_path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| AppError::Infra(format!("Cannot create dir: {e}")))?;
                    }
                    let mut file = std::fs::File::create(local_path)
                        .map_err(|e| AppError::Infra(format!("Cannot create file: {e}")))?;
                    std::io::copy(&mut entry, &mut file)
                        .map_err(|e| AppError::Infra(format!("Copy error: {e}")))?;
                }
                continue;
            }

            // Build the destination path by stripping the first component
            let relative: std::path::PathBuf = components[1..].iter().collect();
            let dest = local_path.join(&relative);

            if entry.header().entry_type().is_dir() {
                std::fs::create_dir_all(&dest)
                    .map_err(|e| AppError::Infra(format!("Cannot create dir: {e}")))?;
            } else {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| AppError::Infra(format!("Cannot create dir: {e}")))?;
                }
                let mut file = std::fs::File::create(&dest)
                    .map_err(|e| AppError::Infra(format!("Cannot create file {}: {e}", dest.display())))?;
                std::io::copy(&mut entry, &mut file)
                    .map_err(|e| AppError::Infra(format!("Copy error: {e}")))?;
            }
        }

        debug!("Copied {} tar entries from container:{} to {}", entry_count, container_path, local_path.display());
        Ok(())
    }

    /// Deploy the app under test into the container. Returns the path to run inside the container.
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
        }
    }

    /// Launch the app inside the container (non-blocking).
    /// App stdout/stderr is captured to /tmp/app.log for debugging.
    /// AppImages are launched with --appimage-extract-and-run to avoid FUSE issues in containers.
    /// All apps get --no-sandbox since Chromium's sandbox doesn't work in containers.
    pub async fn launch_app(&self, app_path: &str, is_appimage: bool) -> Result<(), AppError> {
        let mut args: Vec<&str> = vec![app_path];
        if is_appimage {
            args.push("--appimage-extract-and-run");
        }
        // Chromium/Electron sandbox doesn't work in containers
        args.push("--no-sandbox");

        self.exec_detached_with_log(&args, "/tmp/app.log").await?;
        info!("Launched app: {app_path}");
        Ok(())
    }

    /// Stop and remove the container.
    pub async fn cleanup(&self) -> Result<(), AppError> {
        debug!("Stopping container {}", self.container_id);
        let _ = self
            .client
            .stop_container(
                &self.container_id,
                Some(StopContainerOptions { t: 5 }),
            )
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
            display_width: 1280,
            display_height: 800,
            vnc_bind_addr: "127.0.0.1".into(),
            vnc_port: None,
            app_type: crate::config::AppType::Appimage,
            app_path: None,
            app_dir: None,
            entrypoint: None,
            startup_timeout_seconds: 30,
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
        let session = DockerSession::create(&config).await.unwrap();

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
        let session = DockerSession::create(&config).await.unwrap();

        let output = session.exec(&["echo", "hello"]).await.unwrap();
        assert!(output.trim().contains("hello"));

        session.cleanup().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_copy_file_into_container() {
        let config = test_config();
        let session = DockerSession::create(&config).await.unwrap();

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
}
