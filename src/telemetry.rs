use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::debug;

const DEFAULT_TELEMETRY_URL: &str = "https://telemetry.desktest.dev";
const STATE_FILE_NAME: &str = "telemetry.json";
const NUDGE_INTERVAL: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentLevel {
    None,
    Anonymous,
    Rich,
}

impl std::fmt::Display for ConsentLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsentLevel::None => write!(f, "none"),
            ConsentLevel::Anonymous => write!(f, "anonymous"),
            ConsentLevel::Rich => write!(f, "rich"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    pub consent_level: ConsentLevel,
    pub install_id: String,
    pub prompted_at: Option<String>,
    pub run_count_since_prompt: u32,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            consent_level: ConsentLevel::None,
            install_id: uuid::Uuid::new_v4().to_string(),
            prompted_at: None,
            run_count_since_prompt: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryEvent {
    pub timestamp: String,
    pub desktest_version: String,
    pub install_id: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluator_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_steps: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
    pub used_qa: bool,
    pub used_replay: bool,
    pub used_bash: bool,
    pub platform: String,
    pub command: String,
}

pub struct TelemetryClient {
    config: TelemetryConfig,
    events: Vec<TelemetryEvent>,
    base_url: String,
    /// Whether telemetry level was forced via env var (skip prompts/nudges).
    env_override: bool,
    /// Path to the artifacts directory (for rich uploads).
    artifacts_dir: Option<PathBuf>,
}

impl TelemetryClient {
    /// Load telemetry state from disk or create a fresh config.
    pub fn load_or_init() -> Self {
        let env_override = std::env::var("DESKTEST_TELEMETRY").ok();
        let base_url = std::env::var("DESKTEST_TELEMETRY_URL")
            .unwrap_or_else(|_| DEFAULT_TELEMETRY_URL.to_string());

        let existing = load_config();
        let was_fresh = existing.is_none();
        let mut config = existing.unwrap_or_default();
        let mut is_env_override = false;

        // Persist the config if freshly generated (or reset after corruption),
        // so install_id is stable across runs.
        // Done before env override to avoid leaking an env-overridden consent level to disk.
        if was_fresh {
            let _ = save_config(&config);
        }

        if let Some(val) = env_override {
            is_env_override = true;
            config.consent_level = match val.as_str() {
                "0" => ConsentLevel::None,
                "1" => ConsentLevel::Anonymous,
                "2" => ConsentLevel::Rich,
                other => {
                    eprintln!("Warning: unrecognized DESKTEST_TELEMETRY value '{other}'. Expected 0, 1, or 2. Defaulting to off.");
                    ConsentLevel::None
                }
            };
        }

        Self {
            config,
            events: Vec::new(),
            base_url,
            env_override: is_env_override,
            artifacts_dir: None,
        }
    }

    /// Check consent state: prompt on first run, nudge periodically.
    /// Only call for test commands (run/suite/attach/interactive).
    pub fn check_consent(&mut self) {
        if self.env_override {
            return;
        }

        // If stdin is not a TTY, skip prompting
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            return;
        }

        if self.config.prompted_at.is_none() {
            self.show_first_run_prompt();
        } else if self.should_nudge() {
            self.show_nudge();
        }

        // Only increment and persist for users who can be nudged
        // (not Rich, and not explicitly opted out via `desktest telemetry off`)
        let can_be_nudged = self.config.consent_level != ConsentLevel::Rich
            && !(self.config.consent_level == ConsentLevel::None && self.config.prompted_at.is_some());
        if can_be_nudged {
            self.config.run_count_since_prompt += 1;
            let _ = save_config(&self.config);
        }
    }

    /// Handle the `desktest telemetry <action>` subcommand.
    pub fn handle_command(&mut self, action: &crate::cli::TelemetryAction) {
        use crate::cli::TelemetryAction;
        match action {
            TelemetryAction::Off => {
                self.config.consent_level = ConsentLevel::None;
                self.config.prompted_at = Some(now_iso8601());
                self.config.run_count_since_prompt = 0;
                let _ = save_config(&self.config);
                eprintln!("Telemetry disabled.");
            }
            TelemetryAction::Anonymous => {
                self.config.consent_level = ConsentLevel::Anonymous;
                self.config.prompted_at = Some(now_iso8601());
                self.config.run_count_since_prompt = 0;
                let _ = save_config(&self.config);
                eprintln!("Telemetry set to anonymous (usage stats only).");
            }
            TelemetryAction::Rich => {
                self.config.consent_level = ConsentLevel::Rich;
                self.config.prompted_at = Some(now_iso8601());
                self.config.run_count_since_prompt = 0;
                let _ = save_config(&self.config);
                eprintln!("Telemetry set to rich (usage stats + trajectories & screenshots).");
            }
            TelemetryAction::Status => {
                println!("Telemetry status:");
                println!("  Consent level:    {}", self.config.consent_level);
                println!("  Install ID:       {}", self.config.install_id);
                println!(
                    "  Runs since prompt: {}",
                    self.config.run_count_since_prompt
                );
                if let Some(ref ts) = self.config.prompted_at {
                    println!("  Last prompted:    {ts}");
                }
            }
        }
    }

    /// Record a telemetry event (stored in memory until flush).
    pub fn record_event(&mut self, event: TelemetryEvent) {
        if self.config.consent_level == ConsentLevel::None {
            return;
        }
        self.events.push(event);
    }

    /// Set the artifacts directory for rich uploads.
    pub fn set_artifacts_dir(&mut self, dir: PathBuf) {
        self.artifacts_dir = Some(dir);
    }

    /// Current consent level.
    pub fn consent_level(&self) -> ConsentLevel {
        self.config.consent_level
    }

    /// Install ID for event creation.
    pub fn install_id(&self) -> &str {
        &self.config.install_id
    }

    /// Flush all queued events to the backend. Fire-and-forget with timeout.
    pub async fn flush(&mut self) {
        if self.config.consent_level == ConsentLevel::None {
            return;
        }

        let has_events = !self.events.is_empty();
        let has_artifacts = self.config.consent_level == ConsentLevel::Rich
            && self.artifacts_dir.as_ref().is_some_and(|d| d.exists());

        if !has_events && !has_artifacts {
            return;
        }

        let client = reqwest::Client::new();

        // Send anonymous events (if any were queued)
        if has_events {
            let url = format!("{}/api/events", self.base_url);
            let events = std::mem::take(&mut self.events);
            let payload = serde_json::json!({ "events": events });
            let send_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client.post(&url).json(&payload).send(),
            )
            .await;

            match send_result {
                Ok(Ok(resp)) => {
                    debug!("Telemetry events sent: status {}", resp.status());
                }
                Ok(Err(e)) => {
                    debug!("Telemetry send failed (ignored): {e}");
                }
                Err(_) => {
                    debug!("Telemetry send timed out (ignored)");
                }
            }
        }

        // Rich tier: upload artifacts tarball (independent of events)
        if self.config.consent_level == ConsentLevel::Rich {
            if let Some(ref artifacts_dir) = self.artifacts_dir {
                if artifacts_dir.exists() {
                    self.upload_artifacts(&client, artifacts_dir).await;
                }
            }
        }
    }

    fn should_nudge(&self) -> bool {
        if self.config.consent_level == ConsentLevel::Rich {
            return false;
        }
        // Don't nudge users who explicitly opted out via `desktest telemetry off`
        if self.config.consent_level == ConsentLevel::None && self.config.prompted_at.is_some() {
            return false;
        }
        // +1 accounts for the increment that happens after this check in check_consent()
        let next_count = self.config.run_count_since_prompt + 1;
        next_count % NUDGE_INTERVAL == 0
    }

    fn show_first_run_prompt(&mut self) {
        eprintln!();
        eprintln!("desktest would like to collect usage data to improve the tool.");
        eprintln!();
        eprintln!("  [1] Anonymous stats only \u{2014} test outcomes, duration, error types");
        eprintln!("  [2] Rich diagnostics    \u{2014} also includes trajectories & screenshots");
        eprintln!("  [n] No telemetry");
        eprintln!();
        eprintln!(
            "API keys are NEVER collected. Learn more: https://github.com/Edison-Watch/desktest/wiki/telemetry"
        );
        eprintln!();
        eprint!("Your choice [1/2/n]: ");

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            self.config.consent_level = ConsentLevel::None;
        } else {
            let trimmed = input.trim().to_lowercase();
            self.config.consent_level = match trimmed.as_str() {
                "1" => ConsentLevel::Anonymous,
                "2" => ConsentLevel::Rich,
                _ => ConsentLevel::None,
            };
        }

        self.config.prompted_at = Some(now_iso8601());
        self.config.run_count_since_prompt = 0;
        let _ = save_config(&self.config);

        match self.config.consent_level {
            ConsentLevel::None => eprintln!("No telemetry. You can change this later with `desktest telemetry anonymous` or `desktest telemetry rich`."),
            ConsentLevel::Anonymous => eprintln!("Thanks! Anonymous telemetry enabled. Change anytime with `desktest telemetry off`."),
            ConsentLevel::Rich => eprintln!("Thanks! Rich telemetry enabled. Change anytime with `desktest telemetry off`."),
        }
        eprintln!();
    }

    fn show_nudge(&self) {
        // Only Anonymous users reach here — Rich and None+prompted are
        // excluded by should_nudge(), None+unprompted goes to first-run prompt
        eprintln!(
            "Tip: Share richer diagnostics with `desktest telemetry rich`"
        );
    }

    async fn upload_artifacts(&self, client: &reqwest::Client, artifacts_dir: &std::path::Path) {
        let tarball = match create_artifacts_tarball(artifacts_dir) {
            Ok(data) => data,
            Err(e) => {
                debug!("Failed to create artifacts tarball: {e}");
                return;
            }
        };

        let url = format!("{}/api/upload", self.base_url);
        let run_id = uuid::Uuid::new_v4().to_string();
        let part = reqwest::multipart::Part::bytes(tarball)
            .file_name("artifacts.tar.gz")
            .mime_str("application/gzip")
            .expect("hardcoded MIME type is always valid");

        let form = reqwest::multipart::Form::new()
            .part("archive", part);

        let send_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client
                .post(&url)
                .header("X-Install-Id", &self.config.install_id)
                .header("X-Run-Id", &run_id)
                .multipart(form)
                .send(),
        )
        .await;

        match send_result {
            Ok(Ok(resp)) => {
                debug!("Artifacts uploaded: status {}", resp.status());
            }
            Ok(Err(e)) => {
                debug!("Artifacts upload failed (ignored): {e}");
            }
            Err(_) => {
                debug!("Artifacts upload timed out (ignored)");
            }
        }
    }
}

