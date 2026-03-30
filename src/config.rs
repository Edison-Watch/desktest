use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AppError;

pub(crate) fn default_model() -> String {
    "claude-sonnet-4-5-20250929".into()
}

fn default_base_url() -> String {
    "https://api.anthropic.com".into()
}

fn default_width() -> u32 {
    1920
}

fn default_height() -> u32 {
    1080
}

fn default_vnc_addr() -> String {
    "127.0.0.1".into()
}

fn default_timeout() -> u64 {
    30
}

fn default_provider() -> String {
    "anthropic".into()
}

fn default_llm_max_retries() -> usize {
    5
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppType {
    Appimage,
    Folder,
    DockerImage,
    VncAttach,
    MacosTart,
    MacosNative,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub api_key: String,

    /// Tracks where the API key came from (for diagnostics). Not deserialized.
    #[serde(skip)]
    pub api_key_source: Option<&'static str>,

    #[serde(default = "default_provider")]
    pub provider: String,

    #[serde(default = "default_model")]
    pub model: String,

    /// Maximum number of retry attempts for retryable LLM API failures.
    #[serde(default = "default_llm_max_retries")]
    pub llm_max_retries: usize,

    #[serde(default = "default_base_url")]
    pub api_base_url: String,

    #[serde(default = "default_width")]
    pub display_width: u32,

    #[serde(default = "default_height")]
    pub display_height: u32,

    #[serde(default = "default_vnc_addr")]
    pub vnc_bind_addr: String,

    /// None means "pick a random free port".
    pub vnc_port: Option<u16>,

    pub app_type: AppType,

    /// Required when app_type == "appimage".
    pub app_path: Option<PathBuf>,

    /// Required when app_type == "folder".
    pub app_dir: Option<PathBuf>,

    /// Required when app_type == "folder". Relative to app_dir inside the container.
    pub entrypoint: Option<String>,

    #[serde(default = "default_timeout")]
    pub startup_timeout_seconds: u64,

    #[serde(default)]
    pub electron: bool,

    /// Container memory limit in bytes. Default: 4 GB.
    pub container_memory_bytes: Option<i64>,

    /// Container CPU limit in nano-CPUs. Default: 4 cores (4_000_000_000).
    pub container_nano_cpus: Option<i64>,

    /// Container PID limit. Default: 512.
    pub container_pids_limit: Option<i64>,

    /// Notification integrations (Slack, etc.).
    #[serde(default)]
    pub integrations: IntegrationsConfig,
}

/// Configuration for notification integrations.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct IntegrationsConfig {
    /// Slack webhook integration.
    #[serde(default)]
    pub slack: Option<SlackConfig>,
}

/// Slack Incoming Webhook configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SlackConfig {
    /// Webhook URL. Can also be set via `DESKTEST_SLACK_WEBHOOK_URL` env var.
    pub webhook_url: Option<String>,
    /// Channel override (optional — the webhook URL already targets a default channel).
    pub channel: Option<String>,
}

impl Config {
    /// Create a Config with sensible defaults for task-based runs.
    ///
    /// Used when `desktest run <task.json>` is invoked without a separate config file.
    /// API key and provider are resolved from environment variables at provider creation time.
    pub fn from_task_defaults() -> Self {
        Config {
            api_key: String::new(),
            api_key_source: None,
            provider: default_provider(),
            model: default_model(),
            llm_max_retries: default_llm_max_retries(),
            api_base_url: default_base_url(),
            display_width: default_width(),
            display_height: default_height(),
            vnc_bind_addr: default_vnc_addr(),
            vnc_port: None,
            app_type: AppType::Folder,
            app_path: None,
            app_dir: None,
            entrypoint: None,
            startup_timeout_seconds: default_timeout(),
            electron: false,
            container_memory_bytes: None,
            container_nano_cpus: None,
            container_pids_limit: None,
            integrations: IntegrationsConfig::default(),
        }
    }

