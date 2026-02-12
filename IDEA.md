# MVP Plan (updated design decisions)

## Problem

Automated end-to-end testing of desktop apps is brittle and hard to specify in a platform-agnostic way.

## Idea

Run the application under test inside a lightweight, containerised Linux desktop environment. An LLM acts as an interactive tester by controlling mouse/keyboard and taking screenshots. It follows reproduction steps, verifies expected behaviour, and produces a pass/fail verdict with reasoning.

## MVP scope

- The harness is a CLI executable targeting Linux (developed and run on Debian x86_64).
- It takes exactly three arguments:
  1) path to a config file
  2) path to an instructions Markdown file
  3) debug mode flag
- The app under test runs on Linux and is either:
  - an AppImage, or
  - an executable inside a folder with all required resources
- The desktop environment is XFCE4 with common “normal desktop” features (system tray, notifications, etc).
- Internet access inside the container is required.
- Security hardening is out of scope for the MVP (this is trusted, local testing).

## High-level runtime architecture

The implementation should use the simplest and fastest approach available on the dev machine. The dev machine already has Docker/containerd installed; anything that runs reliably there is acceptable.

Recommended MVP stack (no display manager):
- Container runtime: Docker (or Podman if preferred, but Docker is assumed available)
- Virtual display: Xvfb
- Desktop session: xfce4-session (no lightdm/DM)
- VNC server: x11vnc (or TigerVNC equivalent if simpler)
- Input injection: xdotool (inside container), invoked from host via `docker exec`
- Screenshots: captured from the running X session (preferably via container tooling; saved to artifacts dir and also provided to the LLM)

## Config

- Format: JSON (for simplicity)
- Purpose: avoid erroneous runs; validate schema before doing anything else
- Artifacts location: current working directory (where the CLI is invoked), unless overridden later

Config must include at least:
- `openai_api_key`
- `openai_model` (default: best available vision-capable model; configurable)
- Container settings:
  - `display_width` (default 1280)
  - `display_height` (default 800)
  - `vnc_bind_addr` (default `0.0.0.0` so it’s reachable from LAN; configurable)
  - `vnc_port` (default: random free port; configurable)
- App under test:
  - `app_type`: `"appimage"` or `"folder"`
  - For AppImage:
    - `app_path` (host path)
    - Run it “as on a normal distro” (FUSE allowed; avoid extract-and-run unless required)
  - For folder app:
    - `app_dir` (host path)
    - `entrypoint` (path relative to `app_dir` or absolute inside the copied directory)
    - The harness will `chmod +x` the entrypoint inside the container before running
- Timeouts:
  - `startup_timeout_seconds` (default 30) for “desktop ready + app ready” phase

If config validation fails:
- Print a clear error to stderr
- Exit with a dedicated config error code
- Do not start any container

## Execution flow

1. Validate config.
   - If invalid: exit immediately.

2. Create and start a lightweight container that can run XFCE4 + X11 + VNC.
   - Must have internet access (quick connectivity check acceptable; target < 5 seconds for the check).

3. Start a virtual X display in the container (Xvfb) at the configured resolution.

4. Start XFCE4 session (no DM).
   - Goal: user-like XFCE desktop with tray + notifications.

5. Start VNC server bound to `vnc_bind_addr` and chosen `vnc_port`.
   - No password required for MVP (trusted LAN).
   - Print connection details (IP/addr + port) to stdout for debugging.

6. Copy the app under test into the container.
   - AppImage: copy as a file.
   - Folder app: copy the full folder.
   - Folder app entrypoint: `chmod +x` inside the container.

7. Launch the app inside the container.

8. Readiness checks (with timeout).
   - Desktop readiness:
     - Prefer a standard, low-flake check if available (e.g., xfce4-session “ready” semantics).
     - If no robust standard exists, fall back to a pragmatic approach (e.g., wait for xfce4-session process + a stable X root window + ability to take a screenshot).
   - App readiness (sufficient for MVP):
     - Detect whether any “non-default” X window has appeared since baseline, OR
     - detect a window belonging to the launched process, OR
     - detect an additional tray icon beyond XFCE defaults (best-effort; tray detection can be fragile).
   - Default timeout: 30 seconds (configurable).

   On timeout:
   - Tear down the container
   - Print what happened and what stage failed
   - Leave artifacts (screenshots, copied app dir, container home dir state, logs if available) in the artifacts location

9. Agent loop.
   - Create an OpenAI-based multimodal agent using the API key from config.
   - The prompt is:
     - System: “You are a professional software tester operating a Linux XFCE VM via provided tools.”
     - Then:
       - A brief explanation of the available interaction tools
       - The contents of the instructions Markdown file
   - The agent is encouraged to take screenshots frequently to confirm state.
   - The agent can only interact through the defined tools; it cannot execute commands on host/container directly.

10. Stop condition.
   - The loop continues until:
     - the model calls `done(isGood, reasoning)`, or
     - the process exceeds context (compaction is out of scope for MVP)

11. Output result.
   - Print a human-readable transcript to the terminal:
     - key steps, tool calls (at least high level), final verdict, and reasoning
   - Dump screenshots to the artifacts location.
   - Also dump:
     - any changes in the container user’s home directory
     - the app’s directory (post-run state)
     - any app logs captured (best-effort)

12. Cleanup.
   - Tear down container on success/failure.
   - Leave artifacts on host.

## Agent tools (MVP)

Mouse/keyboard:
- `moveMouse(posX: int, posY: int) -> None`
- `leftClick() -> None`
- `doubleClick() -> None`
- `rightClick() -> None`
- `middleClick() -> None`
- `scrollUp(ticks: int) -> None`
- `scrollDown(ticks: int) -> None`
- `dragLeftClickMouse(startX: int, startY: int, endX: int, endY: int) -> None`

Keyboard:
- `pressAndHoldKey(key: string, milliseconds: int, modifiers?: string[]) -> None`
  - `key` supports common special keys (Enter, Tab, Esc, Backspace, arrows, etc)
  - `modifiers` supports Ctrl/Alt/Shift/Super as needed
- `type(str: string) -> None`

Vision:
- `screenshot() -> Image`
  - Must capture the full virtual display at configured resolution
  - Each screenshot is also saved to artifacts immediately

Termination:
- `done(isGood: bool, reasoning: string) -> None`

## Debug mode

When debug mode is enabled:
- Print additional logs about container creation, process start, readiness checks, and tool execution
- Still print VNC connection details

## Exit codes (proposed)

- `0`: test completed and passed (`done(isGood=true)`)
- `1`: test completed and failed (`done(isGood=false)`)
- `2`: config invalid
- `3`: infrastructure/startup failure (container/desktop/vnc/app could not be started or readiness timed out)
- `4`: agent failure (API error, unexpected tool failure, etc)

## Non-goals (for MVP)

- Strong sandboxing / hostile instruction sets
- Cross-platform host support
- Step limits / budgeting controls
- Report file formats beyond terminal output (JSON/HTML/etc)
- Robust tray-icon enumeration across themes/plugins (best-effort only)