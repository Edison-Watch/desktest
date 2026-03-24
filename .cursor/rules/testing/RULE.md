---
description: "Testing conventions — unit vs integration tests, when to use #[ignore]. Apply when writing or modifying tests."
globs:
  - 'src/**/*.rs'
alwaysApply: false
---

# Testing

- Unit tests: `cargo test` (no Docker required)
- Integration tests: `cargo test -- --ignored --test-threads=1` (require Docker)
- ALWAYS run `cargo test` before pushing
- NEVER use `#[ignore]` on unit tests — only on integration tests that need Docker
- Test modules live alongside their source in `#[cfg(test)] mod tests`
