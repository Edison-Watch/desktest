# Getting Started with Desktest

A guide to running your first automated desktop test.

## 1. Install

**Pre-built binary (recommended):**

```bash
curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
```

**From source:**

```bash
git clone https://github.com/Edison-Watch/desktest.git
cd desktest
make install_cli
```

**Prerequisites:**

- Docker daemon running (Docker Desktop, OrbStack, Colima, etc.)
- An LLM API key — set one of these environment variables:
  - `ANTHROPIC_API_KEY` (for Anthropic/Claude models)
  - `OPENAI_API_KEY` (for OpenAI models)
  - `OPENROUTER_API_KEY` (for OpenRouter)

> For macOS app testing, see [docs/macos-support.md](docs/macos-support.md).
> For Windows app testing, see [dev-docs/windows-ci-guide.md](dev-docs/windows-ci-guide.md).

## 2. Verify your setup

```bash
desktest doctor
```

This checks that Docker is accessible, your API key is configured, and all dependencies are in place. Fix any issues it reports before continuing.

## 3. Run an example test

The `examples/` directory contains ready-to-run task files. Start with the simplest one — a gedit text editing test:

```bash
desktest run examples/gedit-save.json --monitor
```

This will:
1. Pull/build the desktest Docker image
2. Start a container with an XFCE desktop
3. Deploy the test app (gedit with a text file)
4. Run the LLM-powered agent to complete the task
5. Evaluate whether the task succeeded

Open **http://localhost:7860** in your browser to watch the agent interact with the desktop in real time.

## 4. Review the results

After the test completes, inspect what happened:

```bash
# View the full trajectory in the terminal
desktest logs desktest_artifacts/

# View a compact summary
desktest logs desktest_artifacts/ --brief

# View specific steps
desktest logs desktest_artifacts/ --steps 1-3

# Or open an interactive HTML viewer in your browser
desktest review desktest_artifacts/
```

The `desktest_artifacts/` directory contains screenshots, accessibility tree snapshots, and the full trajectory log (`trajectory.jsonl`).

## 5. Write your own test

Create a task JSON file that describes what to test. Here's a minimal example:

```json
{
  "schema_version": "1.0",
  "id": "my-first-test",
  "instruction": "Open the file /home/tester/notes.txt in gedit, type 'Hello from desktest', and save the file.",
  "app": {
    "type": "folder",
    "dir": "./my-app",
    "entrypoint": "start.sh"
  },
  "config": [
    {
      "type": "execute",
      "command": "echo 'initial content' > /home/tester/notes.txt"
    }
  ],
  "evaluator": {
    "mode": "programmatic",
    "metrics": [
      {
        "type": "command_output",
        "command": "cat /home/tester/notes.txt",
        "match_mode": "contains",
        "expected": "Hello from desktest"
      }
    ]
  },
  "timeout": 120
}
```

**Key fields:**

| Field | Description |
|-------|-------------|
| `instruction` | Natural language prompt telling the agent what to do |
| `app` | How to deploy your application (`folder`, `appimage`, `docker_image`, etc.) |
| `config` | Setup steps run before the agent starts (create files, install packages, etc.) |
| `evaluator` | How to check if the task succeeded (`programmatic`, `llm`, or `hybrid`) |
| `timeout` | Maximum seconds for the agent loop |

Scaffold a new task file interactively:

```bash
desktest init my-test.json
```

Validate it without running:

```bash
desktest validate my-test.json
```

## 6. Create a config file

For repeated runs, create a config JSON file instead of passing flags every time:

```json
{
  "api_key": "sk-your-key-here",
  "provider": "anthropic",
  "model": "claude-sonnet-4-5-20250929",
  "app_type": "folder"
}
```

Then reference it:

```bash
desktest run my-test.json --config config.json
```

Or use environment variables — no config file needed:

```bash
export ANTHROPIC_API_KEY="sk-your-key-here"
desktest run my-test.json
```

## 7. Replay without LLM costs

Once you have a working test, convert the agent's trajectory into a deterministic replay script:

```bash
# Convert trajectory to a Python script
desktest codify desktest_artifacts/trajectory.jsonl --overwrite my-test.json

# Re-run deterministically (no LLM, no API costs)
desktest run my-test.json --replay
```

This is ideal for CI/CD — replay mode executes the exact same PyAutoGUI actions without calling any LLM API.

## Next steps

- Browse more examples in [`examples/`](examples/README.md)
- Run a test suite: `desktest suite examples/`
- Try QA mode for bug hunting: `desktest run task.json --qa`
- Debug interactively: `desktest interactive task.json`
- Set up CI integration: [docs/ci.md](docs/ci.md)
- Test Electron apps: [examples/ELECTRON_QUICKSTART.md](examples/ELECTRON_QUICKSTART.md)
- Test macOS apps: [docs/macos-support.md](docs/macos-support.md)
- Test Windows apps: [dev-docs/windows-ci-guide.md](dev-docs/windows-ci-guide.md)
