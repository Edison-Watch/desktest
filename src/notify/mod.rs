//! Modular notification system for bug report integrations.
//!
//! When a bug is discovered in QA mode, the [`NotifierPipeline`] fans out
//! the event to all configured [`Notifier`] implementations (Slack, etc.).
//! Notifications are fire-and-forget — failures are logged but never block
//! the agent loop or fail the test.

pub mod slack;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use tracing::{info, warn};

use crate::config::Config;

/// Data about a bug that was reported.
#[derive(Debug, Clone)]
pub struct BugEvent {
    /// Unique bug identifier (e.g. "BUG-001").
    pub bug_id: String,
    /// Agent step number when the bug was discovered.
    pub step: usize,
    /// One-line summary (first line of description).
    pub summary: String,
    /// Full bug description from the agent.
    pub description: String,
    /// Path to the screenshot at time of bug, if available.
    /// Not yet used by any notifier but reserved for future screenshot uploads.
    #[expect(dead_code)]
    pub screenshot_path: Option<PathBuf>,
    /// Test identifier (e.g. "gedit-save-file").
    pub test_id: String,
}

/// Trait for notification integrations.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// async tasks spawned by [`NotifierPipeline`].
pub trait Notifier: Send + Sync {
    /// Human-readable name for logging (e.g. "Slack").
    fn name(&self) -> &str;

    /// Send a bug notification. Implementations should not panic.
    fn notify<'a>(
        &'a self,
        event: &'a BugEvent,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

/// Fans out bug notifications to all configured integrations.
pub struct NotifierPipeline {
    notifiers: Vec<Arc<dyn Notifier>>,
}

impl NotifierPipeline {
    pub fn new(notifiers: Vec<Arc<dyn Notifier>>) -> Self {
        Self { notifiers }
    }

    /// Returns true if no notifiers are configured.
    pub fn is_empty(&self) -> bool {
        self.notifiers.is_empty()
    }

    /// Spawn notification tasks for all notifiers. Non-blocking, non-failing.
    pub fn notify_all(&self, event: BugEvent) {
        let event = Arc::new(event);
        for notifier in &self.notifiers {
            let name = notifier.name().to_string();
            let notifier = Arc::clone(notifier);
            let event = Arc::clone(&event);
            tokio::spawn(async move {
                if let Err(e) = notifier.notify(&event).await {
                    warn!("[{name}] notification failed: {e}");
                }
            });
        }
    }
}

/// Build a [`NotifierPipeline`] from the application config.
///
/// Checks config and environment variables for each integration.
/// The `DESKTEST_SLACK_WEBHOOK_URL` env var takes precedence over the
/// config file value.
pub fn build_pipeline(config: &Config) -> NotifierPipeline {
    let mut notifiers: Vec<Arc<dyn Notifier>> = Vec::new();

    // Slack: env var takes precedence over config
    let slack_url = std::env::var("DESKTEST_SLACK_WEBHOOK_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            config
                .integrations
                .slack
                .as_ref()
                .and_then(|s| s.webhook_url.clone())
                .filter(|s| !s.is_empty())
        });

    if let Some(url) = slack_url {
        let channel = config
            .integrations
            .slack
            .as_ref()
            .and_then(|s| s.channel.clone());
        notifiers.push(Arc::new(slack::SlackNotifier::new(url, channel)));
        info!("Slack notifications enabled");
    }

    NotifierPipeline::new(notifiers)
}