    /// Populate app-related config fields from a task definition's AppConfig.
    ///
    /// When running via `desktest run <task.json>` without a separate config file,
    /// the Config starts with default/None app fields. This method fills them
    /// from the task definition so that `deploy_app()` works correctly.
    pub fn apply_task_app(&mut self, app: &crate::task::AppConfig) {
        match app {
            crate::task::AppConfig::Appimage { path, electron } => {
                self.app_type = AppType::Appimage;
                self.app_path = Some(PathBuf::from(path));
                self.electron = *electron;
            }
            crate::task::AppConfig::Folder {
                dir,
                entrypoint,
                electron,
            } => {
                self.app_type = AppType::Folder;
                self.app_dir = Some(PathBuf::from(dir));
                self.entrypoint = Some(entrypoint.clone());
                self.electron = *electron;
            }
            crate::task::AppConfig::DockerImage { .. } => {
                self.app_type = AppType::DockerImage;
                self.electron = false;
            }
            crate::task::AppConfig::VncAttach { .. } => {
                self.app_type = AppType::VncAttach;
                self.electron = false;
            }
            crate::task::AppConfig::MacosTart { electron, .. } => {
                self.app_type = AppType::MacosTart;
                self.electron = *electron;
            }
            crate::task::AppConfig::MacosNative { .. } => {
                self.app_type = AppType::MacosNative;
                self.electron = false;
            }
        }
    }

    /// Load and validate configuration from a JSON file.
    pub fn load_and_validate(path: &Path) -> Result<Self, AppError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Cannot read config file: {e}")))?;

        Self::parse_and_validate(&contents)
    }

    /// Parse JSON string and validate cross-field constraints.
    pub fn parse_and_validate(json: &str) -> Result<Self, AppError> {
        let config: Config = serde_json::from_str(json)
            .map_err(|e| AppError::Config(format!("Invalid JSON: {e}")))?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), AppError> {
        match self.app_type {
            AppType::Appimage => {
                let app_path = self.app_path.as_ref().ok_or_else(|| {
                    AppError::Config("app_path is required when app_type is \"appimage\"".into())
                })?;

                if !app_path.exists() {
                    return Err(AppError::Config(format!(
                        "app_path does not exist: {}",
                        app_path.display()
                    )));
                }
            }
            AppType::Folder => {
                let app_dir = self.app_dir.as_ref().ok_or_else(|| {
                    AppError::Config("app_dir is required when app_type is \"folder\"".into())
                })?;

                if !app_dir.exists() {
                    return Err(AppError::Config(format!(
                        "app_dir does not exist: {}",
                        app_dir.display()
                    )));
                }

                if self.entrypoint.is_none() {
                    return Err(AppError::Config(
                        "entrypoint is required when app_type is \"folder\"".into(),
                    ));
                }
            }
            AppType::DockerImage => {
                // No local file validation needed — image is pulled/used at container creation time
            }
            AppType::VncAttach => {
                // No validation needed — container is managed externally
            }
            AppType::MacosTart => {
                // Validation happens in task.rs (base_image + launch method check)
            }
            AppType::MacosNative => {
                // Validation happens in task.rs (bundle_id or app_path check)
            }
        }

        if self.vnc_bind_addr.parse::<std::net::IpAddr>().is_err() {
            return Err(AppError::Config(format!(
                "vnc_bind_addr is not a valid IP address: {:?}",
                self.vnc_bind_addr
            )));
        }

        if let Some(port) = self.vnc_port {
            if port == 0 {
                return Err(AppError::Config("vnc_port must be > 0".into()));
            }
        }

        if self.display_width == 0 || self.display_height == 0 {
            return Err(AppError::Config(
                "display_width and display_height must be > 0".into(),
            ));
        }

        Ok(())
    }
}

