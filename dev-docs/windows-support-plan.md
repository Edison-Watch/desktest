# Windows Support — Transition Plan

This document describes the phased implementation plan for adding Windows desktop app testing to desktest. It covers architecture decisions, the rollout phases, and what changes in each phase.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Virtualization | QEMU/KVM on Linux hosts | Closest analogy to Tart. KVM gives near-native performance. QCOW2 backing files enable copy-on-write cloning. VirtIO-FS provides shared directories. Linux CI runners are cheap and ubiquitous. |
| Base image distribution | Scripts-only (user provides Windows ISO) | Desktest distributes provisioning scripts and Autounattend.xml — never ISOs or pre-built images. Same model as Packer/Vagrant. Legal basis: Quickemu (MIT) does this without issues. |
| Host ↔ VM communication | Shared directory (VirtIO-FS) + file-based IPC agent | Reuse existing protocol from `src/tart/protocol.rs`. Hypervisor-agnostic. Zero network setup. |
| Accessibility | Python `uiautomation` package (Windows UI Automation COM API) | Pure Python, pip-installable, no compilation needed. No SSH workaround needed (unlike macOS). |
| Action execution | PyAutoGUI (Win32 SendInput backend) | Same as Linux/macOS, different backend. Cross-platform consistency for LLM-generated code. |
| Screenshots | PyAutoGUI/Pillow `ImageGrab.grab()` | Consistent with existing stack. Win32 GDI backend. |
| Guest agent | Python `vm-agent.py` via Task Scheduler | Adapted from `macos/vm-agent.py`. Task Scheduler ONLOGON trigger (analogous to macOS LaunchAgent). |
| Session naming | `src/windows/` + `WindowsVmSession` | Named after guest OS (like Tart implies macOS). `SessionKind::WindowsVm`. |
| LLM system prompt | Windows-specific `Platform::Windows` | Win key shortcuts, PowerShell/cmd references, `tasklist`/`Get-Process` diagnostics. |
| Golden image setup | `desktest init-windows` command (Phase 3) | Single command prepares the QCOW2 golden image with all dependencies. |
| Preflight | Windows is optional mode | `desktest doctor` only warns about Windows deps when Windows tasks are used. |

## Shared-Directory Protocol

Desktest reuses the same file-based IPC protocol from the macOS Tart implementation. The protocol module (`src/tart/protocol.rs`) is extracted to `src/vm_protocol.rs` as a shared module used by both `TartSession` and `WindowsVmSession`.

```
shared_dir/
  agent_ready              # Sentinel file — VM agent writes this on startup
  requests/
    cmd_{pid}_{timestamp}_{counter}.json        # Host writes command request
  responses/
    cmd_{pid}_{timestamp}_{counter}.result.json # VM agent writes command result
  transfers/
    {pid}_{timestamp}_{counter}/                # Staging area for file transfers
```

On macOS (Tart): shared dir mounted at `/Volumes/My Shared Files/desktest`
On Windows (QEMU): shared dir mounted as a drive letter (e.g., `Z:\desktest`) via VirtIO-FS + WinFsp

## QEMU VM Lifecycle

### create() — analogous to `TartSession::create()`

```
1. Generate unique VM name: "desktest-windows-{request_id}"
2. Create shared directory: $TMPDIR/desktest-windows-{vm_name}-shared/
3. Initialize protocol layout (requests/, responses/, transfers/)
4. Create QCOW2 overlay from golden image base:
   qemu-img create -b {base_image} -F qcow2 -f qcow2 {overlay}.qcow2
5. Start virtiofsd for shared directory
6. Spawn QEMU:
   qemu-system-x86_64 -enable-kvm -m 4G -smp 4 \
     -object memory-backend-memfd,id=mem,size=4G,share=on \
     -numa node,memdev=mem \
     -drive file={overlay}.qcow2,if=virtio \
     -chardev socket,id=char0,path={virtiofsd.sock} \
     -device vhost-user-fs-pci,chardev=char0,tag=desktest \
     -display none -vnc :{port} \
     ...
7. Wait for agent_ready sentinel in shared dir (up to 120s)
8. Return WindowsVmSession

Note: VNC ports are allocated by binding to port 0 (OS-assigned ephemeral port) to
avoid TOCTOU races in parallel suite mode. The kernel guarantees a unique port. The
allocated port is stored in the session for debugging access via `vnc :{display}`.
```

### cleanup()

```
1. Send ACPI shutdown to QEMU (graceful)
2. Wait up to 10s, then force-kill QEMU process
3. Stop virtiofsd
4. Delete overlay QCOW2
5. Remove shared directory
```

