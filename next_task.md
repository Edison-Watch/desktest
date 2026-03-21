# Completed Tasks

## ~~Fix recording not stopped on agent loop error~~
Fixed — recording is now stopped unconditionally before propagating agent loop errors.

## ~~Add per-execution timeout to evaluator exec calls~~
Fixed — all `session.exec()` and `session.exec_with_exit_code()` calls in evaluator are now wrapped with `tokio::time::timeout()`. Default timeout: 120s. Configurable per-task via `eval_timeout_secs` on `EvaluatorConfig`.

---

# Next Tasks — Medium-Impact Improvements

Items identified during the structural refactoring that are worth addressing but were out of scope for the split.

## ~~1. Provider HTTP dedup~~
Extracted shared HTTP logic into `src/provider/http_base.rs`. `OpenAiProvider` and `CustomProvider` are now thin constructor wrappers delegating to `HttpProvider`.

## 2. Legacy agent removal

`src/agent/mod.rs`, `src/agent/tools.rs`, and `src/agent/openai.rs` (654 lines total) implement the v1 tool-call-based agent loop, fully superseded by v2 (`loop_v2.rs`). The only remaining caller is `run_inner()` in `orchestration.rs` (the legacy CLI path). Removing these requires migrating or deprecating the legacy CLI path.

## 3. Blanket `#![allow(dead_code)]` cleanup

~10 files have `#![allow(dead_code)]` at the top where the code is actively used. Remove the blanket allows and address any actual dead code warnings individually.

## 4. System prompt splitting

`src/agent/context.rs` contains a 116-line `format!()` macro building the system prompt. Split into section builder functions (e.g., `build_interaction_guidelines()`, `build_output_format()`) for readability and testability.

## 5. `recording.rs` `format_caption()` decomposition

Mixed concerns (text truncation, layout calculation, magic numbers for font sizes/margins). Extract into smaller functions with named constants.

## 6. `task.rs` `validate()` decomposition

188-line monolith validating setup steps, evaluator config, metrics, and app config. Split into `validate_setup_steps()`, `validate_evaluator()`, `validate_metrics()`, `validate_app_config()`.

## 7. Shared constants dedup

`DEFAULT_STEP_TIMEOUT_SECS` is defined in both `src/agent/pyautogui.rs` and `src/agent/loop_v2.rs`. Consolidate into a single location (e.g., `src/agent/mod.rs` or a shared constants module).

## ~~8. `is_context_length_error` false-positive on `max_tokens`~~
Removed the overly broad `"max_tokens"` pattern — the remaining patterns cover all real context length errors without false-positives on parameter validation messages.
