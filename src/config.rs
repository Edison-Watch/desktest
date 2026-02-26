use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::AppError;

fn default_model() -> String {
    "gpt-4.1".into()
}

fn default_base_url() -> String {
    "https://api.openai.com".into()
}

fn default_width() -> u32 {
    1920
}

fn default_height() -> u32 {
    1080
}

fn default_vnc_addr() -> String {
    "0.0.0.0".into()
}

fn default_timeout() -> u64 {
    30
}

fn default_provider() -> String {
    "openai".into()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppType {
    Appimage,
    Folder,
    DockerImage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub api_key: String,

    #[serde(default = "default_provider")]
    pub provider: String,

    #[serde(default = "default_model")]
    pub model: String,

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
}

impl Config {
    /// Create a Config with sensible defaults for task-based runs.
    ///
    /// Used when `tent run <task.json>` is invoked without a separate config file.
    /// API key and provider are resolved from environment variables at provider creation time.
    pub fn from_task_defaults() -> Self {
        Config {
            api_key: String::new(),
            provider: default_provider(),
            model: default_model(),
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
        let config: Config =
            serde_json::from_str(json).map_err(|e| AppError::Config(format!("Invalid JSON: {e}")))?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), AppError> {
        match self.app_type {
            AppType::Appimage => {
                let app_path = self
                    .app_path
                    .as_ref()
                    .ok_or_else(|| AppError::Config("app_path is required when app_type is \"appimage\"".into()))?;

                if !app_path.exists() {
                    return Err(AppError::Config(format!(
                        "app_path does not exist: {}",
                        app_path.display()
                    )));
                }
            }
            AppType::Folder => {
                let app_dir = self
                    .app_dir
                    .as_ref()
                    .ok_or_else(|| AppError::Config("app_dir is required when app_type is \"folder\"".into()))?;

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
        }

        if let Some(port) = self.vnc_port {
            if port == 0 {
                return Err(AppError::Config("vnc_port must be > 0".into()));
            }
        }

        if self.display_width == 0 || self.display_height == 0 {
            return Err(AppError::Config("display_width and display_height must be > 0".into()));
        }

        Ok(())
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
        assert_eq!(config.model, "gpt-4.1");
        assert_eq!(config.display_width, 1920);
        assert_eq!(config.display_height, 1080);
        assert_eq!(config.vnc_bind_addr, "0.0.0.0");
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
        assert_eq!(config.provider, "openai");
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
        let json =
            r#"{"api_key": "sk-test", "app_type": "appimage", "app_path": "/nonexistent/app.AppImage"}"#;
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
        assert_eq!(config.provider, "openai");
        assert_eq!(config.model, "gpt-4.1");
        assert!(config.api_key.is_empty());
    }
}
