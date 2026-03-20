# Desktest - Desktop App Test Runner

Desktest is a CLI tool for automated end-to-end testing of Linux desktop applications using LLM-powered agents. It spins up a disposable Docker container with a virtual desktop, deploys your app, and runs an agent that interacts with it like a real user — clicking, typing, and reading the screen. Deterministic programmatic checks then validate correctness.

> **Warning:** Desktest is beta software under active development. APIs, task schema, and CLI flags may change between releases.

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

## Developer Workflow

```
1. EXPLORE   →  desktest run task.json --monitor  # LLM agent explores your app (watch live!)
2. REVIEW    →  desktest review desktest_artifacts/  # Inspect trajectory in web viewer
3. CODIFY    →  desktest codify desktest_artifacts/trajectory.jsonl  # Convert to deterministic script
4. REPLAY    →  desktest run replay-task.json      # Run codified test (no LLM)
5. CI        →  Run codified tests on every commit
```

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

- Linux host with Docker installed
- Rust toolchain (`cargo`)
- An LLM API key (OpenAI, Anthropic, or compatible)

## Installation

```bash
git clone https://github.com/Edison-Watch/desktest.git
cd desktest

# Install the desktest CLI to ~/.cargo/bin/
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

Options:
  --config <FILE>        Config JSON file (optional; API key can come from env vars)
  --output <DIR>         Output directory for results (default: ./test-results/)
  --debug                Enable debug logging
  --verbose              Include full LLM responses in trajectory logs
  --record               Enable video recording
  --monitor              Enable live monitoring web dashboard
  --monitor-port <PORT>  Port for the monitoring dashboard (default: 7860)
```

## Task Definition

Tests are defined in JSON files. Here's a complete example that tests a calculator app:

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
