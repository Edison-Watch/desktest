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
    /// **Privileges:** By default, no containers receive `CAP_SYS_ADMIN` or
    /// `/dev/fuse`. AppImages are launched with `--appimage-extract-and-run`,
    /// bypassing FUSE entirely. Custom Docker images that need FUSE can opt in
    /// via `needs_fuse: true` in the task's `app` config, which adds
    /// `CAP_SYS_ADMIN` and maps `/dev/fuse` into the container.
    pub async fn create(
        config: &Config,
        custom_image: Option<&str>,
        extra_env: Option<&std::collections::HashMap<String, String>>,
        no_network: bool,
        needs_fuse: bool,
        expected_digest: Option<&str>,
    ) -> Result<Self, AppError> {
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
                    warn!(
                        "Custom image '{img}' not found locally — pulling from remote registry. Ensure you trust this image source."
                    );
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

        // Verify image digest if one was specified.
        // Note: digest verification uses `repo_digests` which is only populated
        // for images pulled from a registry. Locally-built images (via
        // `docker build`) have no repo_digests and will fail verification.
        // Users should omit the `digest` field for locally-built images.
        if let Some(digest) = expected_digest {
            let inspect = client.inspect_image(&image_name).await.map_err(|e| {
                AppError::Infra(format!(
                    "Cannot inspect image '{image_name}' for digest verification: {e}"
                ))
            })?;

            let full_digest = format!(
                "sha256:{}",
                digest.strip_prefix("sha256:").unwrap_or(digest)
            );

            let repo_digests = inspect.repo_digests.unwrap_or_default();
            let matched = repo_digests.iter().any(|rd| {
                // repo_digests entries look like "image@sha256:abc123..."
                rd.ends_with(&full_digest) || rd.contains(&format!("@{full_digest}"))
            });

            if !matched {
                let hint = if repo_digests.is_empty() {
                    " (repo_digests is empty — this image may have been built \
                     locally rather than pulled from a registry; digest \
                     verification only works for registry-pulled images)"
                } else {
                    ""
                };
                return Err(AppError::Config(format!(
                    "Image digest mismatch for '{image_name}': expected {full_digest}, \
                     but image has repo_digests: {repo_digests:?}{hint}"
                )));
            }
            info!("Image digest verified: {full_digest}");
        }

        // VNC port inside the container is always 5900.
        // If the user specified a host port, we bind it; otherwise VNC runs but isn't exposed.
        let container_vnc_port = "5900";

        let mut env: Vec<String> = vec![
            format!("DISPLAY_WIDTH={}", config.display_width),
            format!("DISPLAY_HEIGHT={}", config.display_height),
            format!("VNC_PORT={container_vnc_port}"),
        ];

        if let Some(password) = &config.vnc_password {
            if password.len() > 8 {
                tracing::warn!(
                    "VNC password exceeds 8 characters — the VNC RFB protocol silently \
                     truncates to 8. Only the first 8 characters will be used."
                );
            }
            // Note: the password is visible via `docker inspect` and /proc/1/environ.
            // This is acceptable because the threat model is LAN adversaries, not
            // local Docker-admin attackers.
            env.push(format!("VNC_PASSWORD={password}"));
        }

        if let Some(secrets) = extra_env {
            for (key, value) in secrets {
                env.push(format!("DESKTEST_SECRET_{key}={value}"));
            }
        }

        let mem = config
            .container_memory_bytes
            .unwrap_or(4 * 1024 * 1024 * 1024);
        let mut host_config = HostConfig {
            // Resource limits: prevent runaway processes (e.g. from LLM-generated
            // code) from consuming all host resources. Configurable via config JSON
            // with these defaults.
            memory: Some(mem),
            memory_swap: Some(mem), // No swap (equal to memory limit)
            nano_cpus: Some(config.container_nano_cpus.unwrap_or(4_000_000_000)),
            pids_limit: Some(config.container_pids_limit.unwrap_or(512)),
            network_mode: if no_network {
                Some("none".to_string())
            } else {
                None
            },
            // Security hardening: drop all capabilities by default and prevent
            // privilege escalation. Specific caps are added back below as needed.
            cap_drop: Some(vec!["ALL".to_string()]),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            ..Default::default()
        };

        if no_network {
            info!("Container network disabled (--no-network)");
        }

        if needs_fuse {
            host_config.cap_add = Some(vec!["SYS_ADMIN".to_string()]);
            host_config.devices = Some(vec![bollard::models::DeviceMapping {
                path_on_host: Some("/dev/fuse".to_string()),
                path_in_container: Some("/dev/fuse".to_string()),
                cgroup_permissions: Some("rwm".to_string()),
            }]);
            // fusermount/fusermount3 are setuid binaries — no-new-privileges
            // blocks setuid, so we must relax it when FUSE is needed.
            host_config.security_opt = None;
            info!("FUSE enabled: added CAP_SYS_ADMIN and /dev/fuse device (no-new-privileges relaxed)");
        }

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
                "VNC will be available at {}",
                crate::config::format_host_port(&config.vnc_bind_addr, vnc_port)
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
    /// Runs a single batched shell script inside the container to check all
    /// required binaries, Python packages, and files. Returns `AppError::Config`
    /// (exit code 2) if any dependency is missing.
    pub async fn validate_custom_image(&self) -> Result<(), AppError> {
        // Build a single validation script that checks everything in one exec call.
        // Each check echoes a structured line: "CHECK:<tag>:<status>" where status
        // is OK or MISSING. Using `;` (not `&&`) ensures all checks run regardless
        // of individual failures.
        let mut script_parts: Vec<String> = Vec::new();

        for binary in Self::REQUIRED_BINARIES {
            script_parts.push(format!(
                "if command -v {binary} >/dev/null 2>&1; then echo 'CHECK:BIN_{binary}:OK'; else echo 'CHECK:BIN_{binary}:MISSING'; fi"
            ));
        }

        for (package, _apt_name) in Self::REQUIRED_PYTHON_PACKAGES {
            script_parts.push(format!(
                "if python3 -c 'import {package}' 2>/dev/null; then echo 'CHECK:PY_{package}:OK'; else echo 'CHECK:PY_{package}:MISSING'; fi"
            ));
        }

        script_parts.push(
            "if test -f /home/tester/.Xauthority; then echo 'CHECK:XAUTHORITY:OK'; else echo 'CHECK:XAUTHORITY:MISSING'; fi".to_string()
        );

        let script = script_parts.join("; ");
        let output = self.exec(&["sh", "-c", &script]).await?;

        // Guard: if the script produced no output, the results are untrustworthy
        // (e.g. script was killed, OOM, or container issue).
        if output.trim().is_empty() {
            return Err(AppError::Infra(
                "Custom image validation script produced no output; cannot verify dependencies"
                    .into(),
            ));
        }

        // Parse structured output to reconstruct per-check results
        let mut missing: Vec<String> = Vec::new();

        for binary in Self::REQUIRED_BINARIES {
            let tag = format!("CHECK:BIN_{binary}:MISSING");
            if output.contains(&tag) {
                missing.push(format!("{binary} (binary)"));
            }
        }

        for (package, apt_name) in Self::REQUIRED_PYTHON_PACKAGES {
            let tag = format!("CHECK:PY_{package}:MISSING");
            if output.contains(&tag) {
                missing.push(format!("{apt_name} (Python package '{package}')"));
            }
        }

        if output.contains("CHECK:XAUTHORITY:MISSING") {
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
            api_key_source: None,
            provider: "openai".into(),
            model: "gpt-4.1".into(),
            llm_max_retries: 5,
            api_base_url: "https://api.openai.com".into(),
            display_width: 1920,
            display_height: 1080,
            vnc_bind_addr: "127.0.0.1".into(),
            vnc_port: None,
            vnc_password: None,
            tls_ca_bundle: None,
            app_type: crate::config::AppType::Appimage,
            app_path: None,
            app_dir: None,
            entrypoint: None,
            startup_timeout_seconds: 30,
            electron: false,
            container_memory_bytes: None,
            container_nano_cpus: None,
            container_pids_limit: None,
            integrations: Default::default(),
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
        let session = DockerSession::create(&config, None, None, false, false, None)
            .await
            .unwrap();

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
        let session = DockerSession::create(&config, None, None, false, false, None)
            .await
            .unwrap();

        let output = session.exec(&["echo", "hello"]).await.unwrap();
        assert!(output.trim().contains("hello"));

        session.cleanup().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Docker daemon
    async fn test_copy_file_into_container() {
        let config = test_config();
        let session = DockerSession::create(&config, None, None, false, false, None)
            .await
            .unwrap();

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
        let session = DockerSession::create(&config, None, None, false, false, None)
            .await
            .unwrap();
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
