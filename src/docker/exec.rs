use bollard::exec::CreateExecOptions;
use bollard::exec::StartExecResults;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::error::AppError;
use super::DockerSession;

impl DockerSession {
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

    /// Execute a command inside the container and return (stdout, exit_code).
    ///
    /// Unlike `exec()`, this inspects the process exit code via the Docker API,
    /// making it suitable for validation checks where a non-zero exit matters.
    pub async fn exec_with_exit_code(&self, cmd: &[&str]) -> Result<(String, i64), AppError> {
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

        let exec_id = exec.id.clone();

        let start_result = self
            .client
            .start_exec(&exec_id, None)
            .await
            .map_err(AppError::Docker)?;

        let mut output = String::new();
        if let StartExecResults::Attached { output: mut stream, .. } = start_result {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(AppError::Docker)?;
                output.push_str(&chunk.to_string());
            }
        }

        let inspect = self
            .client
            .inspect_exec(&exec_id)
            .await
            .map_err(AppError::Docker)?;

        let exit_code = inspect.exit_code.unwrap_or(-1);
        Ok((output, exit_code))
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
            &format!("nohup {escaped_cmd} > {} 2>&1 &", shell_escape::escape(log_path.into())),
        ])
        .await?;

        Ok(())
    }
}
