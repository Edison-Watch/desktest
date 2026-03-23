use std::time::Duration;

use tracing::{info, warn};

use super::MetricResult;
use crate::docker::DockerSession;
use crate::error::AppError;

/// script_replay: Copy a Python script into the container, run it, check for REPLAY_COMPLETE.
/// If `screenshots_dir` is provided, copies that directory into the container so that
/// screenshot comparison assertions can find their expected files.
pub(super) async fn evaluate_script_replay(
    session: &DockerSession,
    script_path: &str,
    screenshots_dir: Option<&str>,
    eval_timeout: Duration,
) -> Result<MetricResult, AppError> {
    let host_path = std::path::Path::new(script_path);
    if !host_path.exists() {
        return Err(AppError::Config(format!(
            "Replay script not found: {script_path}"
        )));
    }

    // Copy expected screenshots into container (for --with-screenshots scripts)
    if let Some(dir) = screenshots_dir {
        let dir_path = std::path::Path::new(dir);
        if dir_path.exists() {
            tokio::time::timeout(eval_timeout, session.copy_into(dir_path, "/home/tester/"))
                .await
                .map_err(|_| {
                    AppError::Agent(format!(
                        "Evaluation copy_into timed out after {}s: screenshots dir",
                        eval_timeout.as_secs()
                    ))
                })??;
            info!("Copied screenshots from {} into container", dir);
        } else {
            warn!("Screenshots directory not found: {dir}");
        }
    }

    // Copy script into container
    tokio::time::timeout(eval_timeout, session.copy_into(host_path, "/home/tester/"))
        .await
        .map_err(|_| {
            AppError::Agent(format!(
                "Evaluation copy_into timed out after {}s: {script_path}",
                eval_timeout.as_secs()
            ))
        })??;

    let script_name = host_path
        .file_name()
        .ok_or_else(|| AppError::Infra("No filename in script_path".into()))?
        .to_string_lossy();

    let container_script = format!("/home/tester/{script_name}");

    // Make executable and run
    tokio::time::timeout(
        eval_timeout,
        session.exec(&["chmod", "+x", &container_script]),
    )
    .await
    .map_err(|_| {
        AppError::Agent(format!(
            "Evaluation command timed out after {}s: chmod script",
            eval_timeout.as_secs()
        ))
    })??;
    let (output, exit_code) = tokio::time::timeout(
        eval_timeout,
        session.exec_with_exit_code(&["python3", &container_script]),
    )
    .await
    .map_err(|_| {
        AppError::Agent(format!(
            "Evaluation script timed out after {}s: {script_path}",
            eval_timeout.as_secs()
        ))
    })??;

    let has_complete = output.contains("REPLAY_COMPLETE");
    let passed = exit_code == 0 && has_complete;

    let detail = if passed {
        "Replay script completed successfully".to_string()
    } else if exit_code != 0 {
        format!("Replay script exited with code {exit_code}")
    } else {
        "Replay script did not output REPLAY_COMPLETE".to_string()
    };

    Ok(MetricResult {
        passed,
        metric: "script_replay".to_string(),
        expected: "exit_code=0, REPLAY_COMPLETE in output".to_string(),
        actual: format!("exit_code={exit_code}, complete={has_complete}"),
        detail,
    })
}
