use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

const SUPPORTED_SCHEMA_VERSION: &str = "1.0";

/// A structured task definition for desktop app testing.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskDefinition {
    /// Schema version (must be "1.0").
    pub schema_version: String,

    /// Unique identifier for the task.
    pub id: String,

    /// Natural language instruction for the agent.
    pub instruction: String,

    /// Application to test.
    pub app: AppConfig,

    /// Setup steps to run before the agent loop.
    #[serde(default)]
    pub config: Vec<SetupStep>,

    /// Evaluation configuration.
    #[serde(default)]
    pub evaluator: Option<EvaluatorConfig>,

    /// Overall test timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Maximum steps for the agent loop.
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,

    /// Optional explicit a11y extraction timeout in seconds. Skips probe if set.
    #[serde(default)]
    pub a11y_timeout_secs: Option<u64>,

    /// Optional max a11y tree nodes to extract (default: 10000).
    #[serde(default)]
    pub max_a11y_nodes: Option<usize>,

    /// Optional metadata (tags, author, etc.).
    #[serde(default)]
    pub metadata: serde_json::Value,
}

fn default_timeout() -> u64 {
    300
}

fn default_max_steps() -> u32 {
    15
}

/// Application configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppConfig {
    /// Deploy an AppImage file into the container.
    Appimage {
        path: String,
        #[serde(default)]
        electron: bool,
    },
    /// Deploy a folder-based app into the container.
    Folder {
        dir: String,
        entrypoint: String,
        #[serde(default)]
        electron: bool,
    },
    /// Use a pre-built custom Docker image.
    DockerImage {
        image: String,
        #[serde(default)]
        entrypoint_cmd: Option<String>,
    },
    /// Attach to an existing running desktop (used with `desktest attach`).
    /// The app section is ignored in attach mode; this variant exists so
    /// task JSON files can signal they are designed for attach mode.
    VncAttach {
        #[serde(default)]
        note: Option<String>,
    },
}

/// A setup step to execute before the agent loop.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SetupStep {
    /// Run a shell command inside the container.
    Execute { command: String },
    /// Copy a file or directory into the container.
    Copy { src: String, dest: String },
    /// Open a file with xdg-open or a specified application.
    Open {
        target: String,
        #[serde(default)]
        app: Option<String>,
    },
    /// Wait for a specified number of seconds.
    Sleep { seconds: f64 },
}

/// Evaluation configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvaluatorConfig {
    /// Evaluation mode.
    pub mode: EvaluatorMode,

    /// Programmatic metrics to evaluate (used in programmatic and hybrid modes).
    #[serde(default)]
    pub metrics: Vec<MetricConfig>,

    /// How to combine multiple metrics: "and" (all must pass) or "or" (any must pass).
    #[serde(default = "default_conjunction")]
    pub conjunction: Conjunction,

    /// Per-execution timeout in seconds for evaluator commands (default: 120).
    #[serde(default)]
    pub eval_timeout_secs: Option<u64>,
}

fn default_conjunction() -> Conjunction {
    Conjunction::And
}

/// Evaluation mode.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorMode {
    /// Only agent verdict (existing desktest behavior).
    Llm,
    /// Setup steps only + programmatic metrics (no agent loop).
    Programmatic,
    /// Agent verdict AND programmatic metrics both required.
    Hybrid,
}

/// How to combine multiple metric results.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Conjunction {
    And,
    Or,
}

/// A programmatic evaluation metric.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MetricConfig {
    /// Compare a file from the container against an expected file.
    FileCompare {
        /// Path to the file inside the container.
        actual_path: String,
        /// Path to the expected file on the host.
        expected_path: String,
        /// Comparison mode: "exact" or "normalized" (ignore trailing whitespace/newlines).
        #[serde(default = "default_compare_mode")]
        compare_mode: CompareMode,
    },
    /// Semantically compare structured data files (JSON, YAML, XML, CSV).
    FileCompareSemantic {
        /// Path to the file inside the container.
        actual_path: String,
        /// Path to the expected file on the host.
        expected_path: String,
        /// File format for parsing.
        format: SemanticFormat,
    },
    /// Run a command and check stdout.
    CommandOutput {
        /// Command to run inside the container.
        command: String,
        /// Expected output.
        expected: String,
        /// Match mode.
        #[serde(default = "default_match_mode")]
        match_mode: MatchMode,
    },
    /// Check if a file exists (or does not exist) in the container.
    FileExists {
        /// Path to check inside the container.
        path: String,
        /// If true, assert the file does NOT exist.
        #[serde(default)]
        should_not_exist: bool,
    },
    /// Run a command and check its exit code.
    ExitCode {
        /// Command to run inside the container.
        command: String,
        /// Expected exit code.
        expected: i32,
    },
    /// Run a Python replay script inside the container.
    ScriptReplay {
        /// Path to the Python script on the host.
        script_path: String,
        /// Optional directory containing expected screenshots (copied into container).
        #[serde(default)]
        screenshots_dir: Option<String>,
    },
}

