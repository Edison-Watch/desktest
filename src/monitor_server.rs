//! HTTP server for the live monitoring dashboard.
//!
//! Serves the shared dashboard HTML in live mode and streams monitor events via SSE.

use std::convert::Infallible;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::warn;

use crate::monitor::MonitorHandle;

/// Start the monitor HTTP server on the given port.
///
/// Returns a `JoinHandle` that resolves when the server shuts down (i.e. when the
/// process exits). The server is automatically dropped on process exit.
pub fn start_monitor_server(handle: MonitorHandle, port: u16, vnc_url: &str) -> JoinHandle<()> {
    let dashboard_html = build_live_dashboard(vnc_url);

    let app = Router::new()
        .route("/", get(move || async move { Html(dashboard_html) }))
        .route(
            "/events",
            get(move || async move { sse_handler(handle) }),
        );

    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
            Ok(l) => l,
            Err(e) => {
                warn!("Failed to bind monitor server on port {port}: {e}");
                return;
            }
        };
        if let Err(e) = axum::serve(listener, app).await {
            warn!("Monitor server error: {e}");
        }
    })
}

/// Build the dashboard HTML configured for live mode.
fn build_live_dashboard(vnc_url: &str) -> String {
    let template = include_str!("dashboard.html");
    template
        .replace("/*__STEPS__*/[]", "/*__STEPS__*/[]")
        .replace("/*__MODE__*/\"static\"", &format!("/*__MODE__*/\"live\""))
        .replace("/*__VNC_URL__*/\"\"", &format!("/*__VNC_URL__*/\"{}\"", vnc_url.replace('"', "\\\"")))
        .replace("/*__RECORDING_URI__*/\"\"", "/*__RECORDING_URI__*/\"\"")
        .replace("/*__TRAJECTORY_PATH__*/\"\"", "/*__TRAJECTORY_PATH__*/\"\"")
}

/// SSE handler that streams monitor events to the browser.
fn sse_handler(
    handle: MonitorHandle,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = handle.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            let event_type = match &event {
                crate::monitor::MonitorEvent::TestStart { .. } => "test_start",
                crate::monitor::MonitorEvent::StepComplete { .. } => "step_complete",
                crate::monitor::MonitorEvent::TestComplete { .. } => "test_complete",
                crate::monitor::MonitorEvent::SuiteProgress { .. } => "suite_progress",
            };
            let json = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().event(event_type).data(json)))
        }
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
