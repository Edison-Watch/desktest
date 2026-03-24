---
description: "Rust coding conventions for desktest — error handling, async patterns, logging, and visibility. Apply when writing or modifying any Rust source file."
globs:
  - 'src/**/*.rs'
alwaysApply: false
---

# Rust Conventions

- Use Rust edition 2024 features
- Use `tokio` for all async — this is a Tokio-based project
- Handle errors via `AppError` variants — NEVER use `.unwrap()` in non-test code
- Use `tracing` (`info!`, `warn!`, `debug!`) for logging — NEVER use `println!` for diagnostic output (only for user-facing CLI output)
- Prefer `pub(crate)` over `pub` for internal APIs
- NEVER add dependencies without justification — keep the binary lean
- NEVER put business logic in `main.rs` — it should only dispatch to other modules
- Evaluator metrics go in `src/evaluator/` as separate modules
- Agent logic goes in `src/agent/`
- Docker interaction goes in `src/docker/`
