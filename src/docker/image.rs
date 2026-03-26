use std::path::Path;

use bollard::Docker;
use bollard::image::BuildImageOptions;
use futures::StreamExt;
use tracing::{debug, info};

use super::{DockerSession, IMAGE_NAME, IMAGE_NAME_ELECTRON};
use crate::error::AppError;

impl DockerSession {
    /// Build the Docker image from the docker/ directory if it doesn't already exist.
    pub async fn ensure_image(client: &Docker, force_rebuild: bool) -> Result<(), AppError> {
        if !force_rebuild && client.inspect_image(IMAGE_NAME).await.is_ok() {
            debug!("Image {IMAGE_NAME} already exists, skipping build");
            return Ok(());
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

    /// Build the Electron Docker image from docker/Dockerfile.electron if it doesn't already exist.
    pub async fn ensure_electron_image(
        client: &Docker,
        force_rebuild: bool,
    ) -> Result<(), AppError> {
        if !force_rebuild && client.inspect_image(IMAGE_NAME_ELECTRON).await.is_ok() {
            debug!("Image {IMAGE_NAME_ELECTRON} already exists, skipping build");
            return Ok(());
        }

        // Ensure base image exists first
        Self::ensure_image(client, false).await?;

        info!("Building Docker image {IMAGE_NAME_ELECTRON}...");

        let docker_dir = Self::find_docker_context()?;
        let tar_bytes = Self::create_tar_context(&docker_dir)?;

        let options = BuildImageOptions {
            t: IMAGE_NAME_ELECTRON.to_string(),
            dockerfile: "Dockerfile.electron".to_string(),
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

        info!("Docker image {IMAGE_NAME_ELECTRON} built successfully");
        Ok(())
    }

    pub(super) fn find_docker_context() -> Result<std::path::PathBuf, AppError> {
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

    pub(super) fn create_tar_context(docker_dir: &Path) -> Result<Vec<u8>, AppError> {
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
}
