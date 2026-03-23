use std::path::Path;

use futures::StreamExt;
use tracing::debug;

use super::DockerSession;
use crate::error::AppError;

impl DockerSession {
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
        let stream = self.client.download_from_container(
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
                let mut file = std::fs::File::create(&dest).map_err(|e| {
                    AppError::Infra(format!("Cannot create file {}: {e}", dest.display()))
                })?;
                std::io::copy(&mut entry, &mut file)
                    .map_err(|e| AppError::Infra(format!("Copy error: {e}")))?;
            }
        }

        debug!(
            "Copied {} tar entries from container:{} to {}",
            entry_count,
            container_path,
            local_path.display()
        );
        Ok(())
    }
}
