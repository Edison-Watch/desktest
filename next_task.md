# Next Tasks — Medium-Impact Improvements

Items identified during the structural refactoring that are worth addressing but were out of scope for the split.

## ~~1. Legacy agent removal~~ ✅

~~`src/agent/mod.rs`, `src/agent/tools.rs`, and `src/agent/openai.rs` (654 lines total) implement the v1 tool-call-based agent loop, fully superseded by v2 (`loop_v2.rs`).~~ Done — `tools.rs` and `openai.rs` removed, `mod.rs` now contains only module declarations.

## 2. Blanket `#![allow(dead_code)]` cleanup

~8 files have `#![allow(dead_code)]` at the top where the code is actively used. Remove the blanket allows and address any actual dead code warnings individually.

## ~~3. System prompt splitting~~ ✅

~~`src/agent/context.rs` contains a ~93-line `format!()` macro building the system prompt. Split into section builder functions (e.g., `build_interaction_guidelines()`, `build_output_format()`) for readability and testability.~~ Done — extracted `PlatformText` struct, `build_bash_section()`, and `build_qa_section()` from the monolithic function.

## 4. `recording.rs` `format_caption()` decomposition

Mixed concerns (text truncation, layout calculation, magic numbers for font sizes/margins). Extract into smaller functions with named constants.

## ~~5. `task.rs` `validate()` decomposition~~ ✅

~~~170-line monolith validating setup steps, evaluator config, metrics, and app config. Split into `validate_setup_steps()`, `validate_evaluator()`, `validate_metrics()`, `validate_app_config()`.~~ Done — split into `validate_setup_steps()`, `validate_evaluator()`, `validate_metric()`, `validate_early_exit()`, and `validate_app_config()`.

## ~~6. Shared constants dedup~~ ✅

~~`DEFAULT_STEP_TIMEOUT_SECS` is defined in both `src/agent/pyautogui.rs` and `src/agent/loop_v2.rs`. Consolidate into a single location (e.g., `src/agent/mod.rs` or a shared constants module).~~ Done — consolidated into `src/agent/mod.rs`.