/// Format a host:port string, wrapping IPv6 addresses in brackets.
pub fn format_host_port(addr: &str, port: u16) -> String {
    if addr.contains(':') {
        format!("[{addr}]:{port}")
    } else {
        format!("{addr}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_temp_appimage() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.AppImage");
        std::fs::write(&file, b"fake").unwrap();
        (dir, file)
    }

    fn make_temp_folder_app() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let app_dir = dir.path().join("myapp");
        std::fs::create_dir(&app_dir).unwrap();
        std::fs::write(app_dir.join("run.sh"), b"#!/bin/sh\necho hi").unwrap();
        (dir, app_dir)
    }

    #[test]
    fn test_valid_appimage_config() {
        let (_tmp, app_path) = make_temp_appimage();
        let json = format!(
            r#"{{
                "api_key": "sk-test",
                "app_type": "appimage",
                "app_path": "{}"
            }}"#,
            app_path.display()
        );
        let config = Config::parse_and_validate(&json).unwrap();
        assert_eq!(config.app_type, AppType::Appimage);
        assert_eq!(config.api_key, "sk-test");
        assert_eq!(config.app_path.unwrap(), app_path);
    }

    #[test]
    fn test_valid_folder_config() {
        let (_tmp, app_dir) = make_temp_folder_app();
        let json = format!(
            r#"{{
                "api_key": "sk-test",
                "app_type": "folder",
                "app_dir": "{}",
                "entrypoint": "run.sh"
            }}"#,
            app_dir.display()
        );
        let config = Config::parse_and_validate(&json).unwrap();
        assert_eq!(config.app_type, AppType::Folder);
        assert_eq!(config.entrypoint.unwrap(), "run.sh");
    }

    #[test]
    fn test_defaults_applied() {
        let (_tmp, app_path) = make_temp_appimage();
        let json = format!(
            r#"{{
                "api_key": "sk-test",
                "app_type": "appimage",
                "app_path": "{}"
            }}"#,
            app_path.display()
        );
        let config = Config::parse_and_validate(&json).unwrap();
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert_eq!(config.llm_max_retries, 5);
        assert_eq!(config.display_width, 1920);
        assert_eq!(config.display_height, 1080);
        assert_eq!(config.vnc_bind_addr, "127.0.0.1");
        assert!(config.vnc_port.is_none());
        assert_eq!(config.startup_timeout_seconds, 30);
    }

    #[test]
    fn test_missing_api_key_defaults_to_empty() {
        let (_tmp, app_path) = make_temp_appimage();
        let json = format!(
            r#"{{
                "app_type": "appimage",
                "app_path": "{}"
            }}"#,
            app_path.display()
        );
        let config = Config::parse_and_validate(&json).unwrap();
        assert_eq!(config.api_key, "");
    }

    #[test]
    fn test_provider_defaults_to_openai() {
        let (_tmp, app_path) = make_temp_appimage();
        let json = format!(
            r#"{{
                "api_key": "sk-test",
                "app_type": "appimage",
                "app_path": "{}"
            }}"#,
            app_path.display()
        );
        let config = Config::parse_and_validate(&json).unwrap();
        assert_eq!(config.provider, "anthropic");
    }

    #[test]
    fn test_provider_custom_value() {
        let (_tmp, app_path) = make_temp_appimage();
        let json = format!(
            r#"{{
                "api_key": "sk-test",
                "provider": "anthropic",
                "app_type": "appimage",
                "app_path": "{}"
            }}"#,
            app_path.display()
        );
        let config = Config::parse_and_validate(&json).unwrap();
        assert_eq!(config.provider, "anthropic");
    }

    #[test]
    fn test_missing_app_path_for_appimage() {
        let json = r#"{"api_key": "sk-test", "app_type": "appimage"}"#;
        let err = Config::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("app_path is required"));
    }

    #[test]
    fn test_missing_entrypoint_for_folder() {
        let (_tmp, app_dir) = make_temp_folder_app();
        let json = format!(
            r#"{{
                "api_key": "sk-test",
                "app_type": "folder",
                "app_dir": "{}"
            }}"#,
            app_dir.display()
        );
        let err = Config::parse_and_validate(&json).unwrap_err();
        assert!(err.to_string().contains("entrypoint is required"));
    }

    #[test]
    fn test_missing_app_dir_for_folder() {
        let json = r#"{"api_key": "sk-test", "app_type": "folder", "entrypoint": "run.sh"}"#;
        let err = Config::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("app_dir is required"));
    }

    #[test]
    fn test_invalid_json() {
        let err = Config::parse_and_validate("not json at all").unwrap_err();
        assert!(matches!(err, AppError::Config(_)));
        assert!(err.to_string().contains("Invalid JSON"));
    }

    #[test]
    fn test_app_path_not_found() {
        let json = r#"{"api_key": "sk-test", "app_type": "appimage", "app_path": "/nonexistent/app.AppImage"}"#;
        let err = Config::parse_and_validate(json).unwrap_err();
        assert!(err.to_string().contains("app_path does not exist"));
    }

    #[test]
    fn test_vnc_port_zero() {
        let (_tmp, app_path) = make_temp_appimage();
        let json = format!(
            r#"{{
                "api_key": "sk-test",
                "app_type": "appimage",
                "app_path": "{}",
                "vnc_port": 0
            }}"#,
            app_path.display()
        );
        let err = Config::parse_and_validate(&json).unwrap_err();
        assert!(err.to_string().contains("vnc_port must be > 0"));
    }

    #[test]
    fn test_vnc_bind_addr_invalid() {
        let json = r#"{
            "api_key": "sk-test",
            "app_type": "docker_image",
            "vnc_bind_addr": "not-an-ip"
        }"#;
        let err = Config::parse_and_validate(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("vnc_bind_addr is not a valid IP address")
        );
    }

    #[test]
    fn test_vnc_bind_addr_empty() {
        let json = r#"{
            "api_key": "sk-test",
            "app_type": "docker_image",
            "vnc_bind_addr": ""
        }"#;
        let err = Config::parse_and_validate(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("vnc_bind_addr is not a valid IP address")
        );
    }

    #[test]
    fn test_vnc_bind_addr_valid_ipv4() {
        let json = r#"{
            "api_key": "sk-test",
            "app_type": "docker_image",
            "vnc_bind_addr": "0.0.0.0"
        }"#;
        let config = Config::parse_and_validate(json).unwrap();
        assert_eq!(config.vnc_bind_addr, "0.0.0.0");
    }

    #[test]
    fn test_vnc_bind_addr_valid_ipv6() {
        let json = r#"{
            "api_key": "sk-test",
            "app_type": "docker_image",
            "vnc_bind_addr": "::1"
        }"#;
        let config = Config::parse_and_validate(json).unwrap();
        assert_eq!(config.vnc_bind_addr, "::1");
    }

    #[test]
    fn test_format_host_port_ipv4() {
        assert_eq!(format_host_port("127.0.0.1", 5900), "127.0.0.1:5900");
        assert_eq!(format_host_port("0.0.0.0", 8080), "0.0.0.0:8080");
    }

    #[test]
    fn test_format_host_port_ipv6() {
        assert_eq!(format_host_port("::1", 5900), "[::1]:5900");
        assert_eq!(format_host_port("::0", 7860), "[::0]:7860");
    }

    #[test]
    fn test_valid_docker_image_config() {
        let json = r#"{
            "api_key": "sk-test",
            "app_type": "docker_image"
        }"#;
        let config = Config::parse_and_validate(json).unwrap();
        assert_eq!(config.app_type, AppType::DockerImage);
    }

    #[test]
    fn test_docker_image_no_app_path_required() {
        // DockerImage type should not require app_path or app_dir
        let json = r#"{
            "api_key": "sk-test",
            "app_type": "docker_image"
        }"#;
        let config = Config::parse_and_validate(json).unwrap();
        assert!(config.app_path.is_none());
        assert!(config.app_dir.is_none());
    }

    #[test]
    fn test_from_task_defaults() {
        let config = Config::from_task_defaults();
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert_eq!(config.llm_max_retries, 5);
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_custom_llm_max_retries() {
        let json = r#"{
            "api_key": "sk-test",
            "app_type": "docker_image",
            "llm_max_retries": 7
        }"#;
        let config = Config::parse_and_validate(json).unwrap();
        assert_eq!(config.llm_max_retries, 7);
    }
}
