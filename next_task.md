# Next Task: Fix recording not stopped on agent loop error

## Problem

In `run_task_inner` and `run_interactive_step_inner`, if `run_agent_loop().await?` returns an `Err`, the `?` operator propagates the error immediately, skipping the recording stop/collect block. This means:

- ffmpeg is never sent SIGINT
- The MP4 file is never finalized (missing moov atom = unplayable)
- The recording of the failed session — the most diagnostically valuable — is lost

### Affected code paths

- `src/main.rs` `run_task_inner`: `EvaluatorMode::Llm` (line ~681) and `EvaluatorMode::Hybrid` (line ~694) both use `?` which bypasses the recording stop at lines ~720-723
- `src/main.rs` `run_interactive_step_inner`: agent loop error at line ~1073 skips recording stop at lines ~1099-1102

## Fix

Capture the agent loop result without `?`, stop/collect the recording unconditionally, then propagate the error. For example:

```rust
// Instead of:
let agent_outcome = run_agent_loop(...).await?;

// Do:
let agent_loop_result = run_agent_loop(...).await;

// Stop recording unconditionally (before propagating any error)
if let Some(rec) = &recording {
    rec.stop(session).await;
    rec.collect(session, artifacts_dir).await;
}

let agent_outcome = agent_loop_result?;
```

Apply the same pattern in both `run_task_inner` and `run_interactive_step_inner`.

## Context

Identified during PR #3 review (devin-ai-integration). This is a pre-existing issue but was made more impactful by moving recording start to after app launch.

---

# Next Task: Add per-execution timeout to `evaluate_script_replay`

## Problem

**File:** `src/evaluator.rs` (line ~408-414)

The `evaluate_script_replay` function executes user scripts via `exec_with_exit_code` without a timeout. If the script hangs (infinite loop, waiting for an unmet condition), the process blocks indefinitely. This applies to all evaluator metrics (`command_output`, `exit_code`, etc.) — none have per-exec timeouts.

The task-level `timeout` field provides a coarse safety net by killing the container, but individual evaluation steps should also be time-bounded.

## Fix

Wrap `exec_with_exit_code` calls with `tokio::time::timeout`, or add a timeout parameter to the exec methods in `DockerSession`.

## Context

Identified during PR #5 review (sentry). Pre-existing pattern across all evaluator metrics.
