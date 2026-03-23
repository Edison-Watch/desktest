---
name: desktest
description: Guide for using the desktest CLI to run, debug, and review desktop app E2E tests. Use when the user asks to run desktest, review test results, diagnose failures, or work with trajectory logs.
user_invocable: false
triggers:
  - desktest
  - desktop test
  - trajectory logs
  - desktest logs
  - desktest run
---

# Desktest CLI Skill

Desktest is a CLI tool for automated E2E testing of Linux desktop apps using LLM agents. It spins up a Docker container with a virtual XFCE desktop, deploys an app, and runs an agent that interacts via screenshots + PyAutoGUI.

## Key Commands

### Running tests

```bash
desktest run task.json                          # Run a single test (headless)
desktest run task.json --monitor                # Run with live web dashboard at http://localhost:7860
desktest run task.json --monitor --with-bash    # Live + let agent use bash for debugging
desktest run task.json --record --verbose       # Record video + full LLM logs
desktest run task.json --resolution 1280x720    # Custom resolution
desktest suite tests/                           # Run all task JSONs in a directory
desktest suite tests/ --filter gedit            # Filter suite by name
desktest run task.json --replay                  # Deterministic replay from codified script (no LLM)
desktest run task.json --qa                     # Run with QA bug reporting mode
desktest suite tests/ --qa                      # QA mode across a full suite
```

### Reviewing results

```bash
desktest logs desktest_artifacts/               # View trajectory in terminal (agent-friendly)
desktest logs desktest_artifacts/ --brief       # Compact summary table
desktest logs desktest_artifacts/ --step 3      # View only step 3 in detail
desktest logs desktest_artifacts/ --steps 3-7   # View steps 3 through 7
desktest logs desktest_artifacts/ --steps 1,3,5-8  # Mix individual steps and ranges
desktest review desktest_artifacts/             # Open interactive HTML viewer in browser
```

### Other commands

```bash
desktest validate task.json                     # Check task JSON schema without running
desktest interactive task.json                  # Start container + pause for manual VNC debugging
desktest interactive task.json --step           # Step through agent actions one at a time
desktest attach task.json --container ID        # Attach to already-running container
desktest codify desktest_artifacts/trajectory.jsonl  # Convert trajectory to deterministic replay script
desktest codify trajectory.jsonl --overwrite task.json  # Generate script + inject replay_script into task JSON
desktest update                                 # Update desktest to the latest GitHub release
desktest update --force                         # Force reinstall even if already on latest
```

## Developer Workflows

### Workflow 1: Test Authoring (Explore -> Codify -> CI)

1. `desktest run task.json --monitor` — LLM agent explores the app (watch live)
2. `desktest review desktest_artifacts/` — Inspect trajectory in browser
3. `desktest codify desktest_artifacts/trajectory.jsonl --overwrite task.json` — Generate script + update task JSON
4. `desktest run task.json --replay` — Deterministic replay (no LLM, no API costs)
5. Add to CI

### Workflow 2: Live Monitoring + Agent-Assisted Debugging

This is the key workflow for coding agents like Claude Code:

1. Human runs `desktest run task.json --monitor` — watches the agent live in the browser
2. Human tells coding agent: "Go look at `desktest logs desktest_artifacts/` and see what the agent got stuck on"
3. Coding agent reads the logs, diagnoses the issue, and fixes the code
4. Human reruns: `desktest run task.json --monitor` — verify the fix

`--monitor` is for human eyes (real-time web dashboard). `logs` is for agent consumption (structured terminal output). Together they close the loop.

## Using `desktest logs` (Important for Agents)

The `logs` command is specifically designed for agent consumption. When asked to review desktest results:

### Step 1: Get the overview

```bash
desktest logs desktest_artifacts/ --brief
```

This prints a compact table:
```
Step   Result       Timestamp                  Thought
--------------------------------------------------------------------------------
1      success      2026-03-22T16:00:01Z       I see the calculator app...
2      success      2026-03-22T16:00:05Z       Now I'll click the 4 button...
3      error        2026-03-22T16:00:10Z       The button didn't respond...
```

### Step 2: Drill into specific steps

Once you identify problematic steps from the brief view, drill in:

```bash
desktest logs desktest_artifacts/ --step 3        # Single step
desktest logs desktest_artifacts/ --steps 3-7     # Range of steps
desktest logs desktest_artifacts/ --steps 1,3,5-8 # Mix singles and ranges
```

This shows full detail for the selected steps: the agent's thought process, the exact PyAutoGUI action code, and the result.

### Step 3: View all steps (if needed)

```bash
desktest logs desktest_artifacts/
```

Shows every step with full detail (thought + action code + result).

### Important notes for `--brief`, `--step`, and `--steps`

- `--brief` and `--step`/`--steps` cannot be used together
- `--step` and `--steps` are mutually exclusive (use one or the other)
- `--steps` supports comma-separated numbers and ranges: `1,3,5-8` means steps 1, 3, 5, 6, 7, 8
- The header always shows: task ID, total steps, final result (PASS/FAIL/ERROR), and duration
- Step numbers come from the trajectory, not sequential indices

