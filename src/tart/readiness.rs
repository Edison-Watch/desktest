use std::time::Duration;

use tracing::debug;

use crate::error::AppError;
use crate::session::{Session, SessionKind};

/// Wait for the macOS desktop inside a Tart VM to be ready.
///
/// The VM agent sentinel (`agent_ready`) is already verified during
/// `TartSession::create()`. This function additionally verifies that
/// screencapture can take a screenshot, confirming the display is functional.
pub async fn wait_for_desktop_macos(
    session: &SessionKind,
    timeout: Duration,
    debug_mode: bool,
) -> Result<(), AppError> {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(500);

    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Infra(
                "Timeout waiting for macOS desktop to be ready".into(),
            ));
        }

        // Verify screencapture works (confirms display server is functional)
        let result = session
            .exec(&["screencapture", "-x", "/tmp/readiness-check.png"])
            .await;
        if result.is_ok() {
            debug!("macOS desktop is ready (screencapture works)");
            return Ok(());
        }

        if debug_mode {
            debug!("screencapture not yet working, waiting...");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Wait for a macOS app window to appear after launch.
///
/// Polls the process list for new GUI processes beyond the baseline.
/// Uses `lsappinfo` to detect visible applications, falling back to
/// a simple process-count check via the accessibility helper.
pub async fn wait_for_app_window_macos(
    session: &SessionKind,
    baseline_procs: &[String],
    timeout: Duration,
    debug_mode: bool,
) -> Result<(), AppError> {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(500);

    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Infra(
                "Timeout waiting for macOS app window to appear".into(),
            ));
        }

        let current = get_gui_process_list(session).await?;
        let new_procs: Vec<&String> = current
            .iter()
            .filter(|p| !baseline_procs.contains(p))
            .collect();

        if !new_procs.is_empty() {
            debug!("New app process(es) detected: {:?}", new_procs);
            return Ok(());
        }

        if debug_mode {
            debug!("No new app processes yet, waiting...");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Get the current list of GUI applications via AppleScript.
///
/// Returns process names of visible (non-background) applications.
pub async fn get_gui_process_list(session: &SessionKind) -> Result<Vec<String>, AppError> {
    let output = session
        .exec(&[
            "osascript",
            "-e",
            "tell application \"System Events\" to get name of every process whose background only is false",
        ])
        .await?;

    let procs: Vec<String> = output
        .split(", ")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(procs)
}

/// Get a stable baseline of GUI processes by waiting for the list to stabilize.
///
/// Times out after 30 seconds and returns whatever list we have.
pub async fn get_stable_gui_process_list(
    session: &SessionKind,
) -> Result<Vec<String>, AppError> {
    let mut last_procs = get_gui_process_list(session).await?;
    let mut stable_count = 0;
    let required_stable = 3;
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(30);

    loop {
        if start.elapsed() > timeout {
            tracing::warn!(
                "Process list did not stabilize within {}s — using current snapshot",
                timeout.as_secs()
            );
            return Ok(last_procs);
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
        let current = get_gui_process_list(session).await?;

        if current == last_procs {
            stable_count += 1;
            if stable_count >= required_stable {
                return Ok(current);
            }
        } else {
            stable_count = 0;
            last_procs = current;
        }
    }
}
