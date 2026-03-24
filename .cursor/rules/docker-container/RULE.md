---
description: "Docker container constraints — user setup, display, helper scripts. Apply when modifying Dockerfiles, container setup, or helper scripts."
globs:
  - 'docker/**'
  - 'src/docker/**/*.rs'
alwaysApply: false
---

# Docker Container

- Runs as non-root user "tester" inside debian:bookworm-slim
- `~/.Xauthority` MUST exist for the tester user — PyAutoGUI (via python-xlib) crashes with `Xlib.error.XauthError` without it
- Default display resolution: 1920x1080
- Helper scripts: `get-a11y-tree.py` (a11y via pyatspi), `execute-action.py` (PyAutoGUI from stdin), `screenshot_compare.py` (PIL comparison)
- Images: `desktest-desktop:latest` (base), `desktest-desktop:electron` (+ Node.js 20 + Electron deps)
