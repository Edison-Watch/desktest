#![allow(dead_code)]

use std::time::Duration;

use tracing::{debug, info};

use crate::error::AppError;
use crate::session::Session;

use super::WindowsVmSession;

/// Wait for the Windows desktop to be ready.
///
/// The agent_ready sentinel confirms the VM agent is running, but the desktop
/// may not be fully loaded yet. This probes the desktop by attempting a
/// screenshot via PyAutoGUI.
pub async fn wait_for_desktop(session: &WindowsVmSession) -> Result<(), AppError> {
    let timeout = Duration::from_secs(60);
    let deadline = tokio::time::Instant::now() + timeout;

    info!("Waiting for Windows desktop to be ready...");

    loop {
        let result = session
            .exec(&[
                "python",
                "-c",
                "import pyautogui; pyautogui.screenshot('C:\\\\Temp\\\\probe.png')",
            ])
            .await;

        match result {
            Ok(_) => {
                debug!("Windows desktop is ready (screenshot probe succeeded)");
                return Ok(());
            }
            Err(e) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(AppError::Infra(format!(
                        "Timed out waiting for Windows desktop to be ready: {e}"
                    )));
                }
                debug!("Desktop not ready yet, retrying in 3s...");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}
