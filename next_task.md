# Completed Tasks

## ~~Fix recording not stopped on agent loop error~~
Fixed — recording is now stopped unconditionally before propagating agent loop errors.

## ~~Add per-execution timeout to evaluator exec calls~~
Fixed — all `session.exec()` and `session.exec_with_exit_code()` calls in `src/evaluator.rs` are now wrapped with `tokio::time::timeout()`. Default timeout: 120s. Configurable per-task via `eval_timeout_secs` on `EvaluatorConfig`.
