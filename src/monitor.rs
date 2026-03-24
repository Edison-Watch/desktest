//! Monitor event types and broadcast channel for live dashboard streaming.

use std::sync::{Arc, RwLock};

use serde::Serialize;
use tokio::sync::broadcast;

/// Events published by the agent loop / suite runner for the live dashboard.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MonitorEvent {
    /// Emitted when a test begins.
    TestStart {
        test_id: String,
        instruction: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        completion_condition: Option<String>,
        vnc_url: String,
        max_steps: usize,
    },
    /// Emitted after each agent step completes.
    StepComplete {
        step: usize,
        thought: Option<String>,
        action_code: String,
        result: String,
        screenshot_base64: Option<String>,
        timestamp: String,
        /// Captured bash command stdout/stderr (only present when --with-bash is enabled).
        #[serde(skip_serializing_if = "Option::is_none")]
        bash_output: Option<String>,
        /// Error feedback from execution failures (bash or Python).
        #[serde(skip_serializing_if = "Option::is_none")]
        error_feedback: Option<String>,
        /// Action type: "python", "bash", "python+bash", or None.
        #[serde(skip_serializing_if = "Option::is_none")]
        action_type: Option<String>,
    },
    /// Emitted when a test finishes.
    TestComplete {
        test_id: String,
        passed: bool,
        reasoning: String,
        duration_ms: u64,
    },
    /// Emitted during suite runs to report progress.
    SuiteProgress {
        completed: usize,
        total: usize,
        current_test_id: String,
    },
    /// Emitted by the persistent monitor when a new phase directory is detected.
    PhaseStart {
        phase_id: String,
        phase_name: String,
        timestamp: String,
    },
}

/// A cheaply cloneable handle wrapping a broadcast channel for monitor events.
///
/// Also caches the last `TestStart` event synchronously so late-connecting
/// browsers can fetch current state via the `/state` endpoint.
#[derive(Clone)]
pub struct MonitorHandle {
    sender: Arc<broadcast::Sender<MonitorEvent>>,
    last_test_start: Arc<RwLock<Option<MonitorEvent>>>,
}

impl MonitorHandle {
    /// Create a new monitor handle with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
            last_test_start: Arc::new(RwLock::new(None)),
        }
    }

    /// Publish an event. Silently ignores errors when there are no receivers.
    /// Caches `TestStart` events synchronously for late-connecting clients.
    pub fn send(&self, event: MonitorEvent) {
        if matches!(event, MonitorEvent::TestStart { .. }) {
            if let Ok(mut guard) = self.last_test_start.write() {
                *guard = Some(event.clone());
            }
        }
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<MonitorEvent> {
        self.sender.subscribe()
    }

    /// Get the last `TestStart` event (for late-connecting clients).
    pub fn last_test_start(&self) -> Option<MonitorEvent> {
        self.last_test_start
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }
}