fn default_compare_mode() -> CompareMode {
    CompareMode::Exact
}

fn default_match_mode() -> MatchMode {
    MatchMode::Contains
}

/// File comparison mode.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareMode {
    Exact,
    Normalized,
}

/// Supported formats for semantic file comparison.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFormat {
    Json,
    Yaml,
    Xml,
    Csv,
}

/// How to match command output.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    Contains,
    Equals,
    Regex,
}

impl TaskDefinition {
    /// Load and validate a task definition from a JSON file.
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Cannot read task file '{}': {e}", path.display())))?;

        Self::parse_and_validate(&contents)
    }

    /// Parse JSON string and validate the task definition.
    pub fn parse_and_validate(json: &str) -> Result<Self, AppError> {
        let task: TaskDefinition =
            serde_json::from_str(json).map_err(|e| AppError::Config(format!("Invalid task JSON: {e}")))?;

        task.validate()?;
        Ok(task)
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(AppError::Config(format!(
                "Unsupported schema_version '{}'. Expected '{SUPPORTED_SCHEMA_VERSION}'.",
                self.schema_version
            )));
        }

        if self.id.is_empty() {
            return Err(AppError::Config("Task 'id' must not be empty.".into()));
        }

        if self.instruction.is_empty() {
            return Err(AppError::Config("Task 'instruction' must not be empty.".into()));
        }

        if self.timeout == 0 {
            return Err(AppError::Config("Task 'timeout' must be > 0.".into()));
        }

        if self.max_steps == 0 {
            return Err(AppError::Config("Task 'max_steps' must be > 0.".into()));
        }

        if let Some(0) = self.a11y_timeout_secs {
            return Err(AppError::Config(
                "Task 'a11y_timeout_secs' must be > 0 if specified.".into(),
            ));
        }

        if let Some(0) = self.max_a11y_nodes {
            return Err(AppError::Config(
                "Task 'max_a11y_nodes' must be > 0 if specified.".into(),
            ));
        }

        // Validate setup steps
        for (i, step) in self.config.iter().enumerate() {
            match step {
                SetupStep::Execute { command } if command.is_empty() => {
                    return Err(AppError::Config(format!(
                        "Setup step {i}: 'execute' command must not be empty."
                    )));
                }
                SetupStep::Copy { src, dest } => {
                    if src.is_empty() {
                        return Err(AppError::Config(format!(
                            "Setup step {i}: 'copy' src must not be empty."
                        )));
                    }
                    if dest.is_empty() {
                        return Err(AppError::Config(format!(
                            "Setup step {i}: 'copy' dest must not be empty."
                        )));
                    }
                }
                SetupStep::Open { target, .. } if target.is_empty() => {
                    return Err(AppError::Config(format!(
                        "Setup step {i}: 'open' target must not be empty."
                    )));
                }
                SetupStep::Sleep { seconds } if *seconds <= 0.0 => {
                    return Err(AppError::Config(format!(
                        "Setup step {i}: 'sleep' seconds must be > 0."
                    )));
                }
                _ => {}
            }
        }

        // Validate evaluator config
        if let Some(evaluator) = &self.evaluator {
            match evaluator.mode {
                EvaluatorMode::Programmatic | EvaluatorMode::Hybrid => {
                    if evaluator.metrics.is_empty() {
                        return Err(AppError::Config(format!(
                            "Evaluator mode '{}' requires at least one metric.",
                            if evaluator.mode == EvaluatorMode::Programmatic {
                                "programmatic"
                            } else {
                                "hybrid"
                            }
                        )));
                    }
                }
                EvaluatorMode::Llm => {}
            }

            if let Some(0) = evaluator.eval_timeout_secs {
                return Err(AppError::Config(
                    "Evaluator 'eval_timeout_secs' must be > 0 if specified.".into(),
                ));
            }

            // Validate individual metrics
            for (i, metric) in evaluator.metrics.iter().enumerate() {
                match metric {
                    MetricConfig::FileCompare {
                        actual_path,
                        expected_path,
                        ..
                    } => {
                        if actual_path.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (file_compare): 'actual_path' must not be empty."
                            )));
                        }
                        if expected_path.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (file_compare): 'expected_path' must not be empty."
                            )));
                        }
                    }
                    MetricConfig::FileCompareSemantic {
                        actual_path,
                        expected_path,
                        ..
                    } => {
                        if actual_path.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (file_compare_semantic): 'actual_path' must not be empty."
                            )));
                        }
                        if expected_path.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (file_compare_semantic): 'expected_path' must not be empty."
                            )));
                        }
                    }
                    MetricConfig::CommandOutput {
                        command, expected, ..
                    } => {
                        if command.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (command_output): 'command' must not be empty."
                            )));
                        }
                        if expected.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (command_output): 'expected' must not be empty."
                            )));
                        }
                    }
                    MetricConfig::FileExists { path, .. } => {
                        if path.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (file_exists): 'path' must not be empty."
                            )));
                        }
                    }
                    MetricConfig::ExitCode { command, .. } => {
                        if command.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (exit_code): 'command' must not be empty."
                            )));
                        }
                    }
                    MetricConfig::ScriptReplay { script_path, .. } => {
                        if script_path.is_empty() {
                            return Err(AppError::Config(format!(
                                "Metric {i} (script_replay): 'script_path' must not be empty."
                            )));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_valid_task() -> &'static str {
        r#"{
            "schema_version": "1.0",
            "id": "test-001",
            "instruction": "Open gedit and type hello",
            "app": {
                "type": "appimage",
                "path": "/apps/gedit.AppImage"
            }
        }"#
    }

    #[test]
    fn test_parse_minimal_task() {
        let task = TaskDefinition::parse_and_validate(minimal_valid_task()).unwrap();
        assert_eq!(task.schema_version, "1.0");
        assert_eq!(task.id, "test-001");
        assert_eq!(task.instruction, "Open gedit and type hello");
        assert!(matches!(task.app, AppConfig::Appimage { .. }));
        assert!(task.config.is_empty());
        assert!(task.evaluator.is_none());
        assert_eq!(task.timeout, 300);
        assert_eq!(task.max_steps, 15);
    }

    #[test]
    fn test_parse_full_task() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "gedit-save-test",
            "instruction": "Open gedit, type 'hello world', save as /tmp/output.txt",
            "app": {
                "type": "folder",
                "dir": "/apps/gedit",
                "entrypoint": "gedit"
            },
            "config": [
                {"type": "execute", "command": "mkdir -p /tmp/test"},
                {"type": "copy", "src": "fixtures/input.txt", "dest": "/home/tester/input.txt"},
                {"type": "open", "target": "/home/tester/input.txt", "app": "gedit"},
                {"type": "sleep", "seconds": 2.0}
            ],
            "evaluator": {
                "mode": "hybrid",
                "metrics": [
                    {
                        "type": "file_compare",
                        "actual_path": "/tmp/output.txt",
                        "expected_path": "fixtures/expected.txt",
                        "compare_mode": "normalized"
                    },
                    {
                        "type": "command_output",
                        "command": "cat /tmp/output.txt",
                        "expected": "hello world",
                        "match_mode": "contains"
                    },
                    {
                        "type": "file_exists",
                        "path": "/tmp/output.txt"
                    },
                    {
                        "type": "exit_code",
                        "command": "test -f /tmp/output.txt",
                        "expected": 0
                    }
                ],
                "conjunction": "and"
            },
            "timeout": 120,
            "max_steps": 20,
            "metadata": {"author": "test", "tags": ["gedit", "save"]}
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        assert_eq!(task.id, "gedit-save-test");
        assert!(matches!(task.app, AppConfig::Folder { .. }));
        assert_eq!(task.config.len(), 4);
        let evaluator = task.evaluator.unwrap();
        assert_eq!(evaluator.mode, EvaluatorMode::Hybrid);
        assert_eq!(evaluator.metrics.len(), 4);
        assert_eq!(evaluator.conjunction, Conjunction::And);
        assert_eq!(task.timeout, 120);
        assert_eq!(task.max_steps, 20);
    }

    #[test]
    fn test_parse_docker_image_app() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "libreoffice-test",
            "instruction": "Open LibreOffice Writer",
            "app": {
                "type": "docker_image",
                "image": "my-libreoffice:latest",
                "entrypoint_cmd": "libreoffice --writer"
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        match &task.app {
            AppConfig::DockerImage {
                image,
                entrypoint_cmd,
            } => {
                assert_eq!(image, "my-libreoffice:latest");
                assert_eq!(entrypoint_cmd.as_deref(), Some("libreoffice --writer"));
            }
            _ => panic!("Expected DockerImage app config"),
        }
    }

    #[test]
    fn test_parse_semantic_compare_metric() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "json-compare-test",
            "instruction": "Edit JSON file",
            "app": {"type": "appimage", "path": "/apps/editor.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "metrics": [{
                    "type": "file_compare_semantic",
                    "actual_path": "/tmp/output.json",
                    "expected_path": "fixtures/expected.json",
                    "format": "json"
                }]
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        let evaluator = task.evaluator.unwrap();
        match &evaluator.metrics[0] {
            MetricConfig::FileCompareSemantic { format, .. } => {
                assert_eq!(*format, SemanticFormat::Json);
            }
            _ => panic!("Expected FileCompareSemantic metric"),
        }
    }

    #[test]
    fn test_reject_unsupported_schema_version() {
        let json = r#"{
            "schema_version": "2.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(err.to_string().contains("Unsupported schema_version"));
        assert!(err.to_string().contains("2.0"));
    }

    #[test]
    fn test_reject_empty_id() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("id"));
    }

    #[test]
    fn test_reject_empty_instruction() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("instruction"));
    }

    #[test]
    fn test_reject_missing_required_field() {
        // Missing schema_version
        let json = r#"{
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"}
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(err.to_string().contains("schema_version"));
    }

    #[test]
    fn test_reject_unknown_app_type() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "flatpak", "path": "/apps/test"}
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
    }

    #[test]
    fn test_reject_invalid_json() {
        let err = TaskDefinition::parse_and_validate("not json").unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(err.to_string().contains("Invalid task JSON"));
    }

    #[test]
    fn test_reject_empty_execute_command() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "config": [{"type": "execute", "command": ""}]
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("execute"));
        assert!(err.to_string().contains("command"));
    }

    #[test]
    fn test_reject_empty_copy_src() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "config": [{"type": "copy", "src": "", "dest": "/tmp/file"}]
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("copy"));
        assert!(err.to_string().contains("src"));
    }

    #[test]
    fn test_reject_zero_sleep() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "config": [{"type": "sleep", "seconds": 0}]
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("sleep"));
        assert!(err.to_string().contains("seconds"));
    }

    #[test]
    fn test_reject_programmatic_mode_without_metrics() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "metrics": []
            }
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("programmatic"));
        assert!(err.to_string().contains("metric"));
    }

    #[test]
    fn test_reject_hybrid_mode_without_metrics() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "hybrid",
                "metrics": []
            }
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("hybrid"));
        assert!(err.to_string().contains("metric"));
    }

    #[test]
    fn test_llm_mode_without_metrics_is_ok() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "llm",
                "metrics": []
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        assert_eq!(task.evaluator.unwrap().mode, EvaluatorMode::Llm);
    }

    #[test]
    fn test_or_conjunction() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "conjunction": "or",
                "metrics": [
                    {"type": "file_exists", "path": "/tmp/a"},
                    {"type": "file_exists", "path": "/tmp/b"}
                ]
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        let evaluator = task.evaluator.unwrap();
        assert_eq!(evaluator.conjunction, Conjunction::Or);
    }

    #[test]
    fn test_reject_empty_metric_path() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "metrics": [{"type": "file_exists", "path": ""}]
            }
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("file_exists"));
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn test_defaults_for_timeout_and_max_steps() {
        let task = TaskDefinition::parse_and_validate(minimal_valid_task()).unwrap();
        assert_eq!(task.timeout, 300);
        assert_eq!(task.max_steps, 15);
    }

    #[test]
    fn test_match_mode_variants() {
        for (mode_str, expected) in [
            ("contains", MatchMode::Contains),
            ("equals", MatchMode::Equals),
            ("regex", MatchMode::Regex),
        ] {
            let json = format!(
                r#"{{
                    "schema_version": "1.0",
                    "id": "test",
                    "instruction": "test",
                    "app": {{"type": "appimage", "path": "/apps/test.AppImage"}},
                    "evaluator": {{
                        "mode": "programmatic",
                        "metrics": [{{
                            "type": "command_output",
                            "command": "echo hi",
                            "expected": "hi",
                            "match_mode": "{mode_str}"
                        }}]
                    }}
                }}"#
            );
            let task = TaskDefinition::parse_and_validate(&json).unwrap();
            let evaluator = task.evaluator.unwrap();
            match &evaluator.metrics[0] {
                MetricConfig::CommandOutput { match_mode, .. } => {
                    assert_eq!(*match_mode, expected);
                }
                _ => panic!("Expected CommandOutput"),
            }
        }
    }

    #[test]
    fn test_parse_electron_folder_app() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "electron-test",
            "instruction": "Open the Electron app",
            "app": {
                "type": "folder",
                "dir": "/apps/myelectronapp",
                "entrypoint": "myelectronapp",
                "electron": true
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        match &task.app {
            AppConfig::Folder { dir, entrypoint, electron } => {
                assert_eq!(dir, "/apps/myelectronapp");
                assert_eq!(entrypoint, "myelectronapp");
                assert!(electron);
            }
            _ => panic!("Expected Folder app config"),
        }
    }

    #[test]
    fn test_parse_folder_app_electron_defaults_false() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "folder-test",
            "instruction": "Open the app",
            "app": {
                "type": "folder",
                "dir": "/apps/myapp",
                "entrypoint": "myapp"
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        match &task.app {
            AppConfig::Folder { electron, .. } => {
                assert!(!electron);
            }
            _ => panic!("Expected Folder app config"),
        }
    }

    #[test]
    fn test_load_nonexistent_file() {
        let err = TaskDefinition::load(Path::new("/nonexistent/task.json")).unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(err.to_string().contains("Cannot read task file"));
    }

    #[test]
    fn test_reject_zero_timeout() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "timeout": 0
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn test_reject_zero_max_steps() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "max_steps": 0
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("max_steps"));
    }

    #[test]
    fn test_reject_zero_a11y_timeout_secs() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "a11y_timeout_secs": 0
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("a11y_timeout_secs"));
    }

    #[test]
    fn test_reject_zero_max_a11y_nodes() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "max_a11y_nodes": 0
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("max_a11y_nodes"));
    }

    #[test]
    fn test_valid_a11y_overrides() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "a11y_timeout_secs": 30,
            "max_a11y_nodes": 5000
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        assert_eq!(task.a11y_timeout_secs, Some(30));
        assert_eq!(task.max_a11y_nodes, Some(5000));
    }

    #[test]
    fn test_regex_match_mode() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "metrics": [{
                    "type": "command_output",
                    "command": "echo hello123",
                    "expected": "hello\\d+",
                    "match_mode": "regex"
                }]
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        let evaluator = task.evaluator.unwrap();
        match &evaluator.metrics[0] {
            MetricConfig::CommandOutput { match_mode, .. } => {
                assert_eq!(*match_mode, MatchMode::Regex);
            }
            _ => panic!("Expected CommandOutput"),
        }
    }

    #[test]
    fn test_reject_zero_eval_timeout_secs() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "metrics": [{"type": "file_exists", "path": "/tmp/a"}],
                "eval_timeout_secs": 0
            }
        }"#;

        let err = TaskDefinition::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("eval_timeout_secs"));
    }

    #[test]
    fn test_valid_eval_timeout_secs() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "test",
            "instruction": "test",
            "app": {"type": "appimage", "path": "/apps/test.AppImage"},
            "evaluator": {
                "mode": "programmatic",
                "metrics": [{"type": "file_exists", "path": "/tmp/a"}],
                "eval_timeout_secs": 60
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        assert_eq!(task.evaluator.unwrap().eval_timeout_secs, Some(60));
    }

    #[test]
    fn test_parse_vnc_attach_app() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "attach-test",
            "instruction": "Click the OK button",
            "app": {
                "type": "vnc_attach",
                "note": "This task is designed for desktest attach mode"
            }
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        match &task.app {
            AppConfig::VncAttach { note } => {
                assert_eq!(note.as_deref(), Some("This task is designed for desktest attach mode"));
            }
            _ => panic!("Expected VncAttach app config"),
        }
    }

    #[test]
    fn test_parse_vnc_attach_app_minimal() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "attach-test",
            "instruction": "Click the OK button",
            "app": {"type": "vnc_attach"}
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        assert!(matches!(task.app, AppConfig::VncAttach { .. }));
    }

    #[test]
    fn test_vnc_attach_with_evaluator() {
        let json = r#"{
            "schema_version": "1.0",
            "id": "attach-eval-test",
            "instruction": "Approve the dialog",
            "app": {"type": "vnc_attach"},
            "evaluator": {"mode": "llm"},
            "timeout": 60,
            "max_steps": 10
        }"#;

        let task = TaskDefinition::parse_and_validate(json).unwrap();
        assert!(matches!(task.app, AppConfig::VncAttach { .. }));
        assert_eq!(task.timeout, 60);
        assert_eq!(task.max_steps, 10);
        assert_eq!(task.evaluator.unwrap().mode, EvaluatorMode::Llm);
    }
}
