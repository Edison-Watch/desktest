# Examples

Example task files and Dockerfiles for eyetest.

## Task Files

### `gedit-save.json` — GTK App (Folder Deploy)

A simple test that opens a text file in gedit, adds a line, and saves.
Uses the `folder` app deploy type with a local application directory.

```bash
eyetest run examples/gedit-save.json
eyetest run examples/gedit-save.json --monitor   # Watch live at http://localhost:7860
eyetest interactive examples/gedit-save.json
```

### `libreoffice-calc.json` — Custom Docker Image

A spreadsheet test that enters values and a formula in LibreOffice Calc.
Uses the `docker_image` app type with a pre-built custom image.

```bash
# Build the custom image first
docker build -t tent-libreoffice:latest -f examples/Dockerfile.libreoffice .

# Run the test
eyetest run examples/libreoffice-calc.json

# Or interactively
eyetest interactive examples/libreoffice-calc.json
```

### `electron-todo.json` — Electron App (Folder Deploy)

A minimal Electron todo app that demonstrates testing Electron applications.
Uses the `folder` app deploy type with `electron: true` for Node.js support.

```bash
# Build the electron Docker image first
docker build -t eyetest-desktop:latest docker/
docker build -f docker/Dockerfile.electron -t eyetest-desktop:electron docker/

# Run the test
eyetest run examples/electron-todo.json
```

See [ELECTRON_QUICKSTART.md](ELECTRON_QUICKSTART.md) for a complete guide to testing Electron apps.

## Custom Docker Images

`Dockerfile.libreoffice` shows how to create a compatible custom image.

### Required Dependencies

Custom images must include these packages for eyetest to work:

| Category | Packages |
|----------|----------|
| Display | `xvfb`, `x11vnc`, `openbox` |
| Tools | `scrot`, `xdotool`, `ffmpeg` |
| Accessibility | `at-spi2-core`, `libatspi2.0-0` |
| Python | `python3`, `python3-pyautogui`, `python3-xlib`, `python3-pyatspi`, `python3-pyperclip` |
| Clipboard | `xclip` |
| D-Bus | `dbus`, `dbus-x11` |

You must also copy the helper scripts from `docker/`:
- `docker/get-a11y-tree.py` → `/usr/local/bin/get-a11y-tree`
- `docker/execute-action.py` → `/usr/local/bin/execute-action`
- `docker/entrypoint.sh` → `/usr/local/bin/entrypoint.sh`

### Validation

eyetest validates custom images at startup. If a required dependency is missing, it exits with code 2 and a clear error message.

```bash
# Validate a task file without running
eyetest validate examples/libreoffice-calc.json
```

## Live Monitoring

Any example can be run with the `--monitor` flag to open a real-time web dashboard:

```bash
# Single test with live dashboard
eyetest run examples/gedit-save.json --monitor

# Suite with progress tracking
eyetest suite examples/ --monitor

# Custom port
eyetest run examples/gedit-save.json --monitor --monitor-port 8080
```

Open `http://localhost:7860` in your browser to watch the agent's screenshots, thoughts, and actions stream in as each step completes. The dashboard uses the same UI as `eyetest review`.

## Task JSON Schema

See `src/task.rs` for the full schema definition. Key fields:

```json
{
  "schema_version": "1.0",
  "id": "unique-test-id",
  "instruction": "What the agent should do",
  "app": { "type": "appimage|folder|docker_image", "..." : "..." },
  "config": [ { "type": "execute|copy|open|sleep", "..." : "..." } ],
  "evaluator": {
    "mode": "llm|programmatic|hybrid",
    "metrics": [ { "type": "file_exists|command_output|...", "..." : "..." } ]
  },
  "timeout": 120
}
```
