# Electron App Testing with Eyetest

A step-by-step guide to testing your Electron apps with eyetest.

## Prerequisites

- Docker installed and running
- eyetest binary ([install](#install) or build from source)
- Your Electron app source code

## Install

Download the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/Edison-Watch/eyetest/master/install.sh | bash
```

Or build from source:

```bash
git clone https://github.com/Edison-Watch/eyetest.git
cd eyetest
cargo build --release
# Binary at target/release/eyetest
```

## Step 1: Prepare Your Electron App

### Option A: Source Folder (Development)

Your app needs a startup script that installs dependencies and launches Electron with the right flags.

Create `start.sh` in your app directory:

```bash
#!/bin/bash
set -e
cd /home/tester/your-app-name
npx electron . --no-sandbox --disable-gpu --force-renderer-accessibility 2>&1
```

Key flags:
- `--no-sandbox` — required for running inside Docker containers
- `--disable-gpu` — no GPU available in the virtual desktop
- `--force-renderer-accessibility` — enables the accessibility tree for better agent navigation

> **Important:** Do NOT put `npm install` in `start.sh`. The entrypoint runs after the app window detection timer starts, and `npm install` for Electron downloads a ~180MB binary that will exceed the 30-second timeout. Instead, use a `config` step in your task JSON to install dependencies before the app launches (see [Step 3](#step-3-write-a-task-file)).

### Option B: Pre-built Binary (Release Testing)

Package your app as an AppImage using `electron-builder`:

```json
{
  "build": {
    "linux": {
      "target": "AppImage"
    }
  }
}
```

Then use `app.type = "appimage"` with `"electron": true` in your task JSON. This ensures `--no-sandbox`, `--disable-gpu`, and `--force-renderer-accessibility` are passed to the binary, and the `eyetest-desktop:electron` image is used.

### Option C: Custom Docker Image

For complex setups, extend the electron base image:

```dockerfile
FROM eyetest-desktop:electron
COPY my-app /home/tester/my-app
RUN cd /home/tester/my-app && npm install --production

# IMPORTANT: PyAutoGUI requires ~/.Xauthority to connect to the X display.
# Always ensure this file exists after switching users.
USER tester
RUN touch /home/tester/.Xauthority
```

## Step 2: Build the Electron Docker Image

The electron image adds Node.js and Electron dependencies to the base image:

```bash
# Build the base image first (if not already built)
docker build -t eyetest-desktop:latest docker/

# Build the electron image
docker build -f docker/Dockerfile.electron -t eyetest-desktop:electron docker/
```

## Step 3: Write a Task File

Create `my-test.json`:

```json
{
  "schema_version": "1.0",
  "id": "my-electron-test",
  "instruction": "Click the 'New File' button and type 'Hello World' in the editor",
  "app": {
    "type": "folder",
    "dir": "./my-electron-app",
    "entrypoint": "start.sh",
    "electron": true
  },
  "config": [
    {
      "type": "execute",
      "command": "cd /home/tester/my-electron-app && npm install --production"
    }
  ],
  "evaluator": {
    "mode": "hybrid",
    "metrics": [
      {
        "type": "command_output",
        "command": "cat /home/tester/.config/my-app/state.json",
        "expected": "Hello World",
        "match_mode": "contains"
      }
    ]
  },
  "timeout": 120,
  "max_steps": 10
}
```

The `config` step runs `npm install` after the app folder is deployed but before the app is launched, so it doesn't count against the window detection timeout.

The `"electron": true` flag tells eyetest to:
1. Use the `eyetest-desktop:electron` Docker image (which has Node.js and Electron runtime dependencies)
2. For AppImage deploys, pass `--no-sandbox --disable-gpu --force-renderer-accessibility` flags directly to the binary

**Important for folder deploys:** Since `start.sh` is a shell script (not the Electron binary), you must include all Electron flags in your script directly: `npx electron . --no-sandbox --disable-gpu --force-renderer-accessibility`. Electron does not support env vars for these options — they must be CLI flags.

## Step 4: Run the Test

```bash
# Validate the task file first
eyetest validate my-test.json

# Run with VNC enabled so you can watch
eyetest run my-test.json --config config.json

# Or interactively for debugging
eyetest interactive my-test.json
```

## Step 5: Review the Trajectory

After a test run, review what the agent did:

```bash
eyetest review test-results/ --open
```

This generates an interactive HTML viewer showing each step's screenshot, the agent's reasoning, and the action code.

## Step 6: Codify the Test

Convert a successful trajectory into a deterministic replay script:

```bash
# Generate replay script from all successful steps
eyetest codify test-results/trajectory.jsonl --output test_replay.py

# Or select specific steps from the review UI
eyetest codify test-results/trajectory.jsonl --steps 1,2,5,6 --output test_replay.py
```

## Step 7: Run the Codified Test

Create a task that uses the replay script instead of the LLM:

```json
{
  "schema_version": "1.0",
  "id": "my-electron-test-replay",
  "instruction": "Run the codified test (no LLM needed)",
  "app": {
    "type": "folder",
    "dir": "./my-electron-app",
    "entrypoint": "start.sh",
    "electron": true
  },
  "evaluator": {
    "mode": "programmatic",
    "metrics": [
      {
        "type": "script_replay",
        "script_path": "./test_replay.py"
      }
    ]
  },
  "timeout": 60,
  "max_steps": 1
}
```

## CI Integration

Add to your GitHub Actions workflow:

```yaml
- name: Run eyetest
  run: |
    eyetest run my-test-replay.json
```

Since codified tests are deterministic (no LLM calls), they're fast, reliable, and free.

## Tips

- **Start with interactive mode** (`eyetest interactive`) to understand how your app looks in the virtual desktop
- **Use VNC** to watch tests live: add `"vnc_port": 5900` to your config
- **Accessibility matters**: Electron's `--force-renderer-accessibility` flag helps the agent read your UI. Use semantic HTML and ARIA labels for best results
- **npm install is slow**: Use a `setup` step in your task JSON to run `npm install` before the app launches, or use a custom Docker image with dependencies pre-installed for faster test runs. Never put `npm install` in your `start.sh` — it will exceed the window detection timeout
- **File paths**: In the container, your app folder is at `/home/tester/<dir-name>/`

## Example

See `examples/electron-todo-app/` for a complete working example:

```bash
eyetest run examples/electron-todo.json
```
