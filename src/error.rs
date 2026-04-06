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

/// Format an error for display on stderr, including a fix suggestion if available.
///
/// Prints the error message followed by a contextual suggestion on a separate line.
/// This keeps the core `Display` impl clean while giving users actionable guidance.
pub fn format_error_with_suggestion(error: &AppError) -> String {
    let base = error.to_string();
    if let Some(suggestion) = suggest_fix(error) {
        format!("{base}\n  Suggestion: {suggestion}")
    } else {
        base
    }
}

/// Return a contextual fix suggestion for common error patterns, or `None` if
/// no specific guidance applies.
fn suggest_fix(error: &AppError) -> Option<&'static str> {
    let msg = match error {
        AppError::Config(s) | AppError::Infra(s) | AppError::Agent(s) => s.as_str(),
        AppError::Docker(e) => return suggest_fix_docker(e),
        AppError::Http(e) => return suggest_fix_http(e),
        AppError::Io(e) => return suggest_fix_io(e),
    };

    // Task file errors
    if msg.contains("Cannot read task file") {
        return Some("Check the file path. Use `desktest validate <file>` to test task files.");
    }
    if msg.contains("Invalid task JSON") {
        return Some(
            "Check your task JSON syntax. Use `desktest validate <file>` to see detailed errors.",
        );
    }
    if msg.contains("Unsupported schema_version") {
        return Some("Update your task file to use the current schema version.");
    }

    // Config file errors
    if msg.contains("Cannot read config file") {
        return Some("Check the --config path. Run `desktest doctor` to verify your setup.");
    }
    if msg.contains("Invalid JSON") && matches!(error, AppError::Config(_)) {
        return Some("Check your config JSON syntax. See examples/ for reference config files.");
    }

    // API key errors
    if msg.contains("No API key found") {
        return Some("Run `desktest doctor` to check your API key configuration.");
    }

    // Docker errors
    if msg.contains("Cannot connect to Docker") || msg.contains("Docker daemon is not responding") {
        return Some("Run `desktest doctor` to check Docker status.");
    }

    // Timeout errors
    if msg.contains("timeout") || msg.contains("Timeout") || msg.contains("timed out") {
        return Some("Try increasing the task's 'timeout' field (in seconds).");
    }

    // Container/session setup
    if msg.contains("Cannot create artifacts dir") {
        return Some("Check filesystem permissions for the output directory.");
    }

    None
}

fn suggest_fix_docker(error: &bollard::errors::Error) -> Option<&'static str> {
    let msg = error.to_string();
    if msg.contains("connection refused") || msg.contains("No such file or directory") {
        return Some(
            "Docker daemon is not running. Start Docker and retry, or run `desktest doctor`.",
        );
    }
    if msg.contains("permission denied") {
        return Some(
            "Add your user to the docker group: `sudo usermod -aG docker $USER`, then log out and back in.",
        );
    }
    if msg.contains("404") || msg.contains("No such image") {
        return Some(
            "The Docker image was not found. Check the image name or run `docker pull <image>`.",
        );
    }
    None
}

fn suggest_fix_http(error: &reqwest::Error) -> Option<&'static str> {
    if error.is_timeout() {
        return Some("The LLM API request timed out. Check your network or try a different model.");
    }
    if error.is_connect() {
        return Some(
            "Cannot reach the API server. Check your network connection and api_base_url.",
        );
    }
    let status = error.status();
    if status == Some(reqwest::StatusCode::UNAUTHORIZED) {
        return Some("API key is invalid or expired. Check your API key with `desktest doctor`.");
    }
    if status == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
        return Some(
            "Rate limited by the API. Wait a moment and retry, or use a different API key.",
        );
    }
    if status.map(|s| s.is_server_error()).unwrap_or(false) {
        return Some(
            "The API server returned an error. This is usually transient — retry shortly.",
        );
    }
    None
}

fn suggest_fix_io(error: &std::io::Error) -> Option<&'static str> {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            Some("File or directory not found. Check the path and try again.")
        }
        std::io::ErrorKind::PermissionDenied => {
            Some("Permission denied. Check file permissions or run with appropriate privileges.")
        }
        std::io::ErrorKind::AddrInUse => Some(
            "Port is already in use. Choose a different --monitor-port or stop the conflicting process.",
        ),
        _ => None,
    }
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
            AppError::Infra(msg) => msg.contains("Interrupted by user"),
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
    pub bugs_found: usize,
}

impl fmt::Display for AgentOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = if self.passed { "PASSED" } else { "FAILED" };
        if self.bugs_found > 0 {
            write!(
                f,
                "Test {}: {} ({} bug(s) reported)",
                verdict, self.reasoning, self.bugs_found
            )
        } else {
            write!(f, "Test {}: {}", verdict, self.reasoning)
        }
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
            bugs_found: 0,
        };
        assert_eq!(outcome.to_string(), "Test PASSED: all checks passed");
    }

    #[test]
    fn test_agent_outcome_display_failed() {
        let outcome = AgentOutcome {
            passed: false,
            reasoning: "button missing".into(),
            screenshot_count: 3,
            bugs_found: 0,
        };
        assert_eq!(outcome.to_string(), "Test FAILED: button missing");
    }

    #[test]
    fn test_agent_outcome_display_with_bugs() {
        let outcome = AgentOutcome {
            passed: true,
            reasoning: "done".into(),
            screenshot_count: 5,
            bugs_found: 2,
        };
        assert_eq!(outcome.to_string(), "Test PASSED: done (2 bug(s) reported)");
    }

    #[test]
    fn test_suggest_fix_task_file() {
        let err = AppError::Config("Cannot read task file 'missing.json': No such file".into());
        let formatted = format_error_with_suggestion(&err);
        assert!(formatted.contains("Suggestion:"));
        assert!(formatted.contains("desktest validate"));
    }

    #[test]
    fn test_suggest_fix_invalid_json() {
        let err = AppError::Config("Invalid task JSON: expected value at line 1".into());
        let formatted = format_error_with_suggestion(&err);
        assert!(formatted.contains("Suggestion:"));
    }

    #[test]
    fn test_suggest_fix_api_key() {
        let err = AppError::Config("No API key found. Set it in config".into());
        let formatted = format_error_with_suggestion(&err);
        assert!(formatted.contains("desktest doctor"));
    }

    #[test]
    fn test_suggest_fix_docker() {
        let err = AppError::Infra("Cannot connect to Docker: connection refused".into());
        let formatted = format_error_with_suggestion(&err);
        assert!(formatted.contains("desktest doctor"));
    }

    #[test]
    fn test_suggest_fix_timeout() {
        let err = AppError::Infra("Operation timed out after 60s".into());
        let formatted = format_error_with_suggestion(&err);
        assert!(formatted.contains("timeout"));
    }

    #[test]
    fn test_suggest_fix_io_not_found() {
        let err = AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing file",
        ));
        let formatted = format_error_with_suggestion(&err);
        assert!(formatted.contains("not found"));
    }

    #[test]
    fn test_suggest_fix_io_permission() {
        let err = AppError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "access denied",
        ));
        let formatted = format_error_with_suggestion(&err);
        assert!(formatted.contains("Permission denied"));
    }

    #[test]
    fn test_no_suggestion_for_generic_error() {
        let err = AppError::Agent("something went wrong".into());
        let formatted = format_error_with_suggestion(&err);
        assert!(!formatted.contains("Suggestion:"));
    }
}
