//! Monitor event types and broadcast channel for live dashboard streaming.

use std::sync::Arc;

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
}

/// A cheaply cloneable handle wrapping a broadcast channel for monitor events.
#[derive(Clone)]
pub struct MonitorHandle {
    sender: Arc<broadcast::Sender<MonitorEvent>>,
}

impl MonitorHandle {
    /// Create a new monitor handle with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
        }
    }

    /// Publish an event. Silently ignores errors when there are no receivers.
    pub fn send(&self, event: MonitorEvent) {
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<MonitorEvent> {
        self.sender.subscribe()
    }
}