### Stale session cleanup

Analogous to Tart's `cleanup_stale_shared_dirs()`, `WindowsVmSession::create()` runs `cleanup_stale_windows_vms()` on startup. This scans for orphaned resources from crashed sessions:

1. Scan `$TMPDIR` for `desktest-windows-*-shared/` directories without a running QEMU process
2. Kill any orphaned `virtiofsd` daemons (match by socket path in the shared dir)
3. Delete orphaned QCOW2 overlay files
4. Remove the stale shared directories

This prevents disk and process leaks from accumulating after repeated test failures or crashes.

### Key Differences from TartSession

| Aspect | TartSession | WindowsVmSession |
|--------|-------------|-------------------|
| VM binary | `tart run` | `qemu-system-x86_64` |
| Cloning | `tart clone {base} {name}` | `qemu-img create -b {base} -F qcow2 -f qcow2 {overlay}` |
| Shared dir mechanism | `--dir=desktest:{path}` (Tart built-in) | VirtIO-FS via virtiofsd daemon |
| Guest mount point | `/Volumes/My Shared Files/desktest` | Drive letter (e.g., `Z:\desktest`) via WinFsp |
| Cleanup | `tart stop` + `tart delete` | ACPI shutdown + delete overlay QCOW2 |
| Shell | `bash` | `powershell` / `cmd` |
| Temp path | `/tmp/` | `C:\Temp\` |
| Process list | `ps aux` | `tasklist` |

## Golden Image Provisioning

A `windows/provision.ps1` script (run during `desktest init-windows`) configures:

1. **Install Python 3** (via winget or embedded installer)
2. **Install PyAutoGUI + Pillow + uiautomation** (pip)
3. **Install VirtIO drivers + WinFsp** (for VirtIO-FS shared folder)
4. **Deploy agent scripts** (vm-agent.py, execute-action.py, get-a11y-tree.py, win-screenshot.py)
5. **Register vm-agent as scheduled task** (ONLOGON trigger, HIGHEST privilege)
6. **Disable UAC** (registry: `ConsentPromptBehaviorAdmin = 0`)
7. **Disable Windows Defender real-time** (`Set-MpPreference -DisableRealtimeMonitoring $true`)
8. **Disable screen lock / screensaver** (registry)
9. **Configure auto-login** for test user (registry: `AutoAdminLogon`)
10. **Disable Windows Update** (stop & disable `wuauserv`)
11. **Set display resolution** to 1920x1080
12. **Enable OpenSSH Server** (for provisioning access)

## Observation Commands

| Platform | Screenshot | Accessibility Tree |
|----------|-----------|-------------------|
| Linux (Docker) | `scrot -o -p /tmp/screenshot.png` | `/usr/local/bin/get-a11y-tree` (pyatspi) |
| macOS (Tart/Native) | `screencapture -x /tmp/screenshot.png` | `ssh localhost /usr/local/bin/a11y-helper` (Swift AXUIElement) |
| **Windows (QEMU)** | `python C:\desktest\win-screenshot.py` | `python C:\desktest\get-a11y-tree.py` (UIA via `uiautomation` package) |

Note: Windows does not need the macOS SSH localhost workaround — the agent process running in the interactive user session has full UI Automation access by default.

## Licensing Considerations

Desktest is MIT-licensed. For Windows VM support:

- **Distributed by desktest:** Provisioning scripts, Autounattend.xml, QEMU configs, Python agent scripts. All original code under MIT.
- **User provides:** Windows ISO (evaluation or licensed). Users are responsible for Windows licensing compliance.
- **VirtIO drivers:** BSD-3-Clause (fully MIT-compatible). Downloaded during provisioning.
- **WinFsp:** GPLv3 with FLOSS exception (MIT qualifies). Installed inside guest VM, not bundled with desktest.
- **Precedent:** Quickemu (MIT), Packer (was MPL), Vagrant (was MIT) all manage Windows VMs without legal issues.

## Files to Create/Modify

### New Files

| File | Purpose |
|------|---------|
| `src/vm_protocol.rs` | Extracted shared IPC protocol from `src/tart/protocol.rs` |
| `src/windows/mod.rs` | `WindowsVmSession`: QEMU lifecycle, Session impl via ProtocolClient |
| `src/windows/deploy.rs` | App deployment into Windows VM |
| `src/windows/readiness.rs` | Desktop/app readiness detection for Windows |
| `windows/vm-agent.py` | Guest agent polling shared dir (adapted from `macos/vm-agent.py`) |
| `windows/execute-action.py` | PyAutoGUI executor for Windows |
| `windows/get-a11y-tree.py` | UIA accessibility tree extraction |
| `windows/win-screenshot.py` | Screenshot capture wrapper |

### Modified Files

| File | Changes |
|------|---------|
| `src/tart/mod.rs` | Import from `crate::vm_protocol` instead of local `protocol` |
| `src/session/mod.rs` | Add `WindowsVm` variant, `as_windows_vm()`, update `forward_session!` macro |
| `src/agent/context.rs` | Add `Platform::Windows` + Windows-specific system prompt content |
| `src/task.rs` | Add `AppConfig::WindowsVm { base_image, app_path, launch_cmd, installer_cmd }` |
| `src/observation.rs` | Add Windows screenshot/a11y commands and match arm |
| `src/orchestration.rs` | Add Windows VM branching for session creation, deploy, launch, readiness |
| `src/preflight.rs` | Add `check_windows_vm()` for QEMU/KVM availability |
| `src/setup.rs` | Windows shell handling (`powershell -Command`, `Start-Process`) |
| `src/artifacts.rs` | Windows `tasklist`, Windows temp paths |
| `src/main.rs` | Add `mod vm_protocol;` and `mod windows;` |

## Implementation Phases

### Phase 1: Minimum Viable Windows VM Test

**Goal:** Run a basic Windows E2E test against a manually prepared Windows QCOW2 image on a Linux host with KVM.

**1a — Protocol extraction** (safe refactor, no new functionality):
1. Extract `src/tart/protocol.rs` to `src/vm_protocol.rs`
2. Rename Tart-specific error messages to be VM-agnostic (e.g., "Tart VM agent" → "VM agent", "Tart request" → "VM request") and update the corresponding test assertions in `src/tart/protocol.rs` that match on those strings (e.g., `send_request_returns_agent_error_when_error_field_set`, `send_request_returns_error_on_malformed_response`, `send_request_timeout`, `wait_for_agent_ready_timeout`)
3. Update `src/tart/mod.rs` to import from `crate::vm_protocol`
4. Verify existing Tart tests still pass

**1b — Core session infrastructure:**
5. Add `Platform::Windows` to `src/agent/context.rs`
6. Add `AppConfig::WindowsVm` to `src/task.rs`
7. Create `src/windows/mod.rs` with `WindowsVmSession`
8. Update `src/session/mod.rs` with `WindowsVm` variant

**1c — Guest-side scripts:**
9. Create `windows/vm-agent.py` (adapt from `macos/vm-agent.py`)
10. Create `windows/execute-action.py` (adapt from `docker/execute-action.py`)
11. Create `windows/get-a11y-tree.py` (UIA tree extraction)
12. Create `windows/win-screenshot.py`

**1d — Orchestration wiring:**
13. Update `src/observation.rs` with Windows commands
14. Create `src/windows/deploy.rs` and `src/windows/readiness.rs`
15. Update `src/orchestration.rs` with Windows VM branches
16. Update `src/setup.rs` for PowerShell execution
17. Update `src/preflight.rs` and `src/artifacts.rs`
18. Add Windows-specific system prompt content

### Phase 2: Polish, Recording, and Native Windows

1. `WindowsNativeSession` for testing on Windows hosts (cross-compile desktest for Windows)
2. ffmpeg `gdigrab` recording support
3. Full artifact collection with Windows paths
4. Interactive mode support
5. Refine a11y tree quality (UIA property selection, output format tuning)

### Phase 3: Golden Image Automation and CI

1. `desktest init-windows` command
2. `windows/provision.ps1` and `windows/Autounattend.xml`
3. CI/CD integration guide (GitHub Actions with KVM, etc.)
4. Windows-specific test examples

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| Windows licensing | Scripts-only distribution; user provides own ISO; document evaluation ISO path |
| VirtIO-FS on Windows needs WinFsp | Provisioning script installs WinFsp; fallback to SMB share if VirtIO-FS fails |
| QEMU startup time (~30-60s) | Accept; QCOW2 overlays are instant; parallelize in suite mode |
| Windows UI Automation tree verbosity | `--max-nodes` filtering; tune defaults for Windows |
| virtiofsd setup complexity | Provide helper script; document host prerequisites |
| PyAutoGUI `type_text` differences | Windows `execute-action.py` needs a dedicated `type_text()` helper — `pyautogui.write()` has the same backslash/special-character limitations as `typewrite()`. Use `ctypes` + Win32 `SendInput` for reliable Unicode text entry, mirroring how the Linux version uses `xdotool type`. |
