# macOS Support

Desktest's macOS support enables E2E testing of native macOS desktop applications using the same LLM-powered agent loop used for Linux. macOS support requires Apple Silicon hardware and uses [Tart](https://github.com/cirruslabs/tart) VMs for clean, isolated test environments.

## Architecture

On Linux, desktest runs tests inside Docker containers with a virtual X11 desktop. On macOS, Docker cannot host macOS guests (both technically and legally), so desktest uses **Tart VMs** — lightweight macOS virtual machines powered by Apple's Virtualization.framework.

```
Linux                              macOS
─────────────────                  ─────────────────
Docker container                   Tart macOS VM
  Xvfb + XFCE desktop               Native macOS desktop
  PyAutoGUI (X11/python-xlib)        PyAutoGUI (Quartz/CoreGraphics)
  pyatspi (AT-SPI2)                  Swift a11y helper (AXUIElement)
  scrot (screenshot)                 screencapture (screenshot)
  ffmpeg x11grab (recording)         screencapture -V (recording)
```

### Two Operating Modes

| Mode | Environment | Isolation | Use Case |
|------|-------------|-----------|----------|
| `macos_tart` | Tart VM (cloned from golden image) | Full VM isolation, destroyed after test | CI, reproducible tests |
| `macos_native` | Host macOS desktop (no VM) | None — runs on your real desktop | Quick local iteration, debugging |

## Requirements

- **Apple Silicon Mac** (M1 or later) — Virtualization.framework is ARM-only for macOS guests
- **macOS 13 Ventura or later** — required for Virtualization.framework macOS guest support
- **Tart installed** — `brew install cirruslabs/cli/tart` (for `macos_tart` mode)
- **Python 3 + PyAutoGUI** — installed in the VM golden image (or on host for native mode)
- **Xcode Command Line Tools** — `xcode-select --install`

## Quick Start

### 1. Prepare a golden image

```bash
# Install Tart
brew install cirruslabs/cli/tart

# Create the golden image (downloads base macOS, provisions tools)
desktest init-macos --base-image ghcr.io/cirruslabs/macos-sequoia-base:latest \
                    --output-image desktest-macos:latest

# For Electron apps, add --with-electron
desktest init-macos --base-image ghcr.io/cirruslabs/macos-sequoia-base:latest \
                    --output-image desktest-macos-electron:latest \
                    --with-electron
```

### 2. Write a task file

```json
{
  "schema_version": "1.0",
  "id": "macos-textedit-hello",
  "instruction": "Open a new document in TextEdit, type 'Hello World', and save as 'hello.txt' on the Desktop.",
  "app": {
    "type": "macos_tart",
    "base_image": "desktest-macos:latest",
    "bundle_id": "com.apple.TextEdit"
  },
  "config": [],
  "evaluator": {
    "mode": "hybrid",
    "metrics": [
      {
        "type": "file_exists",
        "path": "/Users/admin/Desktop/hello.txt"
      },
      {
        "type": "command_output",
        "command": "cat /Users/admin/Desktop/hello.txt",
        "match_mode": "contains",
        "expected": "Hello World"
      }
    ]
  },
  "timeout": 180,
  "max_steps": 20
}
```

### 3. Run the test

```bash
desktest run examples/macos-textedit.json --config config.json
```

### 4. Native mode (no VM)

For quick iteration without a VM, use `macos_native`:

```json
{
  "schema_version": "1.0",
  "id": "native-textedit",
  "instruction": "Open TextEdit and type 'Hello'",
  "app": {
    "type": "macos_native",
    "bundle_id": "com.apple.TextEdit"
  },
  "config": [],
  "timeout": 120,
  "max_steps": 10
}
```

**Warning:** Native mode runs on your actual desktop with no isolation. The test agent will control your mouse and keyboard.

### Permissions (TCC)

macOS requires explicit grants for Accessibility and Screen Recording. **`desktest init-macos` handles this automatically** — it inserts TCC database entries with proper code signing requirement (`csreq`) blobs during provisioning. This requires SIP to be disabled in the base image (the Cirrus Labs base images ship with SIP disabled).

The permissions granted automatically:

- **Accessibility** (`kTCCServiceAccessibility`) — for PyAutoGUI mouse/keyboard control and the a11y-helper
- **Screen Recording** (`kTCCServiceScreenCapture`) — for `screencapture` and PyAutoGUI screenshot functions
- **Post Event** (`kTCCServicePostEvent`) — for synthesizing keyboard/mouse input
- **Automation** (`kTCCServiceAppleEvents`) — for osascript → System Events

