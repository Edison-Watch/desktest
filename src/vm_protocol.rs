#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestType {
    Exec,
    ExecExitCode,
    ExecStdin,
    ExecDetached,
    CopyToVm,
    CopyFromVm,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Request {
    #[serde(rename = "type")]
    pub kind: RequestType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdin_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Response {
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub exit_code: i64,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ProtocolPaths {
    pub shared_dir: PathBuf,
    pub requests_dir: PathBuf,
    pub responses_dir: PathBuf,
    pub transfers_dir: PathBuf,
    pub agent_ready_path: PathBuf,
}

impl ProtocolPaths {
    pub fn new(shared_dir: impl Into<PathBuf>) -> Self {
        let shared_dir = shared_dir.into();
        Self {
            requests_dir: shared_dir.join("requests"),
            responses_dir: shared_dir.join("responses"),
            transfers_dir: shared_dir.join("transfers"),
            agent_ready_path: shared_dir.join("agent_ready"),
            shared_dir,
        }
    }

    pub async fn ensure_layout(&self) -> Result<(), AppError> {
        tokio::fs::create_dir_all(&self.requests_dir).await?;
        tokio::fs::create_dir_all(&self.responses_dir).await?;
        tokio::fs::create_dir_all(&self.transfers_dir).await?;
        Ok(())
    }
}

/// Shared-directory IPC client for communicating with a VM guest agent.
///
/// The `label` field (e.g. "Tart VM", "Windows VM") is used in error messages
/// to identify which VM type produced the error.
#[derive(Debug, Clone)]
pub struct ProtocolClient {
    paths: ProtocolPaths,
    label: String,
    request_timeout: Duration,
    poll_interval: Duration,
}

impl ProtocolClient {
    pub fn new(shared_dir: impl Into<PathBuf>, label: impl Into<String>) -> Self {
        Self::with_timeouts(
            shared_dir,
            label,
            DEFAULT_REQUEST_TIMEOUT,
            DEFAULT_POLL_INTERVAL,
        )
    }

    pub fn with_timeouts(
        shared_dir: impl Into<PathBuf>,
        label: impl Into<String>,
        request_timeout: Duration,
        poll_interval: Duration,
    ) -> Self {
        Self {
            paths: ProtocolPaths::new(shared_dir),
            label: label.into(),
            request_timeout,
            poll_interval,
        }
    }

    pub fn paths(&self) -> &ProtocolPaths {
        &self.paths
    }

    pub async fn ensure_layout(&self) -> Result<(), AppError> {
        self.paths.ensure_layout().await
    }

    pub async fn wait_for_agent_ready(&self, timeout: Duration) -> Result<(), AppError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if tokio::fs::try_exists(&self.paths.agent_ready_path).await? {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(AppError::Infra(format!(
                    "Timed out waiting for {} agent sentinel at {}",
                    self.label,
                    self.paths.agent_ready_path.display()
                )));
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    pub async fn send_request(&self, request: &Request) -> Result<Response, AppError> {
        self.ensure_layout().await?;

        let request_id = next_request_id();
        let request_path = self
            .paths
            .requests_dir
            .join(format!("cmd_{request_id}.json"));
        let response_path = self
            .paths
            .responses_dir
            .join(format!("cmd_{request_id}.result.json"));

        let payload = serde_json::to_vec_pretty(request).map_err(|e| {
            AppError::Infra(format!("Failed to serialize {} request: {e}", self.label))
        })?;
        let tmp_path = request_path.with_extension("tmp");
        tokio::fs::write(&tmp_path, &payload).await?;
        tokio::fs::rename(&tmp_path, &request_path).await?;

        let deadline = tokio::time::Instant::now() + self.request_timeout;
        loop {
            if tokio::fs::try_exists(&response_path).await? {
                let bytes = tokio::fs::read(&response_path).await?;

                // Clean up request and response files before parsing so they
                // don't leak if deserialization fails.
                let _ = tokio::fs::remove_file(&request_path).await;
                let _ = tokio::fs::remove_file(&response_path).await;

                let response: Response = serde_json::from_slice(&bytes).map_err(|e| {
                    AppError::Infra(format!(
                        "Failed to parse {} response {}: {e}",
                        self.label,
                        response_path.display()
                    ))
                })?;

                if let Some(error) = response.error.clone() {
                    return Err(AppError::Infra(format!(
                        "{} agent error: {error}",
                        self.label
                    )));
                }
                return Ok(response);
            }

            if tokio::time::Instant::now() >= deadline {
                let _ = tokio::fs::remove_file(&request_path).await;
                return Err(AppError::Infra(format!(
                    "Timed out waiting for {} response to {}",
                    self.label,
                    request_path.display()
                )));
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    pub fn transfer_stage(&self, request_id: &str) -> PathBuf {
        self.paths.transfers_dir.join(request_id)
    }
}

pub fn next_request_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", std::process::id(), ts, counter)
}

pub fn relative_transfer_path(base: &Path, path: &Path) -> Result<String, AppError> {
    let relative = path.strip_prefix(base).map_err(|e| {
        AppError::Infra(format!(
            "Transfer path {} is not inside shared dir {}: {e}",
            path.display(),
            base.display()
        ))
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_serializes_snake_case_type() {
        let request = Request {
            kind: RequestType::ExecStdin,
            cmd: Some(vec!["python3".into(), "-c".into(), "print('hi')".into()]),
            stdin_b64: Some("aGVsbG8=".into()),
            src_path: None,
            dest_path: None,
            transfer_path: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"type\":\"exec_stdin\""));

        let decoded: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    /// Helper to create a ProtocolClient with the "Test VM" label for tests.
    fn test_client(shared_dir: &Path, timeout: Duration, poll: Duration) -> ProtocolClient {
        ProtocolClient::with_timeouts(shared_dir, "Test VM", timeout, poll)
    }

    #[tokio::test]
    async fn send_request_reads_response_file() {
        let temp = tempfile::tempdir().unwrap();
        let client = test_client(
            temp.path(),
            Duration::from_secs(30),
            Duration::from_millis(25),
        );
        client.ensure_layout().await.unwrap();

        let paths = client.paths().clone();
        tokio::spawn(async move {
            loop {
                let mut entries = tokio::fs::read_dir(&paths.requests_dir).await.unwrap();
                if let Some(entry) = entries.next_entry().await.unwrap() {
                    let request_name = entry.file_name().to_string_lossy().to_string();
                    let response_name = request_name.replace(".json", ".result.json");
                    let response_path = paths.responses_dir.join(response_name);
                    let response = Response {
                        stdout: "hello\n".into(),
                        exit_code: 0,
                        error: None,
                        duration_ms: 5,
                    };
                    tokio::fs::write(response_path, serde_json::to_vec(&response).unwrap())
                        .await
                        .unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let response = client
            .send_request(&Request {
                kind: RequestType::Exec,
                cmd: Some(vec!["echo".into(), "hello".into()]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap();

        assert_eq!(response.stdout, "hello\n");
        assert_eq!(response.exit_code, 0);
    }

    #[tokio::test]
    async fn send_request_returns_error_on_malformed_response() {
        let temp = tempfile::tempdir().unwrap();
        let client = test_client(
            temp.path(),
            Duration::from_secs(30),
            Duration::from_millis(25),
        );
        client.ensure_layout().await.unwrap();

        let paths = client.paths().clone();
        tokio::spawn(async move {
            loop {
                let mut entries = tokio::fs::read_dir(&paths.requests_dir).await.unwrap();
                if let Some(entry) = entries.next_entry().await.unwrap() {
                    let request_name = entry.file_name().to_string_lossy().to_string();
                    let response_name = request_name.replace(".json", ".result.json");
                    let response_path = paths.responses_dir.join(response_name);
                    tokio::fs::write(response_path, b"not valid json {{{")
                        .await
                        .unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let err = client
            .send_request(&Request {
                kind: RequestType::Exec,
                cmd: Some(vec!["echo".into(), "hello".into()]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("Failed to parse Test VM response"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn send_request_returns_agent_error_when_error_field_set() {
        let temp = tempfile::tempdir().unwrap();
        let client = test_client(
            temp.path(),
            Duration::from_secs(30),
            Duration::from_millis(25),
        );
        client.ensure_layout().await.unwrap();

        let paths = client.paths().clone();
        tokio::spawn(async move {
            loop {
                let mut entries = tokio::fs::read_dir(&paths.requests_dir).await.unwrap();
                if let Some(entry) = entries.next_entry().await.unwrap() {
                    let request_name = entry.file_name().to_string_lossy().to_string();
                    let response_name = request_name.replace(".json", ".result.json");
                    let response_path = paths.responses_dir.join(response_name);
                    let response = Response {
                        stdout: String::new(),
                        exit_code: 1,
                        error: Some("command not found: bogus".into()),
                        duration_ms: 2,
                    };
                    tokio::fs::write(response_path, serde_json::to_vec(&response).unwrap())
                        .await
                        .unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let err = client
            .send_request(&Request {
                kind: RequestType::Exec,
                cmd: Some(vec!["bogus".into()]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("Test VM agent error: command not found: bogus"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn send_request_times_out_when_no_response() {
        let temp = tempfile::tempdir().unwrap();
        let client = test_client(
            temp.path(),
            Duration::from_millis(100),
            Duration::from_millis(20),
        );
        client.ensure_layout().await.unwrap();

        let err = client
            .send_request(&Request {
                kind: RequestType::Exec,
                cmd: Some(vec!["echo".into(), "hello".into()]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("Timed out waiting for Test VM response"),
            "unexpected error: {msg}"
        );

        // Request file should be cleaned up on timeout
        let mut entries = tokio::fs::read_dir(temp.path().join("requests"))
            .await
            .unwrap();
        assert!(
            entries.next_entry().await.unwrap().is_none(),
            "request file should be cleaned up after timeout"
        );
    }

    #[tokio::test]
    async fn wait_for_agent_ready_times_out() {
        let temp = tempfile::tempdir().unwrap();
        let client = test_client(
            temp.path(),
            Duration::from_secs(1),
            Duration::from_millis(20),
        );
        client.ensure_layout().await.unwrap();

        let err = client
            .wait_for_agent_ready(Duration::from_millis(80))
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("Timed out waiting for Test VM agent sentinel"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn wait_for_agent_ready_succeeds_when_sentinel_exists() {
        let temp = tempfile::tempdir().unwrap();
        let client = test_client(
            temp.path(),
            Duration::from_secs(1),
            Duration::from_millis(20),
        );
        client.ensure_layout().await.unwrap();

        // Write sentinel before waiting
        tokio::fs::write(temp.path().join("agent_ready"), "ready\n")
            .await
            .unwrap();

        client
            .wait_for_agent_ready(Duration::from_millis(100))
            .await
            .unwrap();
    }

    #[test]
    fn relative_transfer_path_errors_when_outside_base() {
        let base = Path::new("/tmp/shared");
        let outside = Path::new("/home/user/file.txt");
        let err = relative_transfer_path(base, outside).unwrap_err();
        assert!(
            err.to_string().contains("not inside shared dir"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn send_request_cleans_up_request_and_response_files() {
        let temp = tempfile::tempdir().unwrap();
        let client = test_client(
            temp.path(),
            Duration::from_secs(30),
            Duration::from_millis(25),
        );
        client.ensure_layout().await.unwrap();

        let paths = client.paths().clone();
        tokio::spawn(async move {
            loop {
                let mut entries = tokio::fs::read_dir(&paths.requests_dir).await.unwrap();
                if let Some(entry) = entries.next_entry().await.unwrap() {
                    let request_name = entry.file_name().to_string_lossy().to_string();
                    let response_name = request_name.replace(".json", ".result.json");
                    let response_path = paths.responses_dir.join(response_name);
                    let response = Response {
                        stdout: "ok".into(),
                        exit_code: 0,
                        error: None,
                        duration_ms: 1,
                    };
                    tokio::fs::write(response_path, serde_json::to_vec(&response).unwrap())
                        .await
                        .unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        client
            .send_request(&Request {
                kind: RequestType::Exec,
                cmd: Some(vec!["true".into()]),
                stdin_b64: None,
                src_path: None,
                dest_path: None,
                transfer_path: None,
            })
            .await
            .unwrap();

        // Both request and response files should be cleaned up
        let mut req_entries = tokio::fs::read_dir(temp.path().join("requests"))
            .await
            .unwrap();
        assert!(
            req_entries.next_entry().await.unwrap().is_none(),
            "request file should be cleaned up"
        );

        let mut resp_entries = tokio::fs::read_dir(temp.path().join("responses"))
            .await
            .unwrap();
        assert!(
            resp_entries.next_entry().await.unwrap().is_none(),
            "response file should be cleaned up"
        );
    }
}
