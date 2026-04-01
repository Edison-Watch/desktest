# macOS Support — Transition Plan

This document describes the phased implementation plan for adding macOS desktop app testing to desktest. It covers architecture decisions, the rollout phases, and what changes in each phase.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Session abstraction | Enum dispatch (`SessionKind`) with `Session` trait | Closed set of backends (Docker, Tart, Native). Zero-overhead dispatch. Native `async fn` in trait — no `async-trait` crate needed. |
| macOS VM | [Tart](https://github.com/cirruslabs/tart) (Apple Virtualization.framework) | Open source, CLI-driven, OCI registry support, ephemeral clone workflow mirrors Docker model. |
| Host ↔ VM communication | Shared directory (`tart run --dir`) + lightweight VM agent + SSH for a11y | Near-instant file access for screenshots via shared dir. SSH localhost used for accessibility tree extraction (LaunchAgent gets restricted Aqua session). Passwordless SSH keys set up during provisioning. |
| Accessibility | Swift helper binary (AXUIElement API) | No pyatspi on macOS. Outputs same TSV format as Linux `get-a11y-tree.py`. Separate binary installed in golden image. |
| Action execution | PyAutoGUI (Quartz/CoreGraphics backend) | Same as Linux, different backend. Requires Screen Recording + Accessibility TCC permissions. |
| Screenshots | `screencapture -x` | Built-in macOS utility, no dependencies. |
| Recording | `screencapture -V` (or simplest alternative) | Built-in, no ffmpeg dependency inside VM. |
| Golden image setup | `desktest init-macos` command | Single command prepares the Tart golden image with all dependencies and permissions. |
| Preflight | macOS is optional mode | `desktest doctor` only warns about macOS deps when macOS tasks are used. Linux path unaffected. |

## Shared-Directory Protocol

Instead of SSH, desktest communicates with the macOS VM through a shared directory mounted via `tart run --dir=shared:/path/to/shared`.

```
shared_dir/
  agent_ready              # Sentinel file — VM agent writes this on startup
  requests/
    cmd_{uuid}.json        # Host writes command request
  responses/
    cmd_{uuid}.result.json # VM agent writes command result
  transfers/
    {files}                # Bidirectional file transfer staging area
```

**Request format:**
```json
{
  "type": "exec|exec_exit_code|exec_stdin|exec_detached|copy_to_vm|copy_from_vm",
  "cmd": ["echo", "hello"],
  "stdin_b64": "optional base64 data",
  "src_path": "/path/in/vm",
  "dest_path": "/path/in/vm"
}
```

**Response format:**
```json
{
  "stdout": "hello\n",
  "exit_code": 0,
  "error": null,
  "duration_ms": 12
}
```

The VM agent is a Python script (~150 lines) that polls `requests/` for new files, executes them via `subprocess.run()`, and writes results to `responses/`. Screenshots, a11y trees, and recordings are written directly to the shared directory — no base64 encoding or network transfer needed.

**Constraint**: `tart run --dir` only works when desktest starts the VM. Attaching to an already-running VM would require SSH. For now, desktest must manage the VM lifecycle.

---

## Phase 1: Extract Session Trait + SessionKind Enum ✅

**Type**: Pure refactor. No behavior change. No new features.

**Goal**: Decouple all modules from `DockerSession` so that alternative backends can be plugged in.

### What Changes

Every function that currently takes `&DockerSession` will take `&SessionKind` instead. The `SessionKind` enum initially has one variant (`Docker(DockerSession)`) and delegates all calls to it.

**New file — `src/session/mod.rs`:**
- `Session` trait with 8 async methods: `exec`, `exec_with_exit_code`, `exec_with_stdin`, `exec_detached`, `exec_detached_with_log`, `copy_into`, `copy_from`, `cleanup`
- `SessionKind` enum: `Docker(DockerSession)` (Tart/Native added later)
- `forward_session!` macro generating `impl Session for SessionKind` by matching on variants
- `impl Session for DockerSession` delegating to existing methods
- `fn as_docker(&self) -> Option<&DockerSession>` for Docker-specific operations

**Modified files (mechanical type signature swap):**

| File | Functions affected |
|------|-------------------|
| `src/orchestration.rs` | `TaskContext.session`, `run_task`, `run_attach`, `run_task_inner`, `run_eval_loop` |
| `src/agent/loop_v2.rs` | `AgentLoopV2.session` field, constructor |
| `src/agent/pyautogui.rs` | `execute_code_block` and related functions |
| `src/setup.rs` | `run_setup_steps`, `run_step` |
| `src/observation.rs` | `capture_observation`, `capture_screenshot_*`, `extract_a11y_tree`, `probe_a11y_timing` |
| `src/readiness.rs` | `wait_for_desktop`, `get_window_list`, `get_stable_window_list`, `wait_for_app_window` |
| `src/recording.rs` | `Recording::start`, `stop`, `update_caption`, `collect` |
| `src/artifacts.rs` | `collect_artifacts` (Docker log collection becomes conditional via `as_docker()`) |
| `src/evaluator/mod.rs` | `run_evaluation`, `evaluate_metric` |
| `src/evaluator/command.rs` | `evaluate_command_output`, `evaluate_file_exists`, `evaluate_exit_code` |
| `src/evaluator/file_compare.rs` | `evaluate_file_compare`, `evaluate_file_compare_semantic` |
| `src/evaluator/script.rs` | `evaluate_script_replay` |
| `src/interactive.rs` | `run_interactive_pause_inner`, `run_interactive_step_inner` |

**Docker-specific operations** (`validate_custom_image`, `deploy_app`, `launch_app`, `docker_client`) stay on `DockerSession` directly. Call sites in `orchestration.rs` use `session.as_docker().unwrap()` — these code paths only execute for Docker-type tasks.

### Verification

- `cargo build` — compiles
- `cargo test` — all unit tests pass
- `cargo test -- --ignored --test-threads=1` — Docker integration tests pass
- Manual: `desktest run examples/gedit-save.json` works identically

---

## Phase 2: Shared-Directory Protocol + TartSession ✅

**Type**: New feature. macOS VM communication layer.

**Depends on**: Phase 1

### What's Built

**`macos/vm-agent.py`** — Python script for inside the VM:
- Watches `shared_dir/requests/` for command files
- Executes via `subprocess.run()`, writes results to `shared_dir/responses/`
- Handles file transfers via `shared_dir/transfers/`
- Writes `agent_ready` sentinel on startup

**`macos/vm-agent-install.sh`** — Installer for golden image (copies agent, creates LaunchAgent plist)

**`src/tart/mod.rs`** — `TartSession`:
- `create()`: `tart clone` golden image → `tart run --dir=...` → wait for `agent_ready`
- `cleanup()`: stop VM, delete clone
- All 8 `Session` trait methods implemented via shared-dir protocol

**`src/tart/protocol.rs`** — Request/response structs, polling logic, timeouts

**`src/session/mod.rs`** — Add `Tart(TartSession)` variant, update forwarding macro

### Verification

- Unit tests for protocol serialization/deserialization
- Integration test (ignored, requires Tart + Apple Silicon): create VM → exec "echo hello" → verify response → cleanup

---

## Phase 3: Swift Accessibility Helper ✅

**Type**: New feature. macOS a11y tree extraction.

**Depends on**: Phase 2

### What's Built

**`macos/a11y-helper/`** — Swift package:
- `Package.swift` manifest
- `Sources/main.swift` — CLI using AXUIElement API
- Walks accessibility tree, outputs TSV matching Linux `get-a11y-tree.py` format
- Flags: `--max-nodes <n>`, `--app-pid <pid>`

**`macos/a11y-helper/build.sh`** — Builds release binary via `swift build -c release`

### What Changes

**`src/observation.rs`** — `extract_a11y_tree` gains an `a11y_cmd` parameter (instead of hardcoding the Linux command path). Callers pass the right command based on session type.

### Verification

- `swift build` compiles on macOS
- Manual: run against TextEdit, verify TSV output matches Linux format
- Linux builds unaffected (Swift is a separate build, not a Cargo dependency)

---

## Phase 4: macOS Orchestration ✅

**Type**: New feature. Wires everything together into a usable macOS testing flow.

**Depends on**: Phases 1–3

### What's Built

**`src/tart/deploy.rs`** — macOS app deployment:
- `.app` bundles: `open -a BundleName` or `open /path/to/App.app`
- Electron dev mode: copy source via shared dir, `npm start` / `npx electron . --force-renderer-accessibility`

**`src/tart/readiness.rs`** — macOS-specific readiness:
- Poll for agent sentinel + verify screencapture works
- Detect app windows via `lsappinfo` (not AppleScript — `osascript` hangs in Tart VMs due to TCC)

**`src/init_macos.rs`** — `desktest init-macos` command:
- Pull Tart base image
- Clone, boot, install vm-agent + a11y helper + Python + PyAutoGUI
- Optional: `--with-electron` installs Node.js
- Save as `desktest-macos:latest`

### What Changes

| File | What changes |
|------|-------------|
| `src/task.rs` | Add `AppConfig::MacosTart { base_image, bundle_id, app_path, launch_cmd, electron }` and `AppConfig::MacosNative { bundle_id, app_path }` |
| `src/config.rs` | Add `AppType::MacosTart`, `AppType::MacosNative`, new `apply_task_app` arms |
| `src/cli.rs` | Add `Command::InitMacos` subcommand |
| `src/main.rs` | Add `mod tart; mod init_macos;`, match arm for `InitMacos` |
| `src/preflight.rs` | Add `check_tart()`. `run_doctor()` shows macOS checks only when relevant |
| `src/orchestration.rs` | `run_task()` matches `MacosTart` → creates `TartSession` instead of `DockerSession` |
| `src/observation.rs` | Platform-aware screenshot command (`scrot` vs `screencapture`) and a11y command |

### Verification

- `desktest init-macos` creates golden image (Apple Silicon Mac required)
- `desktest doctor` shows macOS readiness status
- `desktest run` with a macOS task file runs end-to-end
- All existing Linux/Docker tests unaffected

---

## Phase 5: Integration Testing + Polish ✅

**Type**: Stabilization. Examples, tests, edge cases.

**Depends on**: Phase 4

### What's Built

- `examples/macos-textedit.json` — Example macOS task
- `examples/macos-electron.json` — Example Electron-on-macOS task
- `tests/macos_integration.rs` — Integration tests (all `#[ignore]`)
- `src/session/native.rs` — `NativeSession` for `MacosNative` mode (runs on host desktop, no VM, no isolation)

### What's Polished

- Graceful error messages when Tart not installed
- TCC permission failure detection and clear guidance
- Shared dir cleanup on crash (lingering request/response files)
- `CLAUDE.md` architecture updates
- `docs/macos-support.md` usage examples

### Verification

- Full E2E: `desktest run examples/macos-textedit.json` passes
- Full E2E: `desktest run examples/macos-electron.json` passes
- Existing: `desktest suite examples/` passes on Linux
- `desktest doctor` on Linux shows macOS as "not configured" (informational)

---

## Post-Phase 5: E2E Infrastructure Fixes ✅

**Type**: Bug fixes discovered during first real E2E run on Apple Silicon Mac mini.

**PR**: #90 (`fix/macos-tart-e2e-infra`)

### Issues Discovered and Fixed

| Issue | Root Cause | Fix |
|-------|-----------|-----|
| `osascript` hangs in VM | TCC Automation permission can't be granted programmatically for LaunchAgent processes | Replaced with `lsappinfo visibleProcessList` in `readiness.rs` — no TCC needed |
| Empty accessibility tree | vm-agent LaunchAgent gets restricted Aqua session from macOS | Route a11y-helper through `ssh localhost` — SSH sessions get proper Aqua handle |
| `execute-action` not found | `docker/execute-action.py` was never copied during provisioning | Added copy step to `provision_vm()` and install step to provisioning script |
| PyAutoGUI import errors | vm-agent subprocess used system Python (no PyAutoGUI) | Added `EnvironmentVariables` with Homebrew PATH to LaunchAgent plist |
| TCC grants not honored | Entries had NULL `csreq` column | Generate csreq blobs via `codesign -d -r-` + `csreq` tool |
| `PYTHON_BIN` resolves wrong binary | `command -v python3` runs before `/etc/paths.d/homebrew` takes effect | Prefer `/opt/homebrew/bin/python3` with fallback |
| VM filesystem not flushed | `tart stop` + `child.kill()` interrupts shutdown | Provisioning script ends with `sudo shutdown -h now`; Rust side waits for child to exit |
| `AXIsProcessTrustedWithOptions` false in LaunchAgent | TCC check returns false even when AX API calls succeed | Made check non-fatal (warning instead of exit) |

### Key Architectural Discovery: SSH Localhost for A11y

The most significant finding was that **LaunchAgent processes get a restricted Aqua session** from macOS. Even with valid TCC entries, AXUIElement API calls return empty/minimal trees when the calling process is in a background session context.

The workaround: `ssh localhost /usr/local/bin/a11y-helper`. SSH sessions inherit TCC permissions from `sshd-keygen-wrapper` (pre-granted in Tart base images) and get a proper Aqua session handle, giving full accessibility tree access (976+ lines vs ~22 empty lines).

This requires passwordless SSH keys set up during golden image provisioning.

### Files Changed

| File | What changed |
|------|-------------|
| `src/tart/readiness.rs` | `osascript` → `lsappinfo visibleProcessList` |
| `src/observation.rs` | `MACOS_A11Y_CMD` wraps a11y-helper in `ssh localhost` |
| `src/init_macos.rs` | Install execute-action, SSH keys, TCC grants with csreq, Homebrew PATH, graceful shutdown |
| `macos/vm-agent-install.sh` | `EnvironmentVariables` with Homebrew PATH in LaunchAgent plist |
| `macos/a11y-helper/Sources/A11yHelperCLI/main.swift` | `AXIsProcessTrustedWithOptions` check non-fatal |

---

## File Change Summary

| File | Ph1 | Ph2 | Ph3 | Ph4 | Ph5 |
|------|-----|-----|-----|-----|-----|
| `src/session/mod.rs` | **new** | mod | | | mod |
| `src/tart/mod.rs` | | **new** | | | |
| `src/tart/protocol.rs` | | **new** | | | |
| `src/tart/deploy.rs` | | | | **new** | |
| `src/tart/readiness.rs` | | | | **new** | |
| `src/init_macos.rs` | | | | **new** | |
| `src/session/native.rs` | | | | | **new** |
| `macos/vm-agent.py` | | **new** | | | |
| `macos/a11y-helper/` | | | **new** | | |
| `src/orchestration.rs` | mod | | mod | mod | |
| `src/agent/loop_v2.rs` | mod | | | | |
| `src/setup.rs` | mod | | | | |
| `src/observation.rs` | mod | | mod | mod | |
| `src/readiness.rs` | mod | | | mod | |
| `src/recording.rs` | mod | | | | |
| `src/artifacts.rs` | mod | | | mod | |
| `src/evaluator/*.rs` | mod | | | | |
| `src/interactive.rs` | mod | | | | |
| `src/task.rs` | | | | mod | |
| `src/config.rs` | | | | mod | |
| `src/cli.rs` | | | | mod | |
| `src/preflight.rs` | | | | mod | |
| `Cargo.toml` | | mod | | | |

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Phase 1 touches every consumer file | High (breakage) | Purely mechanical refactor. Full test suite validates. No behavioral change. |
| Shared-dir polling adds latency | Low (10-50ms/command) | Acceptable for desktop testing where actions take seconds. Use small poll intervals. |
| TCC permissions break on macOS updates | Medium | `init-macos` generates csreq blobs from the actual binary signatures. Re-run after macOS updates. |
| LaunchAgent restricted Aqua session | High (empty a11y tree) | **Discovered in Phase 5+.** Workaround: route a11y-helper through `ssh localhost`. |
| `osascript` hangs without TCC Automation | High (VM agent blocks) | **Discovered in Phase 5+.** Replaced with `lsappinfo` which needs no TCC. |
| `tart stop` doesn't flush filesystem | Medium (lost state) | **Discovered in Phase 5+.** Provisioning ends with `sudo shutdown -h now`. |
| Tart 2-VM limit constrains parallelism | Low | Documented limitation. Suite runs serialize macOS tests. |
| `tart run --dir` requires desktest to start VM | Low | Acceptable constraint. No "attach to running VM" for macOS initially. |
| Cross-platform rustfmt divergence | Low (CI failures) | Shorten method chain strings to stay under the threshold where both platforms agree. |

---

## Timeline Expectations

Each phase is independently shippable:
- **Phase 1** can ship as a standalone refactor PR
- **Phases 2–3** can ship together as "macOS infrastructure"
- **Phase 4** ships as "macOS support (beta)"
- **Phase 5** ships as "macOS support (stable)"

Phase 1 is the largest single PR (touches ~15 files) but is the lowest risk (mechanical, no behavior change). Phases 2–5 are additive — they create new files and extend existing ones without modifying the Linux path.
