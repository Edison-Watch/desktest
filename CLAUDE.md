# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Tent** is a CLI tool for automated end-to-end testing of Linux desktop applications using LLM-powered agents. It spins up a Docker container with an XFCE desktop (Xvfb + x11vnc), deploys an app (AppImage, folder, or custom Docker image), then runs an OSWorld-style agent loop where the LLM interacts with the app via PyAutoGUI code execution and observes via screenshots + accessibility trees.

**Tech stack:** Rust (edition 2024), Tokio async runtime, Docker (Bollard), multi-model LLM support (OpenAI, Anthropic, custom OpenAI-compatible endpoints).

## Build & Run Commands

```bash
cargo build                                    # Build
cargo run -- validate examples/gedit-save.json # Validate task file
cargo run -- run task.json --config config.json # Run single test
cargo run -- suite examples/                   # Run test suite
cargo run -- interactive task.json             # Interactive debugging
cargo test                                     # All non-ignored tests
cargo test -- --ignored --test-threads=1       # Integration tests (require Docker)
```

## Architecture

**Main flow** (`src/main.rs`): Parse CLI → load task JSON → create Docker container → wait for desktop → run setup steps → run agent loop (or skip for programmatic-only) → run evaluation → write results → collect artifacts → cleanup.

**Exit codes:** 0=pass, 1=fail, 2=config error, 3=infra error, 4=agent error.

### Key modules

- `src/main.rs` — CLI (clap subcommands: run, suite, interactive, validate), orchestration
- `src/task.rs` — Task JSON schema: `TaskDefinition`, `SetupStep`, `EvaluatorConfig`, `MetricConfig` with serde tagged enums
- `src/config.rs` — Runtime config loading with cross-field validation (app_type determines required fields)
- `src/docker.rs` — `DockerSession`: container lifecycle, file transfer (tar-based), app deployment, command execution, custom image validation
- `src/setup.rs` — Setup step execution: execute, copy, open, sleep
- `src/agent/loop_v2.rs` — OSWorld-style agent loop: observe → LLM → parse → execute → repeat, with timeouts and retry
- `src/agent/context.rs` — Sliding window context management, message construction, system prompt
- `src/agent/pyautogui.rs` — Parse LLM output for Python code blocks + special commands (DONE/FAIL/WAIT), execute via container
- `src/agent/mod.rs` — Legacy agent loop (tool-call based, kept for backward compat)
- `src/agent/tools.rs` — Legacy tool definitions (mouse, keyboard, screenshot, done) + xdotool dispatch
- `src/provider/mod.rs` — `LlmProvider` trait, provider factory, common message types
- `src/provider/openai.rs` — OpenAI implementation
- `src/provider/anthropic.rs` — Anthropic Claude implementation (Messages API with vision)
- `src/provider/custom.rs` — Custom OpenAI-compatible endpoint implementation
- `src/observation.rs` — Screenshot + accessibility tree capture with retry, trimming, configurable modes
- `src/evaluator.rs` — Programmatic evaluation: file_compare, file_compare_semantic, command_output, file_exists, exit_code
- `src/results.rs` — Structured `results.json` output with `ResultsWriter`
- `src/trajectory.rs` — Step-by-step `trajectory.jsonl` logging
- `src/recording.rs` — Video recording via ffmpeg x11grab inside container
- `src/suite.rs` — Test suite discovery, execution, aggregated `suite-results.json`
- `src/readiness.rs` — Desktop/app readiness polling via sentinel file, xdotool window detection
- `src/input.rs` — Pure functions building xdotool command strings (legacy)
- `src/screenshot.rs` — Screenshot capture via scrot, base64 encoding
- `src/artifacts.rs` — Collects logs, screenshots, home dir, conversation JSON
- `src/error.rs` — `AppError` enum with typed variants mapping to exit codes, `AgentOutcome`

### Docker container (`docker/`)

Built from debian:bookworm-slim with Xvfb, XFCE4, x11vnc, xdotool, scrot, ffmpeg, Python3, PyAutoGUI, pyatspi, AT-SPI2, FUSE, GTK3 libs. Runs as non-root user "tester". Entrypoint starts display server, dbus, AT-SPI registry, desktop, VNC, then writes sentinel file.

Helper scripts:
- `docker/get-a11y-tree.py` — Extracts linearized accessibility tree via pyatspi (TSV format)
- `docker/execute-action.py` — Executes PyAutoGUI code from stdin, returns JSON result

Default display resolution: 1920x1080.
