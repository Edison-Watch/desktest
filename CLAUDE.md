# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Tent** is a CLI tool for automated end-to-end testing of Linux desktop applications using an LLM-powered agent. It spins up a Docker container with an XFCE desktop (Xvfb + x11vnc), deploys an AppImage or folder-based app, then runs an agent loop where the LLM interacts with the app via mouse/keyboard (xdotool) and visual feedback (screenshots via scrot).

**Tech stack:** Rust (edition 2024), Tokio async runtime, Docker (Bollard), OpenAI-compatible API (supports Gemini via base URL override).

## Build & Run Commands

```bash
cargo build                                    # Build
cargo run -- config.json instructions.md       # Run (add --debug or --interactive)
cargo test --lib                               # Unit tests only
cargo test -- --ignored --test-threads=1       # Integration tests (require Docker daemon)
cargo test                                     # All non-ignored tests
```

## Architecture

**Main flow** (`src/main.rs`): Load config → create Docker container → wait for desktop → deploy & launch app → wait for app window → run agent loop → collect artifacts → cleanup.

**Exit codes:** 0=pass, 1=fail, 2=config error, 3=infra error, 4=agent error.

### Key modules

- `src/config.rs` — JSON config loading with cross-field validation (app_type determines required fields)
- `src/docker.rs` — `DockerSession`: container lifecycle, file transfer (tar-based), app deployment, command execution
- `src/readiness.rs` — Desktop/app readiness polling via sentinel file, xdotool window detection, baseline diffing
- `src/agent/mod.rs` — Agent loop: sends system prompt + instructions to LLM, dispatches tool calls, loops until `done()` tool is called
- `src/agent/openai.rs` — OpenAI-compatible HTTP client with custom base URL support
- `src/agent/tools.rs` — 13 tool definitions (mouse, keyboard, screenshot, think, done) + dispatch via xdotool in container
- `src/input.rs` — Pure functions building xdotool command strings
- `src/screenshot.rs` — Captures via `scrot`, copies from container, base64 encodes
- `src/artifacts.rs` — Collects logs, screenshots, home dir, process list, conversation JSON
- `src/error.rs` — Error enum with typed variants mapping to exit codes

### Docker container (`docker/`)

Built from debian:bookworm-slim with Xvfb, XFCE4, x11vnc, xdotool, scrot, FUSE, GTK3 libs. Runs as non-root user "tester". Entrypoint starts display server, dbus, desktop, VNC, then writes sentinel file.

Apps are launched with `--appimage-extract-and-run` and `--no-sandbox` flags for container compatibility.
