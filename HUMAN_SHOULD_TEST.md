# Manual Testing Checklist

Use [elcalc](https://appimage.github.io/elcalc/) (a simple calculator AppImage) as the benchmark app. Work through each section in order.

## 1. Prerequisites

- [ ] Docker installed and running (`docker info`)
- [ ] Rust toolchain installed (`cargo --version`)
- [ ] LLM API key set (`OPENAI_API_KEY` or `ANTHROPIC_API_KEY`)
- [ ] Download elcalc AppImage:
  ```bash
  wget https://github.com/nicedayzhu/elcalc/releases/download/v2.0.3/elcalc-2.0.3-x86_64.AppImage
  chmod +x elcalc-2.0.3-x86_64.AppImage
  ```

## 2. Build

- [ ] `cargo build --release` completes without errors
- [ ] `cargo test` passes (unit tests, no Docker required)

## 3. Create Task File

Create `elcalc-test.json`:

```json
{
  "schema_version": "1.0",
  "id": "elcalc-addition",
  "instruction": "Using the calculator app, compute 42 + 58 and verify the result shows 100.",
  "app": {
    "type": "appimage",
    "path": "./elcalc-2.0.3-x86_64.AppImage"
  },
  "evaluator": {
    "mode": "llm",
    "llm_judge_prompt": "Does the calculator display show the number 100 as the result? Answer pass or fail."
  },
  "timeout": 120
}
```

## 4. Validate

- [ ] `cargo run -- validate elcalc-test.json` exits 0
- [ ] Try a malformed JSON file — confirm it exits non-zero with a clear error

## 5. Single Test Run

- [ ] `cargo run -- run elcalc-test.json --config config.json` completes
- [ ] Check exit code: 0 = pass, 1 = fail
- [ ] Verify `test-results/results.json` exists and contains pass/fail, duration, metric scores

## 6. Interactive Mode

- [ ] `cargo run -- interactive elcalc-test.json` starts and prints VNC connection info
- [ ] Connect via VNC client to the printed address
- [ ] Verify XFCE desktop is visible with elcalc running
- [ ] Ctrl+C cleanly stops the container

## 7. Step Mode

- [ ] `cargo run -- interactive elcalc-test.json --step` starts
- [ ] Agent pauses after each action and waits for input
- [ ] You can see each screenshot/action before the next step
- [ ] Stepping through to completion produces a result

## 8. Video Recording

- [ ] After a `run` with `--record`, verify `desktest_artifacts/recording.mp4` exists
- [ ] Play the video — it should show the full desktop session
- [ ] Run without `--record` and confirm no mp4 is produced

## 9. Trajectory Log

- [ ] Verify `desktest_artifacts/trajectory.jsonl` exists after a run
- [ ] Each line is valid JSON with fields: step number, action, screenshot reference
- [ ] Running with `--verbose` includes full LLM responses in the log

## 10. Suite Run

- [ ] Create a directory `elcalc-suite/` with 2+ task JSON files (e.g., addition and subtraction)
- [ ] `cargo run -- suite elcalc-suite/ --config config.json` runs all tasks
- [ ] Check `test-results/suite-results.json` for aggregated pass/fail counts
- [ ] Individual task results are in subdirectories

## 11. Custom Docker Image

- [ ] Create a task with `"type": "docker_image"` pointing to a custom image
- [ ] Verify the container starts from that image instead of the default
- [ ] App inside the custom image is accessible to the agent

## 12. Attach Mode

- [ ] Start a container manually: `docker run -d --name test-attach desktest-desktop:latest`
- [ ] Create a task file with `"app": {"type": "vnc_attach"}`
- [ ] `cargo run -- attach attach-task.json --container test-attach` connects and runs
- [ ] Verify desktest does NOT stop or remove the container after completion
- [ ] Try with a non-existent container name — confirm exit code 3 with clear error
- [ ] Clean up: `docker rm -f test-attach`

## 13. Error Cases

- [ ] **Bad task file:** `cargo run -- validate not-valid.json` → exit code 2, clear error message
- [ ] **Missing API key:** Unset all API key env vars, run a test → exit code 2, error mentions missing key
- [ ] **Unreachable container:** Kill Docker mid-run → exit code 3, infra error reported
- [ ] **Timeout:** Set `"timeout": 5` in a task, run it → agent times out, exit code 1 or 4