/// Build a TelemetryEvent with common fields pre-filled.
pub fn build_event(client: &TelemetryClient, event_type: &str, command: &str) -> TelemetryEvent {
    TelemetryEvent {
        timestamp: now_iso8601(),
        desktest_version: env!("CARGO_PKG_VERSION").to_string(),
        install_id: client.install_id().to_string(),
        event_type: event_type.to_string(),
        app_type: None,
        evaluator_mode: None,
        provider: None,
        model: None,
        status: None,
        duration_ms: None,
        agent_steps: None,
        error_category: None,
        used_qa: false,
        used_replay: false,
        used_bash: false,
        platform: std::env::consts::OS.to_string(),
        command: command.to_string(),
    }
}

// --- Config persistence ---

fn config_dir() -> Option<PathBuf> {
    dirs_path().map(|p| {
        let _ = std::fs::create_dir_all(&p);
        p
    })
}

fn dirs_path() -> Option<PathBuf> {
    // Use XDG on Linux, ~/Library/Application Support on macOS
    if cfg!(target_os = "macos") {
        dirs_home().map(|h| h.join("Library/Application Support/desktest"))
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| dirs_home().map(|h| h.join(".config")))
            .map(|p| p.join("desktest"))
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn state_file_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(STATE_FILE_NAME))
}

