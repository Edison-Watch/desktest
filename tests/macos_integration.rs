//! Integration tests for macOS desktop testing support.
//!
//! All tests marked `#[ignore]` require Apple Silicon (M1+) running macOS 13+
//! with Tart installed and a golden image prepared via `desktest init-macos`.
//! Run with: `cargo test -- --ignored --test-threads=1`
//!
//! These tests are expensive (VM clone + boot) and cannot run in parallel due
//! to Tart's 2-VM concurrency limit.

use std::process::Command;

/// Helper: run `desktest validate` on a task file and assert it succeeds.
fn validate_task(task_path: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_desktest"))
        .args(["validate", task_path])
        .output()
        .expect("failed to run desktest validate");

    assert!(
        output.status.success(),
        "desktest validate failed for {task_path}:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Helper: check that Tart is available. Returns false if not installed.
fn tart_available() -> bool {
    Command::new("tart")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Helper: check that a Tart image exists.
fn tart_image_exists(image: &str) -> bool {
    Command::new("tart")
        .args(["get", image])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── Validation tests (no Tart/VM required) ──────────────────────────

#[test]
fn validate_macos_textedit_example() {
    validate_task("examples/macos-textedit.json");
}

#[test]
fn validate_macos_electron_example() {
    validate_task("examples/macos-electron.json");
}

#[test]
fn validate_macos_native_textedit_example() {
    validate_task("examples/macos-native-textedit.json");
}

// ── CLI preflight tests ─────────────────────────────────────────────

#[test]
#[ignore] // Requires macOS with Tart installed
fn doctor_shows_tart_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_desktest"))
        .args(["doctor"])
        .output()
        .expect("failed to run desktest doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // On macOS Apple Silicon, doctor should mention Tart
    if cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64" {
        assert!(
            stdout.contains("Tart"),
            "doctor output should mention Tart on Apple Silicon:\n{stdout}"
        );
    }
}

// ── End-to-end tests ────────────────────────────────────────────────

#[test]
#[ignore] // Requires Apple Silicon + Tart + desktest-macos:latest + LLM API key
fn e2e_macos_textedit() {
    if !tart_available() {
        eprintln!("Skipping: Tart not installed");
        return;
    }
    if !tart_image_exists("desktest-macos:latest") {
        eprintln!(
            "Skipping: desktest-macos:latest image not found \
             (run `desktest init-macos` first)"
        );
        return;
    }

    let output = Command::new(env!("CARGO_BIN_EXE_desktest"))
        .args(["run", "examples/macos-textedit.json"])
        .output()
        .expect("failed to run desktest");

    // Exit code 0 = pass, 1 = fail (test ran but task failed)
    // Exit codes 2-4 = config/infra/agent error
    assert!(
        output.status.code().unwrap_or(99) <= 1,
        "desktest run failed with infrastructure error:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
#[ignore] // Requires Apple Silicon + Tart + desktest-macos-electron:latest + LLM API key
fn e2e_macos_electron() {
    if !tart_available() {
        eprintln!("Skipping: Tart not installed");
        return;
    }
    if !tart_image_exists("desktest-macos-electron:latest") {
        eprintln!("Skipping: desktest-macos-electron:latest image not available");
        return;
    }

    let output = Command::new(env!("CARGO_BIN_EXE_desktest"))
        .args(["run", "examples/macos-electron.json"])
        .output()
        .expect("failed to run desktest");

    assert!(
        output.status.code().unwrap_or(99) <= 1,
        "desktest run failed with infrastructure error:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
