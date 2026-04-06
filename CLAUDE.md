# CLAUDE.md
<!-- This file MUST stay under 200 lines. CI enforces this. Use CLAUDE.local.md for personal notes. -->

## What is Desktest?

CLI tool for automated end-to-end testing of desktop apps. Spins up an isolated environment
(Docker container on Linux, Tart VM on macOS, QEMU/KVM VM on Windows), deploys an app, then
runs an LLM-powered agent that interacts via PyAutoGUI and observes via screenshots + a11y trees.

**Tech stack:** Rust 2024, Tokio, Docker (Bollard), Tart, QEMU/KVM, multi-LLM (Anthropic/OpenAI/custom).

## Build & Test

```bash
cargo build                                    # Build
cargo test                                     # Unit tests
cargo test -- --ignored --test-threads=1       # Integration tests (need Docker)
cargo run -- validate examples/gedit-save.json # Validate a task file
cargo run -- run task.json --config config.json
cargo run -- run task.json --replay            # Deterministic replay (no LLM)
cargo run -- suite examples/                   # Run test suite
cargo run -- interactive task.json             # Interactive debugging
cargo run -- logs desktest_artifacts/          # View trajectory in terminal
```

## Architecture

**Main flow:** CLI → load task JSON → create session → wait for desktop → setup steps → agent loop → evaluation → results → artifacts → cleanup.

**Exit codes:** 0=pass, 1=fail, 2=config error, 3=infra error, 4=agent error (`src/error.rs`).

### Key modules (`src/`)

| Module | Purpose |
|---|---|
| `main.rs` | CLI entry, command routing |
| `orchestration.rs` | Task orchestration engine (the big one) |
| `task.rs` | Task JSON schema — serde tagged enums for `AppConfig`, `MetricConfig`, `SetupStep` |
| `session/mod.rs` | `Session` trait + `SessionKind` enum (Docker/Tart/Native/WindowsVm/WindowsNative) |
| `agent/loop_v2.rs` | OSWorld-style agent loop — screenshot → LLM → PyAutoGUI → repeat |
| `agent/context.rs` | Agent context, platform detection, observation handling |
| `provider/` | Multi-provider LLM support (Anthropic, OpenAI, custom endpoints) |
| `observation.rs` | Platform-specific screenshot + a11y tree extraction |
| `evaluator/` | Test evaluation (command output, file compare, scripts) |
| `config.rs` | Config loading, LLM provider setup |
| `monitor_server.rs` | Live monitoring dashboard (Axum + SSE) |
| `codify.rs` | Convert agent trajectories → deterministic Python replay |
| `suite.rs` | Multi-test runner |

### Session abstraction

`forward_session!` macro generates `impl Session for SessionKind` — enum dispatch, not dynamic dispatch.
Platform-specific access: `session.as_docker()`, `session.as_tart()`, etc.

### Platform support

**Linux (Docker):** `docker/` — Debian bookworm-slim, Xvfb, XFCE4, VNC, PyAutoGUI, pyatspi.
Key scripts: `execute-action.py`, `get-a11y-tree.py`, `screenshot_compare.py`.
`~/.Xauthority` must exist for the tester user or PyAutoGUI crashes.

**macOS (Tart):** `src/tart/`, `macos/` — Tart VM lifecycle, Swift a11y helper, VirtIO-FS IPC (`src/vm_protocol.rs`).

**Windows (QEMU/KVM):** `src/windows/`, `windows/` — QCOW2 golden images, COW overlays, VirtIO-FS + WinFsp.
Guest scripts: `vm-agent.py`, `execute-action.py`, `get-a11y-tree.py`.
Init: `desktest init-windows` (two-stage: ISO install → SSH provisioning).

## Non-obvious things

- `pub(crate) use orchestration::run_task` in `main.rs` re-exports for `suite.rs`
- `ObservationConfig::for_session()` selects platform-specific commands
- `AppError` exit code mapping in `src/error.rs` — don't change without updating docs
- Docker images: `desktest-desktop:latest` (base), `desktest-desktop:electron` (+ Node.js 20)
- Default display resolution: 1920x1080
