# Attach Mode

Use `desktest attach` to run the agent loop against an already-running Docker container, without managing its lifecycle. This is useful when an external orchestration script controls the container and needs desktest to interact with the desktop at specific moments.

## Usage

```bash
# Attach to a running container by name or ID
desktest attach task.json --container my-container

# With a config file
desktest attach task.json --container my-container --config config.json

# With resolution override
desktest attach task.json --container my-container --resolution 1280x720
```

## Task JSON

Task files for attach mode use the `vnc_attach` app type. The `app` section is otherwise ignored since no app is being deployed:

```json
{
  "schema_version": "1.0",
  "id": "approve-dialog",
  "instruction": "A dialog titled 'Confirm' is visible. Click the OK button.",
  "completion_condition": "The dialog has been dismissed and is no longer visible.",
  "app": { "type": "vnc_attach" },
  "evaluator": { "mode": "llm" },
  "timeout": 60,
  "max_steps": 10
}
```

You can also include an optional `note` field for documentation:

```json
"app": {
  "type": "vnc_attach",
  "note": "This task is designed for desktest attach mode"
}
```

## What's skipped vs. what runs

| Step | `desktest run` | `desktest attach` |
|------|:-:|:-:|
| Container creation | Yes | **Skipped** |
| Desktop readiness wait | Yes | **Skipped** |
| Image validation | Yes | **Skipped** |
| App deployment & launch | Yes | **Skipped** |
| Setup steps (`config`) | Yes | Yes |
| Agent loop | Yes | Yes |
| Evaluation | Yes | Yes |
| Artifact collection | Yes | Yes |
| Container cleanup | Yes | **Skipped** |

## Use case: external orchestration

A typical workflow with an orchestration script:

```bash
#!/bin/bash
# 1. Launch a container with your app
docker run -d --name my-app my-desktop-image:latest

# 2. Wait for the app to reach a specific state
# (your logic here — polling logs, waiting for a window, etc.)

# 3. Run desktest against the running container
desktest attach approve-dialog.json --container my-app --config config.json

# 4. Check the result
if [ $? -eq 0 ]; then
  echo "Dialog approved successfully"
fi

# 5. Continue orchestration or clean up
docker rm -f my-app
```

## QA mode

QA bug reporting works with attach mode. Add `--qa` to report application bugs:

```bash
desktest attach task.json --container my-app --qa
```

Bug reports are written to `desktest_artifacts/bugs/`.

## Exit codes

Same as `desktest run`: 0=pass, 1=fail, 2=config error, 3=infra error, 4=agent error.