/// Load telemetry config from disk.
/// Returns Some(config) on success, None if missing or corrupt.
/// Warnings are printed to stderr for I/O errors and corrupt files.
fn load_config() -> Option<TelemetryConfig> {
    let path = state_file_path()?;
    let data = match std::fs::read_to_string(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!(
                "Warning: cannot read telemetry config at '{}' ({}), resetting. Your install_id will change.",
                path.display(),
                e
            );
            return None;
        }
    };
    match serde_json::from_str(&data) {
        Ok(config) => Some(config),
        Err(e) => {
            eprintln!(
                "Warning: telemetry config at '{}' is corrupt ({}), resetting. Your install_id will change.",
                path.display(),
                e
            );
            None
        }
    }
}

fn save_config(config: &TelemetryConfig) -> Result<(), std::io::Error> {
    let path = match state_file_path() {
        Some(p) => p,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Cannot determine config directory",
            ))
        }
    };
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    // Atomic write: write to temp file then rename to avoid corruption on mid-write crash
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp_path);
    })
}

fn now_iso8601() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let (y, mo, d) = epoch_days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).
fn epoch_days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d)
}

/// Create a gzipped tarball of the artifacts directory.
/// Excludes potentially sensitive files (home directory contents, logs).
///
/// Note: Only includes files in the top-level directory (non-recursive).
/// The desktest artifacts directory is flat by design — trajectory.jsonl,
/// screenshots, task.json, and a11y trees are all written directly into it.
/// Suite runs use separate per-test artifact directories.
fn create_artifacts_tarball(artifacts_dir: &std::path::Path) -> Result<Vec<u8>, std::io::Error> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs;

    let buf = Vec::new();
    let encoder = GzEncoder::new(buf, Compression::fast());
    let mut archive = tar::Builder::new(encoder);

    // Only include known safe file types
    let allowed_extensions = ["jsonl", "json", "png", "jpg", "txt"];

    match fs::read_dir(artifacts_dir) {
        Err(e) => {
            debug!("Cannot read artifacts directory '{}': {e}", artifacts_dir.display());
            let encoder = archive.into_inner()?;
            return encoder.finish();
        }
        Ok(entries) => {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !allowed_extensions.contains(&ext.as_str()) {
                continue;
            }
            let file_name = match path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };

            archive.append_path_with_name(&path, &file_name)?;
        }
        }
    }

    let encoder = archive.into_inner()?;
    encoder.finish()
}

