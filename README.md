# Tent - Desktop App Test Runner

Tent is a CLI tool for automated end-to-end testing of Linux desktop applications using LLM-powered agents. It spins up a disposable Docker container with a virtual desktop, deploys your app, and runs an agent that interacts with it like a real user — clicking, typing, and reading the screen. Deterministic programmatic checks then validate correctness.

> **Warning:** Tent is beta software under active development. APIs, task schema, and CLI flags may change between releases.

## Features

- **Structured JSON task definitions** with schema validation
- **OSWorld-style agent loop**: observe (screenshot + accessibility tree) → think → act (PyAutoGUI) → repeat
- **Programmatic evaluation**: file comparison, command output checks, file existence, exit codes
- **Three validation modes**: LLM-only, programmatic-only, or hybrid (both must pass)
- **Test suites**: run a directory of tests with aggregated results
- **Video recording**: ffmpeg captures every test session
- **Trajectory logging**: step-by-step JSONL logs with screenshots and accessibility trees
- **Custom Docker images**: bring your own image for apps with complex dependencies
- **Interactive mode**: step through agent actions one at a time for debugging

## Architecture

```
Developer writes task.json
        │
        ▼
   ┌─────────┐
   │ tent CLI │  validate / run / suite / interactive
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

## Quick Start

```bash
# Build
cargo build --release

# Validate a task file
cargo run -- validate examples/gedit-save.json

# Run a single test
cargo run -- run examples/gedit-save.json --config config.json

# Run a test suite
cargo run -- suite examples/ --config config.json

# Interactive debugging (starts container, prints VNC info, pauses)
cargo run -- interactive examples/gedit-save.json

# Step-by-step mode (pause after each agent action)
cargo run -- interactive examples/gedit-save.json --step
```

## CLI

```
tent [OPTIONS] <COMMAND>

Commands:
  run           Run a single test from a task JSON file
  suite         Run all *.json task files in a directory
  interactive   Start container and pause for debugging
  validate      Check task JSON against schema without running

Options:
  --config <FILE>    Config JSON file (optional; API key can come from env vars)
  --output <DIR>     Output directory for results (default: ./test-results/)
  --debug            Enable debug logging
  --verbose          Include full LLM responses in trajectory logs
  --no-recording     Disable video recording
```

## Task Definition

Tests are defined in JSON files. See `examples/` for complete examples.

```json
{
  "schema_version": "1.0",
  "id": "my-test",
  "instruction": "Open the file, add a line, and save",
  "app": {
    "type": "folder",
    "app_dir": "./my-app",
    "entrypoint": "run.sh"
  },
  "config": [
    { "type": "execute", "command": "echo 'hello' > /tmp/test.txt" },
    { "type": "copy", "src": "./fixtures/data.txt", "dest": "/home/tester/data.txt" }
  ],
  "evaluator": {
    "mode": "programmatic",
    "metrics": [
      { "type": "file_exists", "path": "/home/tester/output.txt" },
      { "type": "command_output", "command": "cat /home/tester/output.txt", "expected": "hello world", "match_mode": "contains" }
    ]
  },
  "timeout": 120
}
```

### App Types

| Type | Description |
|------|-------------|
| `appimage` | Deploy a single AppImage file |
| `folder` | Deploy a directory with an entrypoint script |
| `docker_image` | Use a pre-built custom Docker image |

### Evaluation Metrics

| Metric | Description |
|--------|-------------|
| `file_compare` | Compare a container file against an expected file (exact or normalized) |
| `file_compare_semantic` | Parse and compare structured files (JSON, YAML, XML, CSV) |
| `command_output` | Run a command, check stdout (contains, equals, regex) |
| `file_exists` | Check if a file exists (or doesn't) in the container |
| `exit_code` | Run a command, check its exit code |

## Artifacts

Each test run produces:

```
test-results/
  results.json          # Structured test results (pass/fail, metrics, duration)
  recording.mp4         # Video of the test session
  trajectory.jsonl      # Step-by-step agent log
  conversation.json     # Full LLM conversation
  step_001.png          # Screenshot per step
  step_001_a11y.txt     # Accessibility tree per step
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