## CLI Flags Reference

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Config JSON file (API key can come from env vars) |
| `--output <DIR>` | Output directory for results (default: ./test-results/) |
| `--debug` | Enable debug logging |
| `--verbose` | Include full LLM responses in trajectory logs |
| `--record` | Enable video recording (produces recording.mp4) |
| `--monitor` | Enable live monitoring web dashboard |
| `--monitor-port <PORT>` | Port for dashboard (default: 7860) |
| `--with-bash` | Allow agent to run bash commands inside the container (disabled by default — agent can "cheat") |
| `--qa` | Enable QA bug reporting mode — agent reports app bugs as structured markdown in `bugs/` |
| `--replay` | Use `replay_script` from task JSON for deterministic execution (no LLM, no API costs). Only on `run` subcommand |
| `--resolution <WxH>` | Display resolution (default: 1920x1080) |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Test passed |
| 1 | Test failed |
| 2 | Configuration error |
| 3 | Infrastructure error (Docker, VNC, etc.) |
| 4 | Agent error (API error, unexpected failure) |

## Artifacts

After a test run, artifacts are in `desktest_artifacts/`:

```
desktest_artifacts/
  trajectory.jsonl          # Step-by-step agent log (used by `logs` and `codify`; secrets redacted)
  agent_conversation.json   # Full LLM conversation (secrets redacted)
  step_001.png              # Screenshot per step
  step_001_a11y.txt         # Accessibility tree per step
  recording.mp4             # Video (only with --record)
  task.json                 # Copy of the task definition (secrets redacted)
  bugs/                     # Bug reports (only with --qa)
    BUG-001.md              #   Structured markdown bug report
    BUG-002.md

test-results/
  results.json              # Structured pass/fail results (secrets redacted)
```

## Task JSON Schema

```json
{
  "schema_version": "1.0",
  "id": "unique-test-id",
  "instruction": "What the agent should do",
  "completion_condition": "Optional — when the agent should consider the task done",
  "app": {
    "type": "appimage|folder|docker_image|vnc_attach",
    "path": "./app.AppImage",
    "electron": true
  },
  "secrets": {
    "username": { "env": "APP_USERNAME", "default": "testuser" },
    "password": { "env": "APP_PASSWORD" }
  },
  "config": [
    { "type": "execute", "command": "echo '{{password}}' > /tmp/.token" },
    { "type": "copy", "src": "./file", "dest": "/home/tester/file" },
    { "type": "sleep", "seconds": 2 }
  ],
  "evaluator": {
    "mode": "llm|programmatic|hybrid",
    "llm_judge_prompt": "Did the agent succeed?",
    "metrics": [
      { "type": "file_exists", "path": "/home/tester/output.txt" },
      { "type": "command_output", "command": "cat /tmp/result", "expected": "100", "match_mode": "contains" },
      { "type": "script_replay", "script_path": "./replay.py" }
    ]
  },
  "timeout": 120,
  "max_steps": 15,
  "replay_script": "desktest_replay.py",
  "replay_screenshots_dir": "desktest_artifacts"
}
```

**`replay_script`** (optional): Path to a codified replay script (generated by `desktest codify --overwrite`). When present, `desktest run --replay` uses this script for fully deterministic execution — no LLM, no API costs.

**`completion_condition`** (optional): Lets you define the success criteria separately from the task instruction. When present, it's appended to the instruction sent to the agent and shown as a collapsible section in the review/live dashboards. Useful for long task descriptions where mixing the goal and success criteria makes the prompt hard to read.

## Task Secrets (Environment Variable Credentials)

The `secrets` field lets you pass credentials without hardcoding them in the task JSON. Secrets are:

- **Sourced from environment variables** — each secret declares an `env` key (required) and an optional `default`
- **Substituted** via `{{key}}` syntax in `instruction`, `completion_condition`, and setup step `command` fields
- **Redacted** from all output — trajectory logs, conversation logs, results.json, task.json artifacts, tracing, and step previews all show `[REDACTED]` instead of the actual value
- **Injected** into the container as `DESKTEST_SECRET_{KEY}` environment variables

```json
{
  "secrets": {
    "username": { "env": "APP_USERNAME", "default": "testuser" },
    "password": { "env": "APP_PASSWORD" }
  },
  "instruction": "Log in as {{username}} with password {{password}}"
}
```

- Missing env var without default → `Config` error at load time
- `{{key}}` referencing an undefined secret name → `Config` error at load time
- Values shorter than 3 characters are not redacted (to avoid over-redacting common substrings)

## --with-bash Philosophy

`--with-bash` is disabled by default because the agent can "cheat" — using bash to directly achieve the task goal instead of interacting with the GUI. Enable it when:
- The agent needs to debug issues (e.g., "why am I getting a black screen?")
- You're running in `--qa` mode (implicitly enabled)
- The task genuinely requires terminal interaction

## QA Bug Reporting Mode (`--qa`)

`desktest run task.json --qa` enables QA mode where the agent watches for **application bugs** while completing its task. Key details:

- Automatically enables `--with-bash` so the agent can gather diagnostic evidence (logs, process state, etc.) before filing reports
- Agent uses the `BUG` command (non-terminal, like DONE/FAIL/WAIT) to signal bugs without stopping the test
- Bug reports are written to `desktest_artifacts/bugs/BUG-001.md`, `BUG-002.md`, etc.
- Each report includes: summary, description, screenshot refs, diagnostic evidence, and a11y tree state
- Bug count is tracked in `AgentOutcome` and `results.json`
- Works with all modes: `run`, `suite`, `attach`, `interactive`
- Bug reports are scoped to actual app bugs, NOT PyAutoGUI/infrastructure failures
