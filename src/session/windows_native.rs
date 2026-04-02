use std::path::Path;

use crate::error::AppError;
use crate::session::Session;

/// A session that runs commands directly on a Windows host desktop.
///
/// Analogous to `NativeSession` (macOS), but for Windows hosts. Commands
/// execute as the current user on the host machine with no isolation.
///
/// **Not yet implemented** — requires cross-compiling desktest for Windows.
/// This module provides the type scaffolding so the rest of the codebase can
/// reference `WindowsNativeSession` in match arms and task definitions.
pub struct WindowsNativeSession {
    /// PIDs of processes launched via `exec_detached`, tracked for cleanup.
    #[allow(dead_code)]
    detached_pids: std::sync::Mutex<Vec<u32>>,
}

impl WindowsNativeSession {
    /// Create a new Windows native session.
    pub fn create() -> Self {
        Self {
            detached_pids: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Deploy a Windows app for native session.
    ///
    /// For `WindowsNative`, there is no deployment step — the app is expected
    /// to already be installed on the host. Returns the app_path or launch_cmd.
    pub async fn deploy_app(&self, app: &crate::task::AppConfig) -> Result<String, AppError> {
        match app {
            crate::task::AppConfig::WindowsNative {
                app_path,
                launch_cmd,
            } => {
                if let Some(cmd) = launch_cmd {
                    Ok(cmd.clone())
                } else if let Some(path) = app_path {
                    Ok(path.clone())
                } else {
                    Err(AppError::Config(
                        "WindowsNative: either 'app_path' or 'launch_cmd' must be set".into(),
                    ))
                }
            }
            other => Err(AppError::Config(format!(
                "WindowsNativeSession::deploy_app called with non-WindowsNative app config: {other:?}"
            ))),
        }
    }

    /// Launch a Windows app on the native host.
    pub async fn launch_app(
        &self,
        app: &crate::task::AppConfig,
        app_path: &str,
    ) -> Result<(), AppError> {
        let _ = (app, app_path);
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }
}

impl Session for WindowsNativeSession {
    async fn exec(&self, _cmd: &[&str]) -> Result<String, AppError> {
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }

    async fn exec_with_exit_code(&self, _cmd: &[&str]) -> Result<(String, i64), AppError> {
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }

    async fn exec_with_stdin(
        &self,
        _cmd: &[&str],
        _stdin_data: &[u8],
    ) -> Result<String, AppError> {
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }

    async fn exec_detached(&self, _cmd: &[&str]) -> Result<(), AppError> {
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }

    async fn exec_detached_with_log(
        &self,
        _cmd: &[&str],
        _log_path: &str,
    ) -> Result<(), AppError> {
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }

    async fn copy_into(&self, _src: &Path, _dest_path: &str) -> Result<(), AppError> {
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }

    async fn copy_from(
        &self,
        _container_path: &str,
        _local_path: &Path,
    ) -> Result<(), AppError> {
        Err(AppError::Infra(
            "WindowsNativeSession is not yet implemented — requires Windows host".into(),
        ))
    }

    async fn cleanup(&self) -> Result<(), AppError> {
        Ok(())
    }
}
