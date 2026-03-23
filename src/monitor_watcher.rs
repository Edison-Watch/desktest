//! Persistent monitor that watches an artifacts directory tree for multi-phase runs.
//!
//! Each subdirectory containing a `trajectory.jsonl` is treated as a phase.
//! The watcher tails each trajectory file and emits monitor events (PhaseStart,
//! StepComplete, TestComplete) to connected browser clients via SSE.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing::{debug, info};

use crate::monitor::{MonitorEvent, MonitorHandle};
use crate::trajectory::chrono_iso8601_now;

/// State for a single phase being watched.
struct PhaseState {
    /// Byte offset into the trajectory file — seek here on each poll to avoid
    /// re-reading already-consumed lines.
    byte_offset: u64,
    /// Whether we've emitted a synthetic TestStart for late-connecting clients.
    test_start_emitted: bool,
}

/// Watches `watch_dir` for subdirectories containing `trajectory.jsonl` files.
/// Emits monitor events as new trajectory entries appear.
pub async fn run_watcher(watch_dir: PathBuf, handle: MonitorHandle) {
    let mut phases: HashMap<String, PhaseState> = HashMap::new();

    info!("Watching {} for phase directories...", watch_dir.display());

    // Poll loop: check for new/updated trajectory files every 500ms.
    // Using polling instead of inotify/FSEvents for simplicity and cross-platform compat.
    loop {
        scan_phases(&watch_dir, &mut phases, &handle);
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Scan the watch directory for phase subdirectories and tail their trajectory files.
/// If subdirectories with trajectory.jsonl exist, those are used (multi-phase mode).
/// Otherwise, if watch_dir itself contains trajectory.jsonl, it's used (single-phase mode).
/// This avoids phase ID collisions between the root and a same-named subdirectory.
fn scan_phases(
    watch_dir: &Path,
    phases: &mut HashMap<String, PhaseState>,
    handle: &MonitorHandle,
) {
    let entries = match std::fs::read_dir(watch_dir) {
        Ok(e) => e,
        Err(e) => {
            debug!("Cannot read watch dir: {e}");
            return;
        }
    };

    // Sort entries lexicographically for deterministic phase ordering
    let mut dir_entries: Vec<_> = entries.flatten().collect();
    dir_entries.sort_by_key(|e| e.file_name());

    // Collect qualifying subdirectories
    let mut found_subdirs = false;
    for entry in &dir_entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let trajectory_path = path.join("trajectory.jsonl");
        if !trajectory_path.is_file() {
            continue;
        }

        found_subdirs = true;
        let phase_id = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        process_phase(&phase_id, &path, phases, handle);
    }

    // Fallback: if no subdirectories have trajectories, check if watch_dir itself does
    // (single-phase mode, e.g. `desktest monitor --watch ./desktest_artifacts/`)
    if !found_subdirs {
        let trajectory_in_root = watch_dir.join("trajectory.jsonl");
        if trajectory_in_root.is_file() {
            let phase_id = watch_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "root".to_string());
            process_phase(&phase_id, watch_dir, phases, handle);
        }
    }
}

/// Process a single phase directory: emit PhaseStart if new, then tail new trajectory lines.
fn process_phase(
    phase_id: &str,
    phase_dir: &Path,
    phases: &mut HashMap<String, PhaseState>,
    handle: &MonitorHandle,
) {
    let trajectory_path = phase_dir.join("trajectory.jsonl");

    let is_new = !phases.contains_key(phase_id);

    if is_new {
        info!("Discovered new phase: {phase_id}");
        handle.send(MonitorEvent::PhaseStart {
            phase_id: phase_id.to_string(),
            phase_name: phase_id.to_string(),
            timestamp: chrono_iso8601_now(),
        });
        phases.insert(
            phase_id.to_string(),
            PhaseState {
                byte_offset: 0,
                test_start_emitted: false,
            },
        );
    }

    let state = phases.get_mut(phase_id).unwrap();
    let offset = state.byte_offset;

    // Read new lines from trajectory.jsonl, seeking to where we left off
    let ReadResult {
        entries: new_entries,
        new_byte_offset,
    } = match read_new_lines(&trajectory_path, offset) {
        Ok(result) => result,
        Err(e) => {
            debug!("Error reading trajectory for phase {phase_id}: {e}");
            return;
        }
    };

    // Emit a synthetic TestStart for the first valid entry so the dashboard header populates
    if !state.test_start_emitted && !new_entries.is_empty() {
        // Try to read task.json from the phase directory for metadata
        let (instruction, max_steps) = read_task_metadata(phase_dir);
        handle.send(MonitorEvent::TestStart {
            test_id: phase_id.to_string(),
            instruction,
            completion_condition: None,
            vnc_url: String::new(),
            max_steps,
        });
        state.test_start_emitted = true;
    }

    for entry in &new_entries {
        emit_trajectory_event(entry, phase_id, handle, &trajectory_path);
    }

    // Only advance past lines that were blank or successfully parsed;
    // malformed lines (possibly partial writes) will be retried next poll
    state.byte_offset = new_byte_offset;
}

/// Result from reading new lines from the trajectory file.
struct ReadResult {
    entries: Vec<serde_json::Value>,
    /// Byte offset to seek to on the next poll (avoids re-reading consumed lines).
    /// Only advances past blank or successfully parsed lines; malformed lines at EOF
    /// are retried on the next poll (they may be partial writes).
    new_byte_offset: u64,
}

/// Read new lines from a JSONL file, seeking to `byte_offset` to skip already-consumed data.
/// Returns valid parsed entries and updated cursors for the next poll.
fn read_new_lines(path: &Path, byte_offset: u64) -> Result<ReadResult, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    if byte_offset > 0 {
        file.seek(SeekFrom::Start(byte_offset))?;
    }
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut current_offset = byte_offset;
    let mut safe_offset = byte_offset;
    for line in reader.lines() {
        let line = line?;
        // +1 for the newline character
        current_offset += line.len() as u64 + 1;

        if line.trim().is_empty() {
            safe_offset = current_offset;
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(value) => {
                entries.push(value);
                safe_offset = current_offset;
            }
            Err(e) => {
                debug!("Possibly partial line in {}, will retry: {e}", path.display());
                // Do NOT advance safe_offset — retry on next poll
            }
        }
    }

    Ok(ReadResult {
        entries,
        new_byte_offset: safe_offset,
    })
}

/// Try to read task metadata from a task.json file in the phase directory.
/// Returns (instruction, max_steps) with defaults if not found.
fn read_task_metadata(phase_dir: &Path) -> (String, usize) {
    let task_path = phase_dir.join("task.json");
    if let Ok(content) = std::fs::read_to_string(&task_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            let instruction = value
                .get("instruction")
                .and_then(|v| v.as_str())
                .unwrap_or("(no instruction)")
                .to_string();
            let max_steps = value
                .get("max_steps")
                .and_then(|v| v.as_u64())
                .unwrap_or(15) as usize;
            return (instruction, max_steps);
        }
    }
    ("(no instruction)".to_string(), 15)
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