If you need to grant permissions manually, see [TCC Database Setup](#tcc-database-setup) below.

## Apple Terms of Service

Desktest's macOS testing approach is fully compliant with Apple's macOS Software License Agreement. Here is a summary of the relevant terms and how they apply.

### What is permitted

- **UI automation via accessibility APIs**: Apple explicitly provides AXUIElement, NSAccessibility, and related APIs as public frameworks. Apple's own XCUITest is built on the same accessibility infrastructure. Using these APIs for automated testing is an intended use case.

- **PyAutoGUI / programmatic input**: PyAutoGUI on macOS uses CoreGraphics (`CGEvent`) for mouse/keyboard and CoreGraphics screen capture APIs for screenshots. These are public Apple APIs with no usage restrictions beyond the runtime permission gates (Accessibility, Screen Recording).

- **Screenshot capture**: `screencapture` is a built-in macOS utility. Programmatic capture via CoreGraphics is also unrestricted. The macOS permission system exists to protect user privacy, not to restrict legitimate testing.

- **Running macOS in VMs on Apple hardware**: The macOS SLA permits running up to 2 additional macOS instances in virtual machines, provided the VMs run on the same Apple-branded hardware as the host.

- **LLM-driven automation**: Apple's terms do not restrict what software makes API calls. The fact that an LLM agent drives the automation is irrelevant to licensing.

### Restrictions and limitations

- **Apple hardware required**: macOS may only run on Apple-branded hardware, whether as host or in a VM. Running macOS on non-Apple hardware (e.g., Hackintosh, generic x86 cloud VMs) violates the SLA. This means macOS desktest tests can only run on physical Macs (local, EC2 Mac, MacStadium, etc.).

- **2-VM limit per host**: The macOS SLA permits a maximum of 2 macOS VM instances running simultaneously per physical Mac. This is also enforced technically by Virtualization.framework (a third VM fails with `VZErrorDomain Code 6`). This limits parallel test execution — a single Mac can run at most 2 tests concurrently.

- **No macOS in Docker**: macOS cannot run inside Docker/OCI containers. There are no macOS container images, and distributing macOS as a container image would violate Apple's redistribution terms. This is why desktest uses Tart VMs instead of extending the existing Docker-based approach.

- **No macOS on non-Apple cloud**: Standard cloud VMs (AWS EC2 x86, GCP, Azure) cannot run macOS. Only dedicated Apple hardware offerings (AWS EC2 Mac instances, MacStadium, etc.) are compliant.

### Precedent

Many widely-used open source projects perform macOS UI automation without any known Apple enforcement issues:

- **Appium** (with Mac2 driver) — cross-platform UI automation
- **Hammerspoon** — Lua-scriptable macOS automation via accessibility APIs
- **cliclick** — CLI mouse/keyboard simulation
- **AppleScript + System Events** — Apple's own scripting bridge for UI automation

### Commercial CI services operating within Apple's terms

- **AWS EC2 Mac** — dedicated Mac mini hardware in AWS data centers, direct licensing agreement with Apple
- **MacStadium / Orka** — data centers of physical Mac hardware with Apple licensing agreements
- **Cirrus CI / Tart** — ephemeral macOS VMs on Apple Silicon, open source

## Platform Comparison

| Aspect | Linux (Docker) | macOS (Tart VM) | Windows (QEMU/KVM) |
|--------|---------------|-----------------|---------------------|
| Parallel tests per host | Unlimited (Docker containers) | Max 2 (Apple VM limit) | Unlimited (limited by host resources) |
| Environment startup | ~5s (container start) | ~5-7s (VM clone + resume) | ~30-60s (QCOW2 overlay + boot) |
| State isolation | Complete (container destroyed) | Complete (VM clone destroyed) | Complete (QCOW2 overlay destroyed) |
| Accessibility tree | pyatspi / AT-SPI2 (mature) | AXUIElement via Swift helper (via SSH localhost) | uiautomation (Windows UI Automation COM API) |
| Action execution | PyAutoGUI via X11 | PyAutoGUI via Quartz/CoreGraphics | PyAutoGUI via Win32 SendInput |
| Screenshots | scrot (X11) | screencapture (built-in) | PIL ImageGrab (Win32 GDI) |
| Video recording | ffmpeg x11grab | screencapture -V (built-in) | ffmpeg gdigrab |
| App deployment | Copy into container | Pre-installed in VM image or `open -a` | Copy via shared dir or `launch_cmd` |
| Host↔Guest comms | Docker exec API | VirtIO-FS shared dir (file-based IPC) | VirtIO-FS shared dir (file-based IPC) |
| Security boundary | Docker container (non-root user) | VM boundary (stronger) | VM boundary (stronger) |
| Headless operation | Native (Xvfb) | Requires VM (no headless macOS desktop) | Native (QEMU `-display none`) |
| CI infrastructure | Any Linux host with Docker | Apple Silicon hardware only | Linux host with KVM |

## Clean State Management

Tart VMs provide clean state through an ephemeral clone workflow:

1. **Prepare golden image**: `desktest init-macos` creates a Tart VM with macOS, installs Python, PyAutoGUI, the Swift a11y helper, execute-action script, sets up SSH keys for a11y access, configures TCC permissions, and optionally installs Node.js for Electron
2. **Before each test**: `tart clone golden-image test-run-N` (near-instant, APFS copy-on-write)
3. **Run test**: Boot the clone, run desktest agent loop against it
4. **After test**: `tart delete test-run-N` (clone destroyed, golden image unchanged)

This mirrors Docker's ephemeral container model. Each test gets a pristine environment.

### Alternative: Native mode (no isolation)

For local development, `macos_native` mode skips the VM entirely and runs on the host desktop. This is faster but provides no state isolation. Optional cleanup can reset `~/Library/Preferences` and `~/Library/Application Support` for the tested app between runs.

## TCC Database Setup

`desktest init-macos` handles TCC permission grants automatically. This section documents the manual process for custom images or debugging.

**Important**: TCC entries require valid **code signing requirement** (`csreq`) blobs on modern macOS. Entries with NULL csreq may be silently ignored even with SIP disabled.

### Prerequisites

- SIP must be disabled in the VM (`csrutil disable` from Recovery Mode)
- The Cirrus Labs base images ship with SIP disabled

### Granting permissions with csreq blobs

```bash
# Helper function to grant a TCC permission with proper csreq
grant_tcc() {
    local SERVICE="$1"
    local CLIENT="$2"   # absolute path to binary
    local INDIRECT="${3:-UNUSED}"

    # Generate csreq blob from the binary's code signing requirement
    local REQ=$(codesign -d -r- "$CLIENT" 2>&1 | sed -n 's/.*designated => //p')
    local CSREQ_SQL="NULL"
    if [ -n "$REQ" ]; then
        local HEX=$(echo "$REQ" | csreq -r- -b /dev/stdout 2>/dev/null | xxd -p | tr -d '\n')
        if [ -n "$HEX" ]; then
            CSREQ_SQL="X'${HEX}'"
        fi
    fi

    sudo sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" \
      "INSERT OR REPLACE INTO access \
        (service, client, client_type, auth_value, auth_reason, auth_version, \
         csreq, policy_id, indirect_object_identifier_type, indirect_object_identifier, \
         indirect_object_code_identity, flags, last_modified) \
        VALUES ('${SERVICE}', '${CLIENT}', 1, 2, 4, 1, \
                ${CSREQ_SQL}, NULL, 0, '${INDIRECT}', NULL, 0, \
                $(date +%s));"
}

# Grant permissions to the a11y-helper
grant_tcc kTCCServiceAccessibility /usr/local/bin/a11y-helper

# Grant permissions to Python (use the Homebrew path, not /usr/bin/python3)
PYTHON_BIN="/opt/homebrew/bin/python3"
grant_tcc kTCCServiceAccessibility "$PYTHON_BIN"
grant_tcc kTCCServiceScreenCapture "$PYTHON_BIN"
grant_tcc kTCCServicePostEvent "$PYTHON_BIN"
grant_tcc kTCCServiceAppleEvents "$PYTHON_BIN" com.apple.systemevents

# Grant Screen Recording to screencapture
grant_tcc kTCCServiceScreenCapture /usr/sbin/screencapture
```

### Key details

- `client_type=1` means absolute path. Use `client_type=0` for bundle IDs (e.g., `com.apple.Terminal`)
- `auth_value=2` means allowed, `auth_reason=4` signals admin override (SIP-disabled context)
- The `csreq` blob is generated from the binary's code signature via `codesign -d -r-` and the `csreq` tool. Ad-hoc signed binaries (like Homebrew Python) work — the cdhash is used as the requirement
- TCC entries are stored in the **system-level** database at `/Library/Application Support/com.apple.TCC/TCC.db` (not the user-level one)

### Accessibility tree and SSH localhost

The a11y-helper must be invoked via `ssh localhost` rather than directly, because:

- The vm-agent runs as a LaunchAgent, which gets a **restricted Aqua session** from macOS
- Direct subprocess calls from this context return empty accessibility trees
- SSH sessions inherit full TCC permissions from `sshd-keygen-wrapper` and get a proper Aqua session handle

`desktest init-macos` sets up passwordless SSH keys automatically. For manual setup:

```bash
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519 -N "" -q
cat ~/.ssh/id_ed25519.pub >> ~/.ssh/authorized_keys
chmod 600 ~/.ssh/authorized_keys
```

### Saving the golden image

After configuring permissions, shut down the VM **gracefully** to ensure filesystem changes are flushed:

```bash
sudo shutdown -h now
```

Then clone it on the host: `tart clone work-vm desktest-macos:latest`

> **Note**: `tart stop` does not guarantee filesystem flush. Always use `sudo shutdown -h now` inside the VM before cloning.
