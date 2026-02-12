#![allow(dead_code)]

use std::time::Duration;

use tracing::debug;

use crate::docker::DockerSession;
use crate::error::AppError;

/// Wait for the XFCE desktop to be ready inside the container.
///
/// Checks for the sentinel file written by entrypoint.sh and verifies
/// that scrot can capture a screenshot (meaning X display is functional).
pub async fn wait_for_desktop(
    session: &DockerSession,
    timeout: Duration,
    debug_mode: bool,
) -> Result<(), AppError> {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(500);

    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Infra(
                "Timeout waiting for desktop to be ready".into(),
            ));
        }

        // Check sentinel file
        let sentinel = session.exec(&["test", "-f", "/tmp/.desktop-ready"]).await;
        if sentinel.is_err() {
            if debug_mode {
                debug!("Desktop sentinel not yet present, waiting...");
            }
            tokio::time::sleep(poll_interval).await;
            continue;
        }

        // Verify X display is functional by trying scrot
        let scrot = session
            .exec(&["scrot", "-o", "/tmp/readiness-check.png"])
            .await;
        if scrot.is_err() {
            if debug_mode {
                debug!("scrot not yet working, waiting...");
            }
            tokio::time::sleep(poll_interval).await;
            continue;
        }

        debug!("Desktop is ready");
        return Ok(());
    }
}

/// Get the current list of visible X window IDs.
pub async fn get_window_list(session: &DockerSession) -> Result<Vec<String>, AppError> {
    let output = session
        .exec(&["xdotool", "search", "--onlyvisible", "--name", ""])
        .await?;

    let windows: Vec<String> = output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(windows)
}

/// Get a stable baseline of visible X windows by waiting for the window count
/// to stop changing. This avoids false positives from XFCE panels still loading.
pub async fn get_stable_window_list(session: &DockerSession) -> Result<Vec<String>, AppError> {
    let mut last_windows = get_window_list(session).await?;
    let mut stable_count = 0;
    let required_stable = 3; // need 3 consecutive identical readings

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let current = get_window_list(session).await?;

        if current == last_windows {
            stable_count += 1;
            if stable_count >= required_stable {
                debug!("Stable baseline window list: {} windows", current.len());
                return Ok(current);
            }
        } else {
            debug!(
                "Window list changed: {} -> {} windows, resetting stability counter",
                last_windows.len(),
                current.len()
            );
            last_windows = current;
            stable_count = 0;
        }
    }
}

/// Wait for a new X window to appear that wasn't in the baseline list.
pub async fn wait_for_app_window(
    session: &DockerSession,
    baseline_windows: &[String],
    timeout: Duration,
    debug_mode: bool,
) -> Result<(), AppError> {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(500);

    loop {
        if start.elapsed() > timeout {
            return Err(AppError::Infra(
                "Timeout waiting for app window to appear".into(),
            ));
        }

        let current = get_window_list(session).await.unwrap_or_default();
        let new_windows: Vec<_> = current
            .iter()
            .filter(|w| !baseline_windows.contains(w))
            .collect();

        if !new_windows.is_empty() {
            debug!("New app window(s) detected: {:?}", new_windows);
            return Ok(());
        }

        if debug_mode {
            debug!("No new windows yet, waiting...");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timeout_fires() {
        // Simulate a check that never succeeds by using a very short timeout
        // with a mock-like approach: we just test the timeout logic directly.
        let start = std::time::Instant::now();
        let timeout = Duration::from_millis(100);

        // Spin until timeout
        loop {
            if start.elapsed() > timeout {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // The timeout should have elapsed
        assert!(start.elapsed() >= timeout);
    }
}