/// Returns true if a CLI command is a test command that should trigger consent checks.
pub fn is_test_command(command: &crate::cli::Command) -> bool {
    matches!(
        command,
        crate::cli::Command::Run { .. }
            | crate::cli::Command::Suite { .. }
            | crate::cli::Command::Attach { .. }
            | crate::cli::Command::Interactive { .. }
    )
}

/// Extract the command name string for telemetry events.
pub fn command_name(command: &crate::cli::Command) -> &'static str {
    match command {
        crate::cli::Command::Run { .. } => "run",
        crate::cli::Command::Suite { .. } => "suite",
        crate::cli::Command::Attach { .. } => "attach",
        crate::cli::Command::Interactive { .. } => "interactive",
        crate::cli::Command::Validate { .. } => "validate",
        crate::cli::Command::Codify { .. } => "codify",
        crate::cli::Command::Replay { .. } => "replay",
        crate::cli::Command::Logs { .. } => "logs",
        crate::cli::Command::Doctor => "doctor",
        crate::cli::Command::Update { .. } => "update",
        crate::cli::Command::Monitor { .. } => "monitor",
        crate::cli::Command::Review { .. } => "review",
        crate::cli::Command::Telemetry { .. } => "telemetry",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consent_level_display() {
        assert_eq!(ConsentLevel::None.to_string(), "none");
        assert_eq!(ConsentLevel::Anonymous.to_string(), "anonymous");
        assert_eq!(ConsentLevel::Rich.to_string(), "rich");
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = TelemetryConfig {
            consent_level: ConsentLevel::Anonymous,
            install_id: "test-uuid".to_string(),
            prompted_at: Some("2025-03-25T10:13:20Z".to_string()),
            run_count_since_prompt: 5,
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: TelemetryConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.consent_level, ConsentLevel::Anonymous);
        assert_eq!(parsed.install_id, "test-uuid");
        assert_eq!(parsed.run_count_since_prompt, 5);
    }

    #[test]
    fn test_config_deserialization_from_json() {
        let json = r#"{
            "consent_level": "rich",
            "install_id": "abc-123",
            "prompted_at": "2000-01-01T00:00:00Z",
            "run_count_since_prompt": 3
        }"#;
        let config: TelemetryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.consent_level, ConsentLevel::Rich);
        assert_eq!(config.install_id, "abc-123");
    }

    #[test]
    fn test_default_config() {
        let config = TelemetryConfig::default();
        assert_eq!(config.consent_level, ConsentLevel::None);
        assert!(!config.install_id.is_empty());
        assert!(config.prompted_at.is_none());
        assert_eq!(config.run_count_since_prompt, 0);
    }

    #[test]
    fn test_should_nudge_logic() {
        let mut client = TelemetryClient {
            config: TelemetryConfig {
                consent_level: ConsentLevel::Anonymous,
                install_id: "test".to_string(),
                prompted_at: Some("2025-01-01T00:00:00Z".to_string()),
                run_count_since_prompt: 0,
            },
            events: Vec::new(),
            base_url: "http://localhost".to_string(),
            env_override: false,
            artifacts_dir: None,
        };

        // run_count 0 → no nudge (next_count=1, 1%10!=0)
        assert!(!client.should_nudge());

        // run_count 9 → nudge (next_count=10, 10%10==0)
        client.config.run_count_since_prompt = 9;
        assert!(client.should_nudge());

        // run_count 19 → nudge (next_count=20)
        client.config.run_count_since_prompt = 19;
        assert!(client.should_nudge());

        // run_count 10 → no nudge (next_count=11)
        client.config.run_count_since_prompt = 10;
        assert!(!client.should_nudge());

        // run_count 5 → no nudge
        client.config.run_count_since_prompt = 5;
        assert!(!client.should_nudge());

        // Rich consent → never nudge
        client.config.consent_level = ConsentLevel::Rich;
        client.config.run_count_since_prompt = 9;
        assert!(!client.should_nudge());

        // Explicit opt-out (None + prompted_at set) → never nudge
        client.config.consent_level = ConsentLevel::None;
        client.config.prompted_at = Some("2025-01-01T00:00:00Z".to_string());
        client.config.run_count_since_prompt = 9;
        assert!(!client.should_nudge());
    }

    #[test]
    fn test_record_event_respects_consent() {
        let mut client = TelemetryClient {
            config: TelemetryConfig {
                consent_level: ConsentLevel::None,
                install_id: "test".to_string(),
                prompted_at: None,
                run_count_since_prompt: 0,
            },
            events: Vec::new(),
            base_url: "http://localhost".to_string(),
            env_override: false,
            artifacts_dir: None,
        };

        let event = build_event(&client, "test_completed", "run");
        client.record_event(event);
        assert!(client.events.is_empty(), "Should not record when consent is None");

        client.config.consent_level = ConsentLevel::Anonymous;
        let event = build_event(&client, "test_completed", "run");
        client.record_event(event);
        assert_eq!(client.events.len(), 1);
    }

    #[test]
    fn test_event_serialization() {
        let event = TelemetryEvent {
            timestamp: "2025-03-25T10:13:20Z".to_string(),
            desktest_version: "0.12.0".to_string(),
            install_id: "test-id".to_string(),
            event_type: "test_completed".to_string(),
            app_type: Some("appimage".to_string()),
            evaluator_mode: None,
            provider: Some("anthropic".to_string()),
            model: Some("claude-sonnet-4-5-20250929".to_string()),
            status: Some("pass".to_string()),
            duration_ms: Some(5000),
            agent_steps: Some(10),
            error_category: None,
            used_qa: false,
            used_replay: false,
            used_bash: true,
            platform: "linux".to_string(),
            command: "run".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("test_completed"));
        assert!(json.contains("appimage"));
        // None fields should be skipped
        assert!(!json.contains("evaluator_mode"));
        assert!(!json.contains("error_category"));
    }

    #[test]
    fn test_build_event_fills_common_fields() {
        let client = TelemetryClient {
            config: TelemetryConfig {
                consent_level: ConsentLevel::Anonymous,
                install_id: "my-id".to_string(),
                prompted_at: None,
                run_count_since_prompt: 0,
            },
            events: Vec::new(),
            base_url: "http://localhost".to_string(),
            env_override: false,
            artifacts_dir: None,
        };

        let event = build_event(&client, "test_completed", "run");
        assert_eq!(event.install_id, "my-id");
        assert_eq!(event.event_type, "test_completed");
        assert_eq!(event.command, "run");
        assert_eq!(event.desktest_version, env!("CARGO_PKG_VERSION"));
        assert!(!event.timestamp.is_empty());
    }

    #[test]
    fn test_now_iso8601_format() {
        let ts = now_iso8601();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert!(
            ts.len() == 20,
            "Expected 20-char ISO 8601, got {ts} (len {})",
            ts.len()
        );
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn test_epoch_days_to_ymd_known_dates() {
        // 1970-01-01 = day 0
        assert_eq!(epoch_days_to_ymd(0), (1970, 1, 1));
        // 2000-01-01 = day 10957
        assert_eq!(epoch_days_to_ymd(10957), (2000, 1, 1));
        // 2025-03-25 = day 20172
        assert_eq!(epoch_days_to_ymd(20172), (2025, 3, 25));
    }
}
