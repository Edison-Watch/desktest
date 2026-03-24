---
description: "Desktest project architecture — core flow, module responsibilities, and key patterns. Apply when working on orchestration, CLI, or cross-module changes."
globs:
  - 'src/main.rs'
  - 'src/orchestration.rs'
  - 'src/cli.rs'
  - 'src/task.rs'
  - 'src/error.rs'
alwaysApply: false
---

# Desktest Architecture

Desktest is a CLI tool for automated E2E testing of Linux desktop applications using LLM-powered agents inside Docker containers with XFCE desktops.

## Core Flow

- `src/main.rs` → CLI parsing only, dispatches to orchestration. NEVER put business logic here.
- `src/orchestration.rs` → Main task runner: container setup → setup steps → agent loop → evaluation → artifacts → cleanup
- `src/agent/loop_v2.rs` → OSWorld-style agent loop (LLM ↔ PyAutoGUI code execution)
- `src/task.rs` → Task definition with serde tagged enums (`#[serde(tag = "type")]`)
- `src/evaluator/` → Programmatic evaluation metrics (file compare, command output, script replay)

## Key Patterns

- `AppError` variants in `src/error.rs` map to exit codes: 0=pass, 1=fail, 2=config, 3=infra, 4=agent. NEVER change the mapping without updating docs.
- `pub(crate) use orchestration::{parse_resolution, run_task}` in `main.rs` re-exports for `suite.rs` to use as `crate::run_task`
- Evaluator mode is determined by `EvaluatorMode` enum: `Llm`, `Programmatic`, or `Hybrid`
- `apply_replay_override()` in `task.rs` injects `ScriptReplay` metric and forces `Programmatic` mode
