# Desktest - Desktop App Test Runner

Desktest is a CLI tool for automated end-to-end testing of Linux desktop applications using LLM-powered agents. It spins up a disposable Docker container with a virtual desktop, deploys your app, and runs an agent that interacts with it like a real user — clicking, typing, and reading the screen. Deterministic programmatic checks then validate correctness.

> **Warning:** Desktest is beta software under active development. APIs, task schema, and CLI flags may change between releases.

## Agent Quickstart

Copy-paste the following prompt into Claude Code (or any coding agent) to install desktest and set up the agent skill:

> Install the desktest CLI by running `curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh`. Then copy `skills/desktest-skill.md` from the desktest repo (https://raw.githubusercontent.com/Edison-Watch/desktest/master/skills/desktest-skill.md) to `~/.claude/skills/desktest/SKILL.md` so you have context on how to use it.

## Features

- **Structured JSON task definitions** with schema validation
- **OSWorld-style agent loop**: observe (screenshot + accessibility tree) → think → act (PyAutoGUI) → repeat
- **Programmatic evaluation**: file comparison, command output checks, file existence, exit codes
- **Three validation modes**: LLM-only, programmatic-only, or hybrid (both must pass)
- **Test suites**: run a directory of tests with aggregated results
- **Video recording**: ffmpeg captures every test session
- **Trajectory logging**: step-by-step JSONL logs with screenshots and accessibility trees
- **Custom Docker images**: bring your own image for apps with complex dependencies
- **Live monitoring dashboard**: real-time web UI to watch agent actions as they happen
- **Interactive mode**: step through agent actions one at a time for debugging
- **[Attach mode](docs/attach-mode.md)**: connect to an already-running container for integration with external orchestration
- **QA mode** (`--qa`): agent reports application bugs it encounters as structured markdown reports

## Developer Workflows

### Workflow 1: Test Authoring (Explore → Codify → CI)

Build deterministic regression tests by watching an LLM agent explore your app, then converting the trajectory into a replayable script:

```
1. EXPLORE   →  desktest run task.json --monitor  # LLM agent explores your app (watch live!)
2. REVIEW    →  desktest review desktest_artifacts/  # Inspect trajectory in web viewer
3. CODIFY    →  desktest codify desktest_artifacts/trajectory.jsonl  # Convert to deterministic script
4. REPLAY    →  Add script_replay metric to task.json, then: desktest run task.json  # Run codified test (no LLM)
5. CI        →  Run codified tests on every commit
```

> **Step 4 detail:** `desktest codify` outputs a Python replay script (`desktest_replay.py`), not a task JSON. To replay it, add a `script_replay` metric to your task JSON:
> ```json
> { "type": "script_replay", "script_path": "desktest_replay.py" }
> ```

### Workflow 2: Live Monitoring + Agent-Assisted Debugging

Use desktest as the eyes for your coding agent. You watch the test live, then hand off investigation to your coding agent (e.g. Claude Code) via the CLI-friendly `logs` command:

```
1. RUN       →  desktest run task.json --monitor     # Watch the agent live in the browser
2. DIAGNOSE  →  desktest logs desktest_artifacts/              # Hand off to your coding agent for analysis
                desktest logs desktest_artifacts/ --steps 3-7  # Or drill into specific steps
3. FIX       →  Coding agent reads the logs, diagnoses the issue, and fixes the code
4. RERUN     →  desktest run task.json --monitor     # Verify the fix
```

`--monitor` is designed for human eyes (real-time web dashboard), while `logs` is designed for agent consumption (structured terminal output). Together they close the loop between observing a failure and fixing it.

## Architecture

```
Developer writes task.json
        │
        ▼
   ┌─────────┐
   │ desktest CLI │  validate / run / suite / interactive
   └────┬─────┘
        │
        ▼
   ┌──────────────────────────────────┐
   │  Docker Container                │
   │  ┌──────┐  ┌─────┐  ┌────────┐  │
   │  │ Xvfb │  │XFCE │  │x11vnc  │  │
   │  └──┬───┘  └──┬──┘  └────────┘  │
   │     │  virtual desktop           │
   │  ┌──┴─────────┴──┐              │
   │  │  Your App      │              │
   │  └───────────────┘              │
   └──────────┬───────────────────────┘
              │ screenshot + a11y tree
              ▼
   ┌──────────────────┐
   │  LLM Agent Loop  │  observe → think → act → repeat
   │  (PyAutoGUI code) │
   └────────┬─────────┘
            │
            ▼
   ┌──────────────────┐
   │  Evaluator        │  programmatic checks / LLM judge / hybrid
   └────────┬─────────┘
            │
            ▼
   results.json + recording.mp4 + trajectory.jsonl
```

## Requirements

- Linux or macOS host with Docker installed
- Rust toolchain (`cargo`)
- An LLM API key (OpenAI, Anthropic, or compatible)

## Installation

```bash
# One-line install (downloads pre-built binary)
curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh

# Or build from source
git clone https://github.com/Edison-Watch/desktest.git
cd desktest
make install_cli
```

## Quick Start

```bash
# Build
cargo build --release

# Validate a task file
cargo run -- validate elcalc-test.json

# Run a single test
cargo run -- run elcalc-test.json

# Run a test suite
cargo run -- suite tests/

# Interactive debugging (starts container, prints VNC info, pauses)
cargo run -- interactive elcalc-test.json

# Step-by-step mode (pause after each agent action)
cargo run -- interactive elcalc-test.json --step
```

## CLI

```
desktest [OPTIONS] <COMMAND>

Commands:
  run           Run a single test from a task JSON file
  suite         Run all *.json task files in a directory
  interactive   Start container and pause for debugging
  attach        Attach to an existing running container
  validate      Check task JSON against schema without running
  codify        Convert trajectory to deterministic Python replay script
  review        Generate web-based trajectory review viewer
  logs          View trajectory logs in the terminal (supports --steps N, N-M, or N,M,X-Y)

Options:
  --config <FILE>        Config JSON file (optional; API key can come from env vars)
  --output <DIR>         Output directory for results (default: ./test-results/)
  --debug                Enable debug logging
  --verbose              Include full LLM responses in trajectory logs
  --record               Enable video recording
  --monitor              Enable live monitoring web dashboard
  --monitor-port <PORT>  Port for the monitoring dashboard (default: 7860)
  --qa                   Enable QA mode: agent reports app bugs during testing
  --with-bash            Allow the agent to run bash commands inside the container (disabled by default)
```

## Task Definition

Tests are defined in JSON files. Here's a complete example that tests a calculator app:

```json
{
  "schema_version": "1.0",
  "id": "elcalc-addition",
  "instruction": "Using the calculator app, compute 42 + 58.",
  "completion_condition": "The calculator display shows 100 as the result.",
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

The optional `completion_condition` field lets you define the success criteria separately from the task instruction. When present, it's appended to the instruction sent to the agent, and rendered as a collapsible section in the review and live dashboards.

See `examples/` for more examples including folder deploys and custom Docker images.

### App Types

| Type | Description |
|------|-------------|
| `appimage` | Deploy a single AppImage file |
| `folder` | Deploy a directory with an entrypoint script |
| `docker_image` | Use a pre-built custom Docker image |
| `vnc_attach` | Attach to an existing running desktop (see [Attach Mode](docs/attach-mode.md)) |

> **Electron apps**: Add `"electron": true` to your app config to use the `desktest-desktop:electron` image with Node.js pre-installed. See [examples/ELECTRON_QUICKSTART.md](examples/ELECTRON_QUICKSTART.md).

### Evaluation Metrics

| Metric | Description |
|--------|-------------|
| `file_compare` | Compare a container file against an expected file (exact or normalized) |
| `file_compare_semantic` | Parse and compare structured files (JSON, YAML, XML, CSV) |
| `command_output` | Run a command, check stdout (contains, equals, regex) |
| `file_exists` | Check if a file exists (or doesn't) in the container |
| `exit_code` | Run a command, check its exit code |
| `script_replay` | Run a Python replay script, check for REPLAY_COMPLETE + exit 0 |

## Live Monitoring

Add `--monitor` to any `run` or `suite` command to launch a real-time web dashboard that streams the agent's actions as they happen:

```bash
# Watch a single test live
desktest run task.json --monitor

# Watch a test suite with progress tracking
desktest suite tests/ --monitor

# Use a custom port
desktest run task.json --monitor --monitor-port 8080
```

Open `http://localhost:7860` in your browser to see:
- **Live step feed**: screenshots, agent thoughts, and action code appear as each step completes
- **Test info header**: test ID, instruction, VNC link, and max steps
- **Suite progress**: progress bar showing completed/total tests during suite runs
- **Status indicator**: pulsing dot shows connection state (live vs disconnected)

The dashboard uses the same UI as `desktest review` — a sidebar with step navigation, main panel with screenshot/thought/action details. The difference is that steps stream in via Server-Sent Events (SSE) instead of being loaded from a static file.

## QA Mode

Add `--qa` to any `run`, `suite`, or `attach` command to enable bug reporting. The agent will complete its task as normal, but also watch for application bugs and report them as markdown files:

```bash
# Run a test with QA bug reporting
desktest run task.json --qa

# QA mode in a test suite
desktest suite tests/ --qa
```

When `--qa` is enabled:
- The agent gains a `BUG` command to report application bugs it discovers
- Bash access is automatically enabled for diagnostic investigation (log files, process state, etc.)
- Bug reports are written to `desktest_artifacts/bugs/BUG-001.md`, `BUG-002.md`, etc.
- Each report includes: summary, description, screenshot reference, accessibility tree state
- The agent continues its task after reporting — multiple bugs can be found per run
- Bug count is included in `results.json` and the test output

## Artifacts

Each test run produces:

```
test-results/
  results.json                # Structured test results (pass/fail, metrics, duration)

desktest_artifacts/
  recording.mp4               # Video of the test session (with --record)
  trajectory.jsonl            # Step-by-step agent log
  agent_conversation.json     # Full LLM conversation
  step_001.png                # Screenshot per step
  step_001_a11y.txt           # Accessibility tree per step
  bugs/                       # Bug reports (with --qa)
    BUG-001.md                # Individual bug report
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Test passed |
| 1 | Test failed |
| 2 | Configuration error |
| 3 | Infrastructure error |
| 4 | Agent error |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `LLM_API_KEY` | Fallback API key for any provider |

## Legacy CLI

The original CLI format is still supported for backward compatibility:

```bash
cargo run -- config.json instructions.md [--debug] [--interactive]
```
