<img width="615" height="277" alt="Screenshot 2026-04-01 at 20 25 14" src="https://github.com/user-attachments/assets/64d67932-1aea-4022-bd5c-146e768d2069" />


Desktest is a general computer use CLI for automated end-to-end virtualised testing of desktop applications using LLM-powered agents. It spins up a disposable Docker container (Linux) or Tart VM (macOS) with a desktop environment, deploys your app, and runs an agent that interacts with it like a real user — clicking, typing, and reading the screen. Deterministic programmatic checks then validate correctness.

> **Warning:** Desktest is beta software under active development. APIs, task schema, and CLI flags may change between releases.

## Agent Quickstart

Copy-paste the following prompt into Claude Code (or any coding agent) to install desktest and set up the agent skill:

> Install the desktest CLI by running `curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh`. Then copy `skills/desktest-skill.md` from the desktest repo (https://raw.githubusercontent.com/Edison-Watch/desktest/master/skills/desktest-skill.md) to `~/.claude/skills/desktest/SKILL.md` so you have context on how to use it.

<img width="831" height="549" alt="Screenshot 2026-03-25 at 21 04 46" src="https://github.com/user-attachments/assets/fc07dd36-2c49-4ac7-ada9-105a55e85629" />


## Features

**Testing & Execution**
- **[Structured JSON task definitions](#task-definition)** with schema validation
- **OSWorld-style agent loop**: observe (screenshot + accessibility tree) → think → act (PyAutoGUI) → repeat
- **Programmatic evaluation**: file comparison, command output checks, file existence, exit codes
- **Three validation modes**: LLM-only, programmatic-only, or hybrid (both must pass)
- **Test suites**: run a directory of tests with aggregated results

**Observability & Debugging**
- **Live monitoring dashboard**: real-time web UI to watch agent actions as they happen
- **Video recording**: ffmpeg captures every test session
- **Trajectory logging**: step-by-step JSONL logs with screenshots and accessibility trees
- **Interactive mode**: step through agent actions one at a time for debugging

**Extensibility**
- **Custom Docker images**: bring your own image for apps with complex dependencies
- **[Attach mode](docs/attach-mode.md)**: connect to an already-running container for integration with external orchestration
- **[CI integration](docs/ci.md)**: run tests in GitHub Actions, Cirrus CI, EC2 Mac, and other CI environments
- **[Remote monitoring](docs/remote-monitoring.md)**: access the dashboard and VNC from another machine via SSH or direct network access
- **QA mode** (`--qa`): agent reports application bugs it encounters as structured markdown reports
- **Slack notifications**: send QA bug reports to Slack channels via Incoming Webhooks

## Developer Workflows

### Workflow 1: Test Authoring (Explore → Codify → CI)

Build deterministic regression tests by watching an LLM agent explore your app, then converting the trajectory into a replayable script:

```
1. EXPLORE   →  desktest run task.json --monitor     # LLM agent explores your app (watch live!)
2. REVIEW    →  desktest review desktest_artifacts/   # Inspect trajectory in web viewer
3. CODIFY    →  desktest codify trajectory.jsonl --overwrite task.json  # Generate script + update task JSON
4. REPLAY    →  desktest run task.json --replay       # Deterministic replay (no LLM, no API costs)
5. CI        →  Run codified tests on every commit
```

> **Step 4 detail:** `--replay` switches to fully deterministic execution — the codified PyAutoGUI script drives the app directly with zero LLM calls and zero API costs. The same evaluator metrics validate the result. Without `--replay`, the LLM agent runs as normal (useful for re-recording).

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

## CLI-Based Providers (No API Key Needed)

Desktest can shell out to locally-installed coding agent CLIs instead of calling LLM APIs directly:

### Claude Code CLI

```bash
desktest run task.json --provider claude-cli
```

Uses your existing Claude Code subscription. Each step shells out to `claude -p`, saves trajectory screenshots and accessibility trees as files, and instructs Claude to read them via its Read tool. See [docs/claude-cli-provider.md](docs/claude-cli-provider.md).

### OpenAI Codex CLI

```bash
desktest run task.json --provider codex-cli
```

Uses your existing ChatGPT login or `CODEX_API_KEY`. Screenshots are passed directly as `-i` flags (native image input), and accessibility trees are embedded inline in the prompt. See [docs/codex-cli-provider.md](docs/codex-cli-provider.md).

## Requirements

**To run tests (Linux — default):**
- Linux or macOS host
- Docker daemon running (Docker Desktop, OrbStack, Colima, etc.)
- An LLM API key (OpenAI, Anthropic, or compatible), **or** a CLI-based provider: [Claude Code](https://claude.ai/code) (`--provider claude-cli`) or [Codex CLI](https://github.com/openai/codex) (`--provider codex-cli`) — not needed for `--replay` mode

<details>
<summary><b>To run tests (macOS apps)</b></summary>

- Apple Silicon Mac (M1 or later) running macOS 13+
- [Tart](https://github.com/cirruslabs/tart) installed (`brew install cirruslabs/cli/tart`)
- [sshpass](https://github.com/hudochenkov/sshpass) installed (`brew install hudochenkov/sshpass/sshpass`) — for golden image provisioning
- A golden image prepared via `desktest init-macos` (handles Python, PyAutoGUI, a11y helper, TCC permissions, and SSH key setup automatically)
- An LLM API key (same as Linux), **or** `--provider claude-cli` to use your Claude Code subscription
- **2-VM limit**: Apple's macOS SLA and Virtualization.framework permit max 2 macOS VMs simultaneously per Mac. See [macOS Support](docs/macos-support.md) for details and Apple TOS compliance.

</details>

<details>
<summary><b>To run tests (Windows apps — planned)</b></summary>

- Windows VM support is planned but not yet designed. Expected to use QEMU/libvirt or Hyper-V with Windows VMs, RDP or VNC for display access, and UI Automation APIs for accessibility. Details TBD.

</details>

**To build from source (optional):**
- Rust toolchain (`cargo`)
- Git
- Xcode Command Line Tools (for macOS a11y helper binary — macOS only)

Run `desktest doctor` to verify your setup.

## Installation

```bash
# One-line install (downloads pre-built binary)
curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh

# Or build from source
git clone https://github.com/Edison-Watch/desktest.git
cd desktest
make install_cli
```

## Main Commands

<details>
<summary>Expand</summary>

```bash
# Validate a task file
desktest validate elcalc-test.json

# Run a single test
desktest run elcalc-test.json

# Run a test suite
desktest suite tests/

# Interactive debugging (starts container, prints VNC info, pauses)
desktest interactive elcalc-test.json

# Step-by-step mode (pause after each agent action)
desktest interactive elcalc-test.json --step
```

</details>

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
  review        Generate interactive HTML trajectory viewer
  logs          View trajectory logs in the terminal (supports --steps N, N-M, or N,M,X-Y)
  monitor       Start a persistent monitor server for multi-phase runs
  init-macos    Prepare a macOS golden image for Tart VM testing
  doctor        Check that all prerequisites are installed and configured
  update        Update desktest to the latest release from GitHub

Options:
  --config <FILE>            Config JSON file (optional; API key can come from env vars)
  --output <DIR>             Output directory for results (default: ./test-results/)
  --debug                    Enable debug logging
  --verbose                  Include full LLM responses in trajectory logs
  --record                   Enable video recording
  --monitor                  Enable live monitoring web dashboard
  --monitor-port <PORT>      Port for the monitoring dashboard (default: 7860)
  --resolution <WxH>         Display resolution (e.g., 1280x720, 1920x1080, or preset: 720p, 1080p)
  --artifacts-dir <DIR>      Directory for trajectory logs, screenshots, and a11y snapshots
  --qa                       Enable QA mode: agent reports app bugs during testing
  --with-bash                Allow the agent to run bash commands inside the container (disabled by default)
  --provider <PROVIDER>      LLM provider: anthropic, openai, openrouter, cerebras, gemini, claude-cli, codex-cli, custom
  --model <MODEL>            LLM model name (overrides config file)
  --api-key <KEY>            API key for the LLM provider (prefer env vars to avoid shell history exposure)
```

## Task Definition

<details>
<summary>Expand</summary>

Tests are defined in JSON files. Here's a complete example that tests a calculator app:

```json
{
  "schema_version": "1.0",        // Required: task schema version
  "id": "elcalc-addition",        // Unique test identifier
  "instruction": "Using the calculator app, compute 42 + 58.",  // What the agent should do
  "completion_condition": "The calculator display shows 100 as the result.",  // Success criteria (optional)
  "app": {
    "type": "appimage",            // How to deploy the app (see App Types below)
    "path": "./elcalc-2.0.3-x86_64.AppImage"
  },
  "evaluator": {
    "mode": "llm",                 // Validation mode: "llm", "programmatic", or "hybrid"
    "llm_judge_prompt": "Does the calculator display show the number 100 as the result? Answer pass or fail."
  },
  "timeout": 120                   // Max seconds before the test is aborted
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
| `macos_tart` | macOS app in a Tart VM — isolated, destroyed after test (see [macOS Support](docs/macos-support.md)) |
| `macos_native` | macOS app on host desktop, no VM isolation (see [macOS Support](docs/macos-support.md)) |
| `windows` | **(Planned)** Windows app in a VM — details TBD |

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

</details>

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

### Slack Notifications

Optionally send bug reports to Slack as they're discovered. Add an `integrations` section to your config JSON:

```json
{
  "integrations": {
    "slack": {
      "webhook_url": "https://hooks.slack.com/services/T.../B.../xxx",
      "channel": "#qa-bugs"
    }
  }
}
```

Or set the `DESKTEST_SLACK_WEBHOOK_URL` environment variable (takes precedence over config). The `channel` field is optional — webhooks already target a default channel. Notifications are fire-and-forget and never block the test.

## Architecture

<details>
<summary>Expand</summary>

```
Developer writes task.json
        │
        ▼
   ┌──────────────┐
   │ desktest CLI  │  validate / run / suite / interactive
   └────┬─────────┘
        │
        ├─── Linux ────────────────────┐     ├─── macOS ────────────────────┐
        │  Docker Container            │     │  Tart VM (or native host)    │
        │  Xvfb + XFCE + x11vnc       │     │  Native macOS desktop        │
        │  PyAutoGUI (X11)             │     │  PyAutoGUI (Quartz)          │
        │  pyatspi (AT-SPI2)           │     │  a11y-helper (AXUIElement)   │
        │  scrot (screenshot)          │     │  screencapture (screenshot)  │
        └──────────┬───────────────────┘     └──────────┬───────────────────┘
                   │ screenshot + a11y tree              │
                   └──────────────┬──────────────────────┘
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

</details>

## Artifacts

<details>
<summary>Expand</summary>

Each test run produces:

```
test-results/
  results.json                # Structured test results (always)

desktest_artifacts/
  recording.mp4               # Video of the test session (with --record)
  trajectory.jsonl            # Step-by-step agent log (always)
  agent_conversation.json     # Full LLM conversation (always)
  step_001.png                # Screenshot per step (always)
  step_001_a11y.txt           # Accessibility tree per step (always)
  bugs/                       # Bug reports (with --qa)
    BUG-001.md                # Individual bug report (with --qa)
```

</details>

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Test passed |
| 1 | Test failed |
| 2 | Configuration error |
| 3 | Infrastructure error |
| 4 | Agent error |

## Environment Variables

<details>
<summary>Expand</summary>

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `CEREBRAS_API_KEY` | Cerebras API key |
| `GEMINI_API_KEY` | Gemini API key |
| `CODEX_API_KEY` | Codex CLI API key (alternative to ChatGPT login) |
| `LLM_API_KEY` | Fallback API key for any provider |
| `DESKTEST_SLACK_WEBHOOK_URL` | Slack Incoming Webhook URL for QA bug notifications (overrides config) |
| `GITHUB_TOKEN` | GitHub token (used by `desktest update`) |

</details>
