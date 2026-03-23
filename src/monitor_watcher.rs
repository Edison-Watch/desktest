//! Persistent monitor that watches an artifacts directory tree for multi-phase runs.
//!
//! Each subdirectory containing a `trajectory.jsonl` is treated as a phase.
//! The watcher tails each trajectory file and emits monitor events (PhaseStart,
//! StepComplete, TestComplete) to connected browser clients via SSE.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::monitor::{MonitorEvent, MonitorHandle};
use crate::trajectory::chrono_iso8601_now;

/// State for a single phase being watched.
struct PhaseState {
    /// Number of lines already read from this phase's trajectory.jsonl.
    lines_read: usize,
}

/// Watches `watch_dir` for subdirectories containing `trajectory.jsonl` files.
/// Emits monitor events as new trajectory entries appear.
pub async fn run_watcher(watch_dir: PathBuf, handle: MonitorHandle) {
    let phases: Arc<Mutex<HashMap<String, PhaseState>>> =
        Arc::new(Mutex::new(HashMap::new()));

    info!("Watching {} for phase directories...", watch_dir.display());

    // Poll loop: check for new/updated trajectory files every 500ms.
    // Using polling instead of inotify/FSEvents for simplicity and cross-platform compat.
    loop {
        if let Err(e) = scan_phases(&watch_dir, &phases, &handle).await {
            debug!("Scan error: {e}");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Scan the watch directory for phase subdirectories and tail their trajectory files.
async fn scan_phases(
    watch_dir: &Path,
    phases: &Arc<Mutex<HashMap<String, PhaseState>>>,
    handle: &MonitorHandle,
) -> Result<(), std::io::Error> {
    // Also check if watch_dir itself contains a trajectory.jsonl (single-phase mode)
    let trajectory_in_root = watch_dir.join("trajectory.jsonl");
    if trajectory_in_root.is_file() {
        let phase_id = watch_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string());
        process_phase(&phase_id, watch_dir, phases, handle).await;
    }

    let entries = match std::fs::read_dir(watch_dir) {
        Ok(e) => e,
        Err(e) => {
            debug!("Cannot read watch dir: {e}");
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let trajectory_path = path.join("trajectory.jsonl");
        if !trajectory_path.is_file() {
            continue;
        }

        let phase_id = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        process_phase(&phase_id, &path, phases, handle).await;
    }

    Ok(())
}

/// Process a single phase directory: emit PhaseStart if new, then tail new trajectory lines.
async fn process_phase(
    phase_id: &str,
    phase_dir: &Path,
    phases: &Arc<Mutex<HashMap<String, PhaseState>>>,
    handle: &MonitorHandle,
) {
    let trajectory_path = phase_dir.join("trajectory.jsonl");

    let mut phases_guard = phases.lock().await;
    let is_new = !phases_guard.contains_key(phase_id);

    if is_new {
        info!("Discovered new phase: {phase_id}");
        handle.send(MonitorEvent::PhaseStart {
            phase_id: phase_id.to_string(),
            phase_name: phase_id.to_string(),
            timestamp: chrono_iso8601_now(),
        });
        phases_guard.insert(
            phase_id.to_string(),
            PhaseState { lines_read: 0 },
        );
    }

    let state = phases_guard.get_mut(phase_id).unwrap();
    let lines_read = state.lines_read;

    // Read new lines from trajectory.jsonl
    let new_entries = match read_new_lines(&trajectory_path, lines_read) {
        Ok(entries) => entries,
        Err(e) => {
            debug!("Error reading trajectory for phase {phase_id}: {e}");
            return;
        }
    };

    for (line_json, _line_num) in &new_entries {
        emit_trajectory_event(line_json, phase_id, handle, &trajectory_path);
    }

    state.lines_read += new_entries.len();
}

/// Read lines from a JSONL file starting after `skip` lines.
fn read_new_lines(
    path: &Path,
    skip: usize,
) -> Result<Vec<(serde_json::Value, usize)>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut results = Vec::new();

    for (i, line) in reader.lines().enumerate() {
        if i < skip {
            continue;
        }
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(value) => results.push((value, i)),
            Err(e) => {
                debug!("Skipping malformed JSON line {i} in {}: {e}", path.display());
            }
        }
    }

    Ok(results)
}

/// Convert a trajectory JSONL entry into a MonitorEvent and send it.
fn emit_trajectory_event(
    entry: &serde_json::Value,
    phase_id: &str,
    handle: &MonitorHandle,
    trajectory_path: &Path,
) {
    let step = entry.get("step").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let thought = entry.get("thought").and_then(|v| v.as_str()).map(String::from);
    let action_code = entry
        .get("action_code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let result = entry
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let timestamp = entry
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let bash_output = entry.get("bash_output").and_then(|v| v.as_str()).map(String::from);
    let error_feedback = entry.get("error_feedback").and_then(|v| v.as_str()).map(String::from);

    // Load screenshot from the phase's artifact directory
    let screenshot_base64 = entry
        .get("screenshot_path")
        .and_then(|v| v.as_str())
        .and_then(|rel_path| {
            let screenshot_file = trajectory_path.parent()?.join(rel_path);
            match std::fs::read(&screenshot_file) {
                Ok(bytes) => {
                    use base64::Engine;
                    Some(base64::engine::general_purpose::STANDARD.encode(&bytes))
                }
                Err(_) => None,
            }
        });

    handle.send(MonitorEvent::StepComplete {
        step,
        thought,
        action_code,
        result: result.clone(),
        screenshot_base64,
        timestamp,
        bash_output,
        error_feedback,
    });

    // If this is a terminal result, emit TestComplete
    if result == "done" || result == "fail" || result == "timeout" || result == "max_steps" {
        let passed = result == "done";
        handle.send(MonitorEvent::TestComplete {
            test_id: phase_id.to_string(),
            passed,
            reasoning: format!("Phase '{phase_id}' ended with result: {result}"),
            duration_ms: 0, // Duration not available from trajectory alone
        });
    }
}
