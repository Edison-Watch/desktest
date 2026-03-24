---
description: "Replay pipeline — codify, replay script generation, trajectory reconstruction, and review. Apply when working on replay, codify, trajectory, or review features."
globs:
  - 'src/codify.rs'
  - 'src/evaluator/script.rs'
  - 'src/trajectory.rs'
  - 'src/review.rs'
alwaysApply: false
---

# Replay Pipeline

The replay pipeline converts an LLM-driven run into a deterministic, repeatable test:

1. `desktest run` → generates `trajectory.jsonl` + screenshots in artifacts
2. `desktest codify` → converts trajectory to a Python replay script
3. `desktest run --replay` → deterministic execution (no LLM); evaluator reconstructs trajectory
4. `desktest review` → generates HTML viewer from trajectory

## Key Implementation Details

- Replay scripts emit `REPLAY_STEP_DONE:N:thought` markers after each step and capture screenshots via `scrot`
- `evaluate_script_replay` in `evaluator/script.rs` parses these markers, copies screenshots from the container, and writes `trajectory.jsonl`
- `TrajectoryLogger::new()` truncates the file; `TrajectoryLogger::new_append()` appends. ALWAYS use `new_append()` when multiple callers may write to the same trajectory (e.g. multiple ScriptReplay metrics)
- `apply_replay_override()` always places `ScriptReplay` as the first metric and sets `Programmatic` mode
- The orchestration layer skips generic pre/post screenshots when a `ScriptReplay` metric is present (the evaluator writes richer per-step data instead)
- Thought text in `REPLAY_STEP_DONE` markers is capped at 200 chars to keep generated scripts readable
