//! HTTP server for the live monitoring dashboard.
//!
//! Serves the shared dashboard HTML in live mode and streams monitor events via SSE.

use std::convert::Infallible;

use axum::Router;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, Json};
use axum::routing::get;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tracing::warn;

use crate::monitor::MonitorHandle;

/// Start the monitor HTTP server on the given port.
///
/// Binds the port synchronously (before spawning) so the caller knows immediately
/// whether the server started successfully. Returns `None` if the port is unavailable.
pub async fn start_monitor_server(handle: MonitorHandle, port: u16) -> Option<JoinHandle<()>> {
    let dashboard_html = build_live_dashboard();

    let state_handle = handle.clone();
    let app = Router::new()
        .route("/", get(move || async move { Html(dashboard_html) }))
        .route("/events", get(move || async move { sse_handler(handle) }))
        .route(
            "/state",
            get(move || async move { state_handler(state_handle).await }),
        );

    // Localhost only — no auth on this endpoint. Unlike VNC (which has vnc_bind_addr),
    // there is no config override for the monitor bind address yet.
    let listener = match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            warn!("Failed to bind monitor server on port {port}: {e}");
            return None;
        }
    };

    Some(tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            warn!("Monitor server error: {e}");
        }
    }))
}

/// Build the dashboard HTML configured for live mode.
///
/// VNC URL is not baked in — it arrives via SSE `test_start` event and
/// the `/state` endpoint for late-connecting clients.
fn build_live_dashboard() -> String {
    let template = include_str!("dashboard.html");
    template.replace("/*__MODE__*/\"static\"", "/*__MODE__*/\"live\"")
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
                crate::monitor::MonitorEvent::PhaseStart { .. } => "phase_start",
            };
            let json = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().event(event_type).data(json)))
        }
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
            warn!("SSE client lagged; dropped {n} monitor events");
            None
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// State endpoint returning the last TestStart event as JSON.
/// Late-connecting browsers fetch this to get current test context.
async fn state_handler(handle: MonitorHandle) -> Json<serde_json::Value> {
    match handle.last_test_start() {
        Some(event) => Json(serde_json::to_value(&event).unwrap_or_default()),
        None => Json(serde_json::json!(null)),
    }
}
