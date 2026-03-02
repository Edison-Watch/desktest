// Allow dead code for types/fields that are defined now but used in later phases.
#![allow(dead_code)]

use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Infrastructure error: {0}")]
    Infra(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Docker error: {0}")]
    Docker(#[from] bollard::errors::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::Config(_) => 2,
            AppError::Infra(_) | AppError::Docker(_) | AppError::Io(_) => 3,
            AppError::Agent(_) | AppError::Http(_) => 4,
        }
    }

    /// Returns true if this error represents a user interrupt (Ctrl+C / SIGINT).
    pub fn is_interrupt(&self) -> bool {
        match self {
            AppError::Io(e) => e.kind() == std::io::ErrorKind::Interrupted,
            AppError::Docker(e) => e.to_string().contains("interrupted"),
            _ => false,
        }
    }
}

/// The outcome of a completed test run (not an error).
pub struct AgentOutcome {
    pub passed: bool,
    pub reasoning: String,
    pub screenshot_count: usize,
}

impl fmt::Display for AgentOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.passed { "PASSED" } else { "FAILED" };
        write!(f, "Test {}: {}", verdict, self.reasoning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_exit_code() {
        let err = AppError::Config("bad key".into());
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn test_infra_error_exit_code() {
        let err = AppError::Infra("container failed".into());
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn test_agent_error_exit_code() {
        let err = AppError::Agent("api timeout".into());
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn test_io_error_exit_code() {
        let err = AppError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn test_agent_outcome_display_passed() {
        let outcome = AgentOutcome {
            passed: true,
            reasoning: "all checks passed".into(),
            screenshot_count: 5,
        };
        assert_eq!(outcome.to_string(), "Test PASSED: all checks passed");
    }

    #[test]
    fn test_agent_outcome_display_failed() {
        let outcome = AgentOutcome {
            passed: false,
            reasoning: "button missing".into(),
            screenshot_count: 3,
        };
        assert_eq!(outcome.to_string(), "Test FAILED: button missing");
    }
}
