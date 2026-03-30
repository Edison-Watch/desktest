use std::path::Path;

use crate::docker::DockerSession;
use crate::error::AppError;
use crate::tart::TartSession;

/// Core session trait abstracting the environment (Docker container, Tart VM, native host).
///
/// All methods correspond to operations that are environment-agnostic:
/// executing commands, transferring files, and lifecycle management.
#[allow(dead_code)]
pub trait Session: Send + Sync {
    /// Execute a command and return stdout.
    async fn exec(&self, cmd: &[&str]) -> Result<String, AppError>;

    /// Execute a command and return (stdout, exit_code).
    async fn exec_with_exit_code(&self, cmd: &[&str]) -> Result<(String, i64), AppError>;

    /// Execute a command with data piped to stdin, return stdout.
    async fn exec_with_stdin(&self, cmd: &[&str], stdin_data: &[u8]) -> Result<String, AppError>;

    /// Execute a command in the background (detached).
    async fn exec_detached(&self, cmd: &[&str]) -> Result<(), AppError>;

    /// Execute a command in the background, redirecting output to a log file.
    async fn exec_detached_with_log(&self, cmd: &[&str], log_path: &str) -> Result<(), AppError>;

    /// Copy a file or directory from the host into the session environment.
    async fn copy_into(&self, src: &Path, dest_path: &str) -> Result<(), AppError>;

    /// Copy a file from the session environment to the host.
    async fn copy_from(&self, container_path: &str, local_path: &Path) -> Result<(), AppError>;

    /// Stop and clean up the session environment.
    async fn cleanup(&self) -> Result<(), AppError>;
}

/// Runtime session backend selection.
///
/// Each variant wraps a concrete session implementation. The `Session` trait
/// is implemented via the `forward_session!` macro which delegates to the
/// inner type.
#[allow(dead_code)]
pub enum SessionKind {
    /// Docker container session (Linux desktop testing).
    Docker(DockerSession),
    /// Tart VM session (macOS desktop testing).
    Tart(TartSession),
    // Native(NativeSession), — Phase 5
}

impl SessionKind {
    /// Access the underlying `DockerSession`, if this is a Docker session.
    ///
    /// Used for Docker-specific operations that are not part of the `Session`
    /// trait (e.g., `docker_client()`, `validate_custom_image()`, `deploy_app()`).
    pub fn as_docker(&self) -> Option<&DockerSession> {
        match self {
            SessionKind::Docker(s) => Some(s),
            SessionKind::Tart(_) => None,
        }
    }

    /// Access the underlying `TartSession`, if this is a Tart session.
    ///
    /// Used for Tart-specific operations that are not part of the `Session`
    /// trait (e.g., `deploy_app()`, `launch_app()`).
    pub fn as_tart(&self) -> Option<&TartSession> {
        match self {
            SessionKind::Tart(s) => Some(s),
            SessionKind::Docker(_) => None,
        }
    }
}

// Implement Session for DockerSession by delegating to existing methods.
impl Session for DockerSession {
    async fn exec(&self, cmd: &[&str]) -> Result<String, AppError> {
        self.exec(cmd).await
    }

    async fn exec_with_exit_code(&self, cmd: &[&str]) -> Result<(String, i64), AppError> {
        self.exec_with_exit_code(cmd).await
    }

    async fn exec_with_stdin(&self, cmd: &[&str], stdin_data: &[u8]) -> Result<String, AppError> {
        self.exec_with_stdin(cmd, stdin_data).await
    }

    async fn exec_detached(&self, cmd: &[&str]) -> Result<(), AppError> {
        self.exec_detached(cmd).await
    }

    async fn exec_detached_with_log(&self, cmd: &[&str], log_path: &str) -> Result<(), AppError> {
        self.exec_detached_with_log(cmd, log_path).await
    }

    async fn copy_into(&self, src: &Path, dest_path: &str) -> Result<(), AppError> {
        self.copy_into(src, dest_path).await
    }

    async fn copy_from(&self, container_path: &str, local_path: &Path) -> Result<(), AppError> {
        self.copy_from(container_path, local_path).await
    }

    async fn cleanup(&self) -> Result<(), AppError> {
        self.cleanup().await
    }
}

/// Generate `impl Session for SessionKind` by matching on each variant and
/// delegating to the inner type's `Session` implementation.
macro_rules! forward_session {
    ($($variant:ident),+ $(,)?) => {
        impl Session for SessionKind {
            async fn exec(&self, cmd: &[&str]) -> Result<String, AppError> {
                match self { $(SessionKind::$variant(s) => s.exec(cmd).await,)+ }
            }

            async fn exec_with_exit_code(&self, cmd: &[&str]) -> Result<(String, i64), AppError> {
                match self { $(SessionKind::$variant(s) => s.exec_with_exit_code(cmd).await,)+ }
            }

            async fn exec_with_stdin(&self, cmd: &[&str], stdin_data: &[u8]) -> Result<String, AppError> {
                match self { $(SessionKind::$variant(s) => s.exec_with_stdin(cmd, stdin_data).await,)+ }
            }

            async fn exec_detached(&self, cmd: &[&str]) -> Result<(), AppError> {
                match self { $(SessionKind::$variant(s) => s.exec_detached(cmd).await,)+ }
            }

            async fn exec_detached_with_log(&self, cmd: &[&str], log_path: &str) -> Result<(), AppError> {
                match self { $(SessionKind::$variant(s) => s.exec_detached_with_log(cmd, log_path).await,)+ }
            }

            async fn copy_into(&self, src: &Path, dest_path: &str) -> Result<(), AppError> {
                match self { $(SessionKind::$variant(s) => s.copy_into(src, dest_path).await,)+ }
            }

            async fn copy_from(&self, container_path: &str, local_path: &Path) -> Result<(), AppError> {
                match self { $(SessionKind::$variant(s) => s.copy_from(container_path, local_path).await,)+ }
            }

            async fn cleanup(&self) -> Result<(), AppError> {
                match self { $(SessionKind::$variant(s) => s.cleanup().await,)+ }
            }
        }
    };
}

forward_session!(Docker, Tart);
