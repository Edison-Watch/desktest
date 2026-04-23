# CLAUDE.md

**Before any other work in this repo, enable prek:** `brew install prek && prek install`. Hooks are defined in `prek.toml`.

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Desktest** is a CLI tool for automated end-to-end testing of desktop applications (Linux, macOS, and Windows) using LLM-powered agents. It spins up a Docker container (Linux), Tart VM (macOS), or QEMU/KVM VM (Windows) with a desktop environment, deploys an app, then runs an OSWorld-style agent loop where the LLM interacts with the app via PyAutoGUI code execution and observes via screenshots + accessibility trees.

**Tech stack:** Rust (edition 2024), Tokio async runtime, Docker (Bollard), Tart (Apple Virtualization.framework), QEMU/KVM (Windows VMs), multi-model LLM support (OpenAI, Anthropic, custom OpenAI-compatible endpoints).

## Build & Run Commands

```bash
cargo build                                    # Build
cargo run -- validate examples/gedit-save.json # Validate task file
cargo run -- run task.json --config config.json # Run single test
cargo run -- run task.json --replay            # Deterministic replay (no LLM)
cargo run -- run task.json --qa                # Run with QA bug reporting
cargo run -- suite examples/                   # Run test suite
cargo run -- interactive task.json             # Interactive debugging
cargo run -- attach task.json --container ID  # Attach to existing container
cargo run -- logs desktest_artifacts/          # View trajectory in terminal
cargo run -- logs desktest_artifacts/ --steps 3-7  # View specific step range
cargo test                                     # All non-ignored tests
cargo test -- --ignored --test-threads=1       # Integration tests (require Docker)
```

## Architecture

**Main flow** (`src/main.rs`): Parse CLI → load task JSON → create session (Docker container, Tart VM, QEMU/KVM VM, or native host) → wait for desktop → run setup steps → run agent loop (or skip for programmatic-only) → run evaluation → write results → collect artifacts → cleanup.

**Attach flow** (`desktest attach`): Parse CLI → load task JSON → attach to existing container (no create/cleanup) → run setup steps → run agent loop → run evaluation → write results → collect artifacts. Uses `DockerSession::attach()` instead of `DockerSession::create()`.

**Exit codes:** 0=pass, 1=fail, 2=config error, 3=infra error, 4=agent error.

### Session abstraction

- `src/session/mod.rs` defines the `Session` trait (8 async methods) and `SessionKind` enum with five variants: `Docker(DockerSession)`, `Tart(TartSession)`, `Native(NativeSession)`, `WindowsVm(WindowsVmSession)`, `WindowsNative(WindowsNativeSession)`
- `forward_session!` macro generates `impl Session for SessionKind` by matching on variants — enum dispatch, not dynamic dispatch
- Platform-specific operations accessed via `session.as_docker()`, `session.as_tart()`, `session.as_native()`, `session.as_windows_vm()`, `session.as_windows_native()`
- `src/session/native.rs` — `NativeSession` runs commands directly on the host macOS desktop (no VM, no isolation)
- `src/session/windows_native.rs` — `WindowsNativeSession` scaffolding (stub impl, full implementation deferred to when a Windows host is available for testing)

### Non-obvious details

- The agent loop lives in `agent/loop_v2.rs` (`AgentLoopV2`) — the OSWorld-style PyAutoGUI code execution loop
- `src/task.rs` uses serde tagged enums (`#[serde(tag = "type")]`) for `AppConfig` (including `VncAttach` for attach mode, `MacosTart` for Tart VMs, `MacosNative` for host testing, `WindowsVm` for QEMU/KVM VMs, `WindowsNative` for host testing), `MetricConfig`, and `SetupStep`
- `AppError` variants in `src/error.rs` map to specific exit codes (0–4) — don't change the mapping without updating docs
- `pub(crate) use orchestration::run_task` in `main.rs` re-exports this for `suite.rs` to use as `crate::run_task`
- `src/observation.rs` uses `ObservationConfig::for_session()` to select platform-specific screenshot and a11y commands (Linux: scrot + pyatspi, macOS: screencapture + AXUIElement, Windows: PIL ImageGrab + uiautomation)

### Docker container (`docker/`)

Built from debian:bookworm-slim with Xvfb, XFCE4, x11vnc, xdotool, scrot, ffmpeg, Python3, PyAutoGUI, pyatspi, AT-SPI2, FUSE, GTK3 libs. Runs as non-root user "tester". Entrypoint starts display server, dbus, AT-SPI registry, desktop, VNC, then writes sentinel file.

**IMPORTANT:** `~/.Xauthority` must exist for the tester user. PyAutoGUI (via python-xlib) crashes with `Xlib.error.XauthError` without it. The base Dockerfile creates it, but custom images or images that switch users must ensure it exists. Custom images are validated at startup; built-in images have a fallback in `execute-action.py`.

Docker images:
- `desktest-desktop:latest` — Base image (Dockerfile)
- `desktest-desktop:electron` — Extends base with Node.js 20 + Electron deps (Dockerfile.electron)

Helper scripts:
- `docker/get-a11y-tree.py` — Extracts linearized accessibility tree via pyatspi (TSV format)
- `docker/execute-action.py` — Executes PyAutoGUI code from stdin, returns JSON result
- `docker/screenshot_compare.py` — PIL-based screenshot comparison for visual assertions

Default display resolution: 1920x1080.

### Windows VM (`src/windows/`, `windows/`)

Uses QEMU/KVM with a user-provided Windows 11 QCOW2 golden image. Copy-on-write overlays give each test a clean environment. VirtIO-FS shared directory (via WinFsp) provides host↔VM communication using the same file-based IPC protocol as Tart (`src/vm_protocol.rs`). Requires swtpm (software TPM 2.0) and OVMF (UEFI firmware).

Golden image preparation: `desktest init-windows` — two-stage process: Stage 1 installs Windows from ISO via Autounattend.xml, Stage 2 provisions via SSH (Python, PyAutoGUI, uiautomation, WinFsp, agent scripts).

Guest-side scripts:
- `windows/vm-agent.py` — Guest agent polling shared directory for requests (runs via Task Scheduler at logon)
- `windows/execute-action.py` — PyAutoGUI executor for Windows (Win32 SendInput backend)
- `windows/get-a11y-tree.py` — UIA accessibility tree extraction via `uiautomation` package
- `windows/win-screenshot.py` — Screenshot capture via PIL `ImageGrab.grab()`

Host-side modules:
- `src/windows/mod.rs` — `WindowsVmSession`: QEMU lifecycle, Session trait impl via `ProtocolClient`
- `src/windows/deploy.rs` — App deployment into Windows VM
- `src/windows/readiness.rs` — Desktop/app readiness detection for Windows
- `src/init_windows.rs` — `desktest init-windows` golden image provisioning

## Subagents

- Folder-size CI failure → spawn subagent `.claude/agents/folder-refactor-advisor.md`.
